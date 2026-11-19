//! Gestion des utilisateurs (ADMIN uniquement).

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::{check_password_policy, hash_password, CurrentUser},
    domain::{
        roles::{Permission, Role},
        validation::is_valid_email,
    },
    error::{AppError, AppResult},
    modules::audit,
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
// Compte tel que renvoyé à l'interface (jamais le hachage du mot de passe).
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub active: bool,
    pub created_at: String,
}

// Colonnes communes à toutes les lectures ; `concat!` garde une chaîne SQL constante (exigence de sqlx).
macro_rules! select_users {
    ($tail:literal) => {
        concat!("SELECT id, email, display_name, role, active, created_at FROM users ", $tail)
    };
}

// Liste de tous les comptes, actifs ou non.
pub async fn list(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<UserRow>>> {
    user.require(Permission::ManageUsers)?;
    let rows = sqlx::query_as::<_, UserRow>(select_users!("ORDER BY display_name COLLATE NOCASE"))
        .fetch_all(&st.pool)
        .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
// Données de création d'un compte ; le mot de passe initial est fixé par l'administrateur.
pub struct CreateUser {
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub password: String,
}

// Création d'un compte : contrôles de forme, hachage, insertion, trace d'audit.
pub async fn create(
    State(st): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateUser>,
) -> AppResult<Json<UserRow>> {
    user.require(Permission::ManageUsers)?;
    let email = req.email.trim().to_lowercase();
    let name = req.display_name.trim().to_string();
    if !is_valid_email(&email) {
        return Err(AppError::Validation("Adresse email invalide".into()));
    }
    if name.is_empty() {
        return Err(AppError::Validation("Le nom est obligatoire".into()));
    }
    let role = Role::parse(&req.role).ok_or_else(|| AppError::Validation("Rôle inconnu".into()))?;
    check_password_policy(&req.password)?;
    let hash = hash_password(req.password).await?;

    let mut tx = st.pool.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (email, display_name, password_hash, role) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(&email)
    .bind(&name)
    .bind(hash)
    .bind(role.as_str())
    .fetch_one(&mut *tx)
    .await
    // L'email est unique : message explicite en cas de doublon.
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("Un utilisateur utilise déjà cet email".into()),
        other => other,
    })?;
    audit::record(&mut tx, Some(user.id), "CREATION", "user", Some(id.to_string()),
        Some(json!({ "email": email, "display_name": name, "role": role.as_str() }))).await?;
    let row = sqlx::query_as::<_, UserRow>(select_users!("WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(row))
}

#[derive(Deserialize)]
// Modification partielle : seuls les champs présents changent.
pub struct UpdateUser {
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub active: Option<bool>,
    pub new_password: Option<String>,
}

// Modification d'un compte avec deux garde-fous : on ne se désactive pas soi-même
// et il reste toujours au moins un administrateur actif.
pub async fn update(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUser>,
) -> AppResult<Json<UserRow>> {
    user.require(Permission::ManageUsers)?;
    // Réinitialisation éventuelle du mot de passe (hachage hors transaction, il est lent).
    let new_hash = match &req.new_password {
        Some(p) => {
            check_password_policy(p)?;
            Some(hash_password(p.clone()).await?)
        }
        None => None,
    };

    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = sqlx::query_as::<_, UserRow>(select_users!("WHERE id = ?"))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Utilisateur introuvable".into()))?;

    let name = match &req.display_name {
        Some(n) if n.trim().is_empty() => return Err(AppError::Validation("Le nom est obligatoire".into())),
        Some(n) => n.trim().to_string(),
        None => before.display_name.clone(),
    };
    let role = match &req.role {
        Some(r) => Role::parse(r).ok_or_else(|| AppError::Validation("Rôle inconnu".into()))?,
        None => Role::parse(&before.role).ok_or_else(|| AppError::internal("rôle inconnu"))?,
    };
    let active = req.active.unwrap_or(before.active);

    // Garde-fou 1 : auto-désactivation interdite.
    if id == user.id && !active {
        return Err(AppError::Validation("Vous ne pouvez pas désactiver votre propre compte".into()));
    }
    let was_admin = before.role == "ADMIN" && before.active;
    let stays_admin = role == Role::Admin && active;
    // Garde-fou 2 : pas de perte du dernier administrateur.
    if was_admin && !stays_admin {
        let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'ADMIN' AND active = 1")
            .fetch_one(&mut *tx)
            .await?;
        if admins <= 1 {
            return Err(AppError::Validation("Il doit rester au moins un administrateur actif".into()));
        }
    }

    // COALESCE : le hachage n'est remplacé que si un nouveau mot de passe a été fourni.
    sqlx::query(
        "UPDATE users SET display_name = ?, role = ?, active = ?,
            password_hash = COALESCE(?, password_hash),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(&name)
    .bind(role.as_str())
    .bind(active)
    .bind(&new_hash)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    // Désactivation ou nouveau mot de passe : les sessions ouvertes sont fermées.
    if !active || new_hash.is_some() {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&mut *tx).await?;
    }

    let after = sqlx::query_as::<_, UserRow>(select_users!("WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let mut changes = audit::diff(
        &json!({ "display_name": before.display_name, "role": before.role, "active": before.active }),
        &json!({ "display_name": after.display_name, "role": after.role, "active": after.active }),
    );
    if new_hash.is_some() {
        changes["password"] = json!("réinitialisé");
    }
    audit::record(&mut tx, Some(user.id), "MODIFICATION", "user", Some(id.to_string()), Some(changes)).await?;
    tx.commit().await?;
    Ok(Json(after))
}
