//! Sauvegardes : copie cohérente de la base (VACUUM INTO), manuelle ou automatique quotidienne.

use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    Json,
};
use chrono::Local;
use serde::Serialize;
use serde_json::json;
use std::path::Path as FsPath;

use crate::{
    auth::CurrentUser,
    domain::roles::Permission,
    error::{AppError, AppResult},
    modules::audit,
    state::AppState,
};

/// Nombre de sauvegardes automatiques conservées.
pub const AUTO_KEEP: usize = 30;

#[derive(Serialize, Debug)]
// Un fichier de sauvegarde listé : `kind` = "auto" ou "manuel" selon le préfixe du nom.
pub struct BackupFile {
    pub name: String,
    pub size: u64,
    pub kind: &'static str,
}

// Noms acceptés : auto-AAAAMMJJ-HHMMSS.db ou manuel-... ; interdit toute traversée de dossier (../).
fn is_safe_name(name: &str) -> bool {
    name.ends_with(".db")
        && (name.starts_with("auto-") || name.starts_with("manuel-"))
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !name.contains("..")
}

/// Crée une sauvegarde et renvoie le nom du fichier.
pub async fn create_backup(st: &AppState, kind: &str) -> AppResult<String> {
    let name = format!("{kind}-{}.db", Local::now().format("%Y%m%d-%H%M%S"));
    let target = st.paths.backups.join(&name);
    if target.exists() {
        return Err(AppError::Conflict("Une sauvegarde vient déjà d'être faite, réessayez dans une seconde".into()));
    }
    // VACUUM INTO écrit une copie compacte et cohérente, même pendant que l'ERP est utilisé.
    let target_str = target.to_string_lossy().to_string();
    sqlx::query("VACUUM INTO ?").bind(target_str).execute(&st.pool).await?;
    Ok(name)
}

// Liste les sauvegardes du dossier, les plus récentes d'abord (tri sur l'horodatage du nom).
pub fn list_files(dir: &FsPath) -> std::io::Result<Vec<BackupFile>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_safe_name(&name) {
            continue;
        }
        let kind = if name.starts_with("auto-") { "auto" } else { "manuel" };
        out.push(BackupFile { size: entry.metadata()?.len(), name, kind });
    }
    out.sort_by(|a, b| b.name[b.name.find('-').unwrap()..].cmp(&a.name[a.name.find('-').unwrap()..]));
    Ok(out)
}

/// Supprime les sauvegardes automatiques au-delà de AUTO_KEEP.
pub fn rotate(dir: &FsPath) -> std::io::Result<()> {
    let mut autos: Vec<_> = list_files(dir)?.into_iter().filter(|f| f.kind == "auto").collect();
    autos.sort_by(|a, b| b.name.cmp(&a.name));
    for f in autos.into_iter().skip(AUTO_KEEP) {
        std::fs::remove_file(dir.join(&f.name))?;
    }
    Ok(())
}

/// Sauvegarde automatique si aucune n'a été faite aujourd'hui.
// Appelée au démarrage puis toutes les heures par la tâche de fond.
pub async fn daily_if_needed(st: &AppState) -> AppResult<Option<String>> {
    let today = format!("auto-{}", Local::now().format("%Y%m%d"));
    let files = list_files(&st.paths.backups).map_err(AppError::internal)?;
    if files.iter().any(|f| f.name.starts_with(&today)) {
        return Ok(None);
    }
    let name = create_backup(st, "auto").await?;
    rotate(&st.paths.backups).map_err(AppError::internal)?;
    let mut conn = st.pool.acquire().await?;
    audit::record(&mut conn, None, "SAUVEGARDE_AUTO", "backup", Some(name.clone()), None).await?;
    Ok(Some(name))
}

/// Tâche de fond : vérifie toutes les heures.
pub fn spawn_daily_task(st: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            match daily_if_needed(&st).await {
                Ok(Some(name)) => tracing::info!("sauvegarde automatique : {name}"),
                Ok(None) => {}
                Err(e) => tracing::error!("échec de la sauvegarde automatique : {e:?}"),
            }
        }
    });
}

// Liste pour l'interface, avec le dossier pour que l'utilisateur sache où copier les fichiers.
pub async fn list(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    user.require(Permission::RunBackup)?;
    let files = list_files(&st.paths.backups).map_err(AppError::internal)?;
    Ok(Json(json!({ "folder": st.paths.backups.to_string_lossy(), "files": files })))
}

// Sauvegarde manuelle à la demande.
pub async fn create(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    user.require(Permission::RunBackup)?;
    let name = create_backup(&st, "manuel").await?;
    let mut conn = st.pool.acquire().await?;
    audit::record(&mut conn, Some(user.id), "SAUVEGARDE", "backup", Some(name.clone()), None).await?;
    Ok(Json(json!({ "name": name })))
}

// Téléchargement d'une sauvegarde (nom validé pour rester dans le dossier backups).
pub async fn download(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::RunBackup)?;
    if !is_safe_name(&name) {
        return Err(AppError::NotFound("Sauvegarde introuvable".into()));
    }
    let bytes = tokio::fs::read(st.paths.backups.join(&name))
        .await
        .map_err(|_| AppError::NotFound("Sauvegarde introuvable".into()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_autorises() {
        assert!(is_safe_name("auto-20261118-101500.db"));
        assert!(is_safe_name("manuel-20261118-101500.db"));
        assert!(!is_safe_name("../erp.db"));
        assert!(!is_safe_name("auto-..\\erp.db"));
        assert!(!is_safe_name("erp.db"));
    }
}
