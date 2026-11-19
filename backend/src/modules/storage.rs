//! Paramètres > Stockage : emplacement du dossier de données (partageable entre plusieurs PC).

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::{
    auth::CurrentUser,
    config,
    domain::roles::Permission,
    error::{AppError, AppResult},
    modules::audit,
    state::AppState,
};

#[derive(Serialize)]
// État de l'accès réseau : valeurs configurées (prochain démarrage) et valeurs actives (ce démarrage).
pub struct NetworkInfo {
    pub lan: bool,
    pub port: u16,
    pub active_lan: bool,
    pub active_port: u16,
    pub hostname: String,
    pub ips: Vec<String>,
}

// Nom du PC, à communiquer aux autres postes (http://NOM:port).
fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

/// Adresses IP locales (sans envoyer de paquet : simple sélection de route).
fn local_ips() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0")
        && sock.connect("192.0.2.1:9").is_ok()
        && let Ok(a) = sock.local_addr()
    {
        out.push(a.ip().to_string());
    }
    out
}

// Lecture de la configuration réseau (ADMIN).
pub async fn get_network(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<NetworkInfo>> {
    user.require(Permission::ManageSettings)?;
    let cfg = config::load();
    Ok(Json(NetworkInfo {
        lan: cfg.lan.unwrap_or(false),
        port: cfg.port.unwrap_or(8080),
        active_lan: st.lan,
        active_port: st.port,
        hostname: hostname(),
        ips: local_ips(),
    }))
}

#[derive(Deserialize)]
// Réglage demandé par l'interface.
pub struct NetworkInput {
    pub lan: bool,
    pub port: u16,
}

// Enregistre lan/port dans erp.config.json ; prend effet au prochain lancement (restart_required).
pub async fn update_network(State(st): State<AppState>, user: CurrentUser, Json(i): Json<NetworkInput>) -> AppResult<Json<serde_json::Value>> {
    user.require(Permission::ManageSettings)?;
    if i.port < 1024 {
        return Err(AppError::Validation("Choisissez un port entre 1024 et 65535 (8080 par défaut)".into()));
    }
    let mut cfg = config::load();
    cfg.lan = Some(i.lan);
    cfg.port = Some(i.port);
    config::save(&cfg).map_err(|e| AppError::Internal(format!("écriture de {} : {e}", config::config_path().display())))?;
    let mut conn = st.pool.acquire().await?;
    audit::record(&mut conn, Some(user.id), "MODIFICATION", "network", None, Some(json!({ "lan": i.lan, "port": i.port }))).await?;
    Ok(Json(json!({ "ok": true, "restart_required": i.lan != st.lan || i.port != st.port })))
}

#[derive(Serialize)]
// État du stockage affiché dans Paramètres > Stockage.
pub struct StorageInfo {
    pub current_dir: String,
    pub default_dir: String,
    pub config_path: String,
    pub configured_dir: Option<String>,
    pub shared: bool,
    pub db_size: u64,
    pub documents_count: usize,
    // Vrai si la configuration pointe ailleurs que le dossier réellement ouvert.
    pub restart_required: bool,
}

// Nombre de fichiers (récursif) d'un dossier.
fn count_files(p: &Path) -> usize {
    walk(p).len()
}

// Liste récursive des fichiers d'un dossier.
fn walk(p: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() { out.extend(walk(&path)); } else { out.push(path); }
        }
    }
    out
}

// Lecture de l'état du stockage.
pub async fn get(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<StorageInfo>> {
    user.require(Permission::ManageSettings)?;
    let cfg = config::load();
    let configured = config::resolve_data_dir(None, &cfg);
    Ok(Json(StorageInfo {
        current_dir: st.paths.root.to_string_lossy().into(),
        default_dir: config::default_data_dir().to_string_lossy().into(),
        config_path: config::config_path().to_string_lossy().into(),
        configured_dir: cfg.data_dir.clone(),
        shared: cfg.shared || st.shared_storage,
        db_size: std::fs::metadata(&st.paths.db_file).map(|m| m.len()).unwrap_or(0),
        documents_count: count_files(&st.paths.documents),
        restart_required: configured != st.paths.root,
    }))
}

#[derive(Deserialize)]
// Changement d'emplacement demandé par l'interface.
pub struct StorageInput {
    /// Vide = revenir au dossier par défaut.
    pub data_dir: String,
    pub shared: Option<bool>,
    /// Copier les données actuelles dans le nouveau dossier s'il ne contient pas encore de base.
    pub copy_data: Option<bool>,
}

// Change le dossier de données : contrôle du chemin, test d'écriture, copie facultative, enregistrement.
// La base ouverte n'est pas déplacée à chaud : l'ERP doit être relancé.
pub async fn update(State(st): State<AppState>, user: CurrentUser, Json(i): Json<StorageInput>) -> AppResult<Json<serde_json::Value>> {
    user.require(Permission::ManageSettings)?;
    let raw = i.data_dir.trim().trim_end_matches(['\\', '/']).to_string();
    let target = if raw.is_empty() { config::default_data_dir() } else { PathBuf::from(&raw) };
    if !raw.is_empty() && !target.is_absolute() {
        return Err(AppError::Validation("Indiquez un chemin complet (ex. D:\\Mercure\\data ou \\\\NAS\\partage\\erp)".into()));
    }
    std::fs::create_dir_all(&target).map_err(|e| AppError::Validation(format!("Dossier inaccessible ({}) : {e}", target.display())))?;
    // Test d'écriture.
    let probe = target.join(".erp-test");
    std::fs::write(&probe, b"ok").map_err(|e| AppError::Validation(format!("Impossible d'écrire dans ce dossier : {e}")))?;
    let _ = std::fs::remove_file(&probe);

    // Même dossier qu'actuellement (chemins comparés après résolution).
    let same = target == st.paths.root || target.canonicalize().ok() == st.paths.root.canonicalize().ok();
    let target_db = target.join("erp.db");
    let mut copied = false;
    if !same && i.copy_data.unwrap_or(false) {
        if target_db.exists() {
            return Err(AppError::Conflict("Le dossier choisi contient déjà une base erp.db : choisissez « utiliser les données existantes » ou videz le dossier".into()));
        }
        // Copie cohérente de la base, puis des documents et sauvegardes.
        sqlx::query("VACUUM INTO ?").bind(target_db.to_string_lossy().to_string()).execute(&st.pool).await?;
        for sub in ["documents", "backups"] {
            let from = st.paths.root.join(sub);
            for f in walk(&from) {
                let rel = f.strip_prefix(&from).unwrap_or(&f);
                let to = target.join(sub).join(rel);
                if let Some(d) = to.parent() { std::fs::create_dir_all(d).map_err(AppError::internal)?; }
                std::fs::copy(&f, &to).map_err(AppError::internal)?;
            }
        }
        copied = true;
    }
    // Mode partagé explicite, sinon déduit du chemin (UNC, OneDrive...).
    let shared = i.shared.unwrap_or_else(|| config::looks_shared(&target));
    let mut cfg = config::load();
    cfg.data_dir = if raw.is_empty() { None } else { Some(target.to_string_lossy().into()) };
    cfg.shared = shared;
    config::save(&cfg).map_err(|e| AppError::Internal(format!("écriture de {} : {e}", config::config_path().display())))?;
    let mut conn = st.pool.acquire().await?;
    audit::record(&mut conn, Some(user.id), "MODIFICATION", "storage", None,
        Some(json!({ "data_dir": cfg.data_dir, "shared": shared, "copied": copied }))).await?;
    Ok(Json(json!({
        "ok": true,
        "data_dir": target.to_string_lossy(),
        "shared": shared,
        "copied": copied,
        "existing_db": target_db.exists() && !copied,
        "restart_required": !same,
    })))
}
