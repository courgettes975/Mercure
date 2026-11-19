//! Authentification par session (cookie httpOnly) et contrôle des rôles.

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::{
    extract::{FromRequestParts, State},
    http::request::Parts,
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::{
    domain::{
        roles::{Permission, Role},
        validation::is_valid_email,
    },
    error::{AppError, AppResult},
    modules::audit,
    state::AppState,
};

// Nom du cookie de session.
pub const COOKIE_NAME: &str = "erp_session";
// Durée de vie d'une session (heures).
const SESSION_HOURS: i64 = 12;
// Longueur minimale d'un mot de passe.
pub const MIN_PASSWORD_LEN: usize = 10;
// Nombre d'échecs de connexion avant verrouillage temporaire...
const MAX_FAILURES: u32 = 5;
// ... et durée du verrouillage (15 minutes).
const LOCK_SECONDS: u64 = 15 * 60;

/// Limitation des tentatives de connexion (mémoire, par email).
#[derive(Clone, Default)]
pub struct LoginGuard(Arc<Mutex<HashMap<String, (u32, Instant)>>>);

impl LoginGuard {
    // Vrai si l'email a atteint le maximum d'échecs et que le délai n'est pas écoulé.
    fn is_locked(&self, email: &str) -> bool {
        let map = self.0.lock().unwrap();
        matches!(map.get(email), Some((n, t)) if *n >= MAX_FAILURES && t.elapsed().as_secs() < LOCK_SECONDS)
    }
    // Enregistre un échec ; le compteur repart de zéro si le verrou précédent est expiré.
    fn fail(&self, email: &str) {
        let mut map = self.0.lock().unwrap();
        let e = map.entry(email.to_string()).or_insert((0, Instant::now()));
        if e.1.elapsed().as_secs() >= LOCK_SECONDS {
            *e = (0, Instant::now());
        }
        e.0 += 1;
        e.1 = Instant::now();
    }
    // Connexion réussie : on oublie les échecs de cet email.
    fn success(&self, email: &str) {
        self.0.lock().unwrap().remove(email);
    }
}

#[derive(Debug, Clone, Serialize)]
// Utilisateur connecté, extrait automatiquement du cookie par Axum (voir FromRequestParts).
pub struct CurrentUser {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub role: Role,
}

impl CurrentUser {
    // Vérifie une permission ; renvoie 403 sinon. Appelé en tête de chaque handler sensible.
    pub fn require(&self, p: Permission) -> AppResult<()> {
        if self.role.can(p) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

// Les jetons de session sont stockés hachés : une fuite de la base ne permet pas de les rejouer.
fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

// Hachage Argon2 (lent par conception) exécuté hors du fil asynchrone.
pub async fn hash_password(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(AppError::internal)
    })
    .await
    .map_err(AppError::internal)?
}

// Vérification Argon2 ; toute erreur vaut "mot de passe incorrect".
async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        Argon2::default().verify_password(password.as_bytes(), hash.as_str()).is_ok()
    })
    .await
    .unwrap_or(false)
}

// Hachage factice calculé une fois : utilisé quand l'email est inconnu pour que le temps de réponse
// ne révèle pas si un compte existe.
static DUMMY_HASH: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();

async fn dummy_hash() -> String {
    DUMMY_HASH
        .get_or_init(|| async { hash_password("mot-de-passe-factice".into()).await.unwrap_or_default() })
        .await
        .clone()
}

// Politique de mot de passe : uniquement une longueur minimale (pas de règles de composition).
pub fn check_password_policy(p: &str) -> AppResult<()> {
    if p.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::Validation(format!(
            "Le mot de passe doit contenir au moins {MIN_PASSWORD_LEN} caractères"
        )));
    }
    Ok(())
}

// Extraction de l'utilisateur courant : cookie -> session non expirée -> utilisateur actif.
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get(COOKIE_NAME).map(|c| c.value().to_string()).ok_or(AppError::Unauthorized)?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let row: Option<(i64, String, String, String)> = sqlx::query_as(
            "SELECT u.id, u.email, u.display_name, u.role
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND s.expires_at > ? AND u.active = 1",
        )
        .bind(hash_token(&token))
        .bind(now)
        .fetch_optional(&st.pool)
        .await?;
        let (id, email, display_name, role) = row.ok_or(AppError::Unauthorized)?;
        let role = Role::parse(&role).ok_or_else(|| AppError::internal("rôle inconnu"))?;
        Ok(CurrentUser { id, email, display_name, role })
    }
}

// Crée une session : jeton aléatoire de 32 octets, stocké haché, renvoyé au navigateur en cookie
// httpOnly + SameSite=Strict (inaccessible au JavaScript, non envoyé depuis un autre site).
async fn open_session(st: &AppState, jar: CookieJar, user_id: i64) -> AppResult<CookieJar> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let expires = (Utc::now() + Duration::hours(SESSION_HOURS)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // Nettoyage des sessions expirées.
    sqlx::query("DELETE FROM sessions WHERE expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
        .execute(&st.pool)
        .await?;
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)")
        .bind(hash_token(&token))
        .bind(user_id)
        .bind(expires)
        .execute(&st.pool)
        .await?;
    let cookie = Cookie::build((COOKIE_NAME, token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(st.secure_cookies)
        .path("/")
        .max_age(time::Duration::hours(SESSION_HOURS))
        .build();
    Ok(jar.add(cookie))
}

#[derive(Serialize)]
// Réponse de /auth/status : l'ERP attend-il la création du premier compte ?
pub struct StatusResponse {
    pub setup_required: bool,
}

// Aucun utilisateur en base = assistant de première installation côté interface.
pub async fn status(State(st): State<AppState>) -> AppResult<Json<StatusResponse>> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&st.pool).await?;
    Ok(Json(StatusResponse { setup_required: n == 0 }))
}

#[derive(Deserialize)]
// Données du premier compte administrateur.
pub struct SetupRequest {
    pub email: String,
    pub display_name: String,
    pub password: String,
}

/// Création du premier administrateur. Possible uniquement si aucun utilisateur n'existe.
pub async fn setup(
    State(st): State<AppState>,
    jar: CookieJar,
    Json(req): Json<SetupRequest>,
) -> AppResult<(CookieJar, Json<CurrentUser>)> {
    let email = req.email.trim().to_lowercase();
    let name = req.display_name.trim().to_string();
    if !is_valid_email(&email) {
        return Err(AppError::Validation("Adresse email invalide".into()));
    }
    if name.is_empty() {
        return Err(AppError::Validation("Le nom est obligatoire".into()));
    }
    check_password_policy(&req.password)?;
    let hash = hash_password(req.password).await?;

    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&mut *tx).await?;
    if n > 0 {
        return Err(AppError::Conflict("L'application est déjà initialisée".into()));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (email, display_name, password_hash, role) VALUES (?, ?, ?, 'ADMIN') RETURNING id",
    )
    .bind(&email)
    .bind(&name)
    .bind(hash)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(&mut tx, Some(id), "INITIALISATION", "user", Some(id.to_string()), Some(json!({ "email": email, "role": "ADMIN" }))).await?;
    tx.commit().await?;

    let jar = open_session(&st, jar, id).await?;
    Ok((jar, Json(CurrentUser { id, email, display_name: name, role: Role::Admin })))
}

#[derive(Deserialize)]
// Identifiants de connexion.
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

// Connexion : verrou anti-force brute, vérification du mot de passe, ouverture de session, trace d'audit.
pub async fn login(
    State(st): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> AppResult<(CookieJar, Json<CurrentUser>)> {
    let email = req.email.trim().to_lowercase();
    if st.login_guard.is_locked(&email) {
        return Err(AppError::Validation(
            "Trop de tentatives. Réessayez dans 15 minutes.".into(),
        ));
    }
    let row: Option<(i64, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, display_name, password_hash, role, active FROM users WHERE email = ?",
    )
    .bind(&email)
    .fetch_optional(&st.pool)
    .await?;

    // Hachage factice si l'utilisateur n'existe pas : temps de réponse homogène.
    let dummy = dummy_hash().await;
    let (ok, user) = match row {
        Some((id, name, hash, role, active)) => {
            let ok = verify_password(req.password, hash).await && active == 1;
            (ok, Some((id, name, role)))
        }
        None => {
            verify_password(req.password, dummy).await;
            (false, None)
        }
    };
    let Some((id, name, role)) = user.filter(|_| ok) else {
        st.login_guard.fail(&email);
        return Err(AppError::BadCredentials);
    };
    st.login_guard.success(&email);
    let role = Role::parse(&role).ok_or_else(|| AppError::internal("rôle inconnu"))?;
    let jar = open_session(&st, jar, id).await?;
    let mut conn = st.pool.acquire().await?;
    audit::record(&mut conn, Some(id), "CONNEXION", "user", Some(id.to_string()), None).await?;
    Ok((jar, Json(CurrentUser { id, email, display_name: name, role })))
}

// Déconnexion : suppression de la session côté serveur et effacement du cookie.
pub async fn logout(State(st): State<AppState>, jar: CookieJar) -> AppResult<CookieJar> {
    if let Some(c) = jar.get(COOKIE_NAME) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(hash_token(c.value()))
            .execute(&st.pool)
            .await?;
    }
    Ok(jar.remove(Cookie::build(COOKIE_NAME).path("/").build()))
}

// Renvoie l'utilisateur courant (utilisé par l'interface au chargement).
pub async fn me(user: CurrentUser) -> Json<CurrentUser> {
    Json(user)
}

#[derive(Deserialize)]
// Changement de mot de passe par l'utilisateur lui-même.
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

// L'ancien mot de passe est exigé pour éviter qu'une session oubliée serve à changer le mot de passe.
pub async fn change_password(
    State(st): State<AppState>,
    user: CurrentUser,
    Json(req): Json<ChangePasswordRequest>,
) -> AppResult<Json<serde_json::Value>> {
    check_password_policy(&req.new_password)?;
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&st.pool)
        .await?;
    if !verify_password(req.current_password, hash).await {
        return Err(AppError::Validation("Mot de passe actuel incorrect".into()));
    }
    let new_hash = hash_password(req.new_password).await?;
    let mut tx = st.pool.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
        .bind(new_hash)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, Some(user.id), "CHANGEMENT_MOT_DE_PASSE", "user", Some(user.id.to_string()), None).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
