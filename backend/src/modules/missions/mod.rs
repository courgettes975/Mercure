//! Lot 3b : missions de formation (routes HTTP).

// structures échangées avec l'interface
pub mod model;
// règles métier et SQL
pub mod service;

use axum::{
    extract::{Multipart, Path, Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    auth::CurrentUser,
    domain::roles::Permission,
    error::{AppError, AppResult},
    modules::audit::{self, Page},
    state::AppState,
};
use model::*;

// GET /missions : liste paginée (vue liste / kanban).
pub async fn list(State(st): State<AppState>, _u: CurrentUser, Query(f): Query<MissionFilters>) -> AppResult<Json<Page<MissionListRow>>> {
    let per_page = f.per_page.unwrap_or(80).clamp(1, 1000);
    let page = f.page.unwrap_or(1).max(1);
    let (items, total) = service::list(&st.pool, &f).await?;
    Ok(Json(Page { items, total, page, per_page }))
}

// GET /missions/dashboard : compteurs par statut et montants.
pub async fn dashboard(State(st): State<AppState>, _u: CurrentUser, Query(f): Query<MissionFilters>) -> AppResult<Json<service::Dashboard>> {
    Ok(Json(service::dashboard(&st.pool, &f).await?))
}

#[derive(Deserialize)]
// Bornes de la vue calendrier.
pub struct Period {
    pub from: String,
    pub to: String,
}

// GET /missions/calendar : sessions de la période.
pub async fn calendar(State(st): State<AppState>, _u: CurrentUser, Query(p): Query<Period>) -> AppResult<Json<Vec<service::CalendarSession>>> {
    Ok(Json(service::calendar(&st.pool, &p.from, &p.to).await?))
}

// GET /missions/{id} : fiche complète.
pub async fn get(State(st): State<AppState>, _u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<MissionDetail>> {
    let mut conn = st.pool.acquire().await?;
    Ok(Json(service::load(&mut conn, id).await?))
}

// POST /missions : création (projet FORMATION + mission, référence MF-AAAA-NNNN).
pub async fn create(State(st): State<AppState>, u: CurrentUser, Json(i): Json<MissionInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::create(&st.pool, u.id, i).await?))
}

// PUT /missions/{id}
pub async fn update(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<MissionInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::update(&st.pool, u.id, id, i).await?))
}

#[derive(Deserialize)]
// Statut cible (mission ou session).
pub struct StatusInput {
    pub status: String,
}

// POST /missions/{id}/status : changement manuel de statut (transitions contrôlées).
pub async fn set_status(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(s): Json<StatusInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::set_status(&st.pool, u.id, id, &s.status).await?))
}

// DELETE /missions/{id} : supprime la mission puis ses fichiers joints sur le disque.
pub async fn delete(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    u.require(Permission::ManageBusinessData)?;
    let files: Vec<String> = sqlx::query_scalar("SELECT storage_name FROM documents WHERE owner_type = 'MISSION' AND owner_id = ?")
        .bind(id)
        .fetch_all(&st.pool)
        .await?;
    service::delete(&st.pool, u.id, id).await?;
    for f in files {
        let _ = tokio::fs::remove_file(st.paths.documents.join(f)).await;
    }
    Ok(Json(json!({ "ok": true })))
}

// POST /missions/{id}/sessions
pub async fn add_session(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<SessionInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::add_session(&st.pool, u.id, id, i).await?))
}

// PUT /sessions/{id}
pub async fn update_session(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<SessionInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::update_session(&st.pool, u.id, id, i).await?))
}

// DELETE /sessions/{id}
pub async fn delete_session(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::delete_session(&st.pool, u.id, id).await?))
}

// POST /sessions/{id}/realize : marque réalisée, génère le temps du formateur, note les absents.
pub async fn realize_session(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<service::RealizeInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::realize_session(&st.pool, u.id, id, i).await?))
}

// POST /sessions/{id}/status : PREVUE / REALISEE / ANNULEE.
pub async fn session_status(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(s): Json<StatusInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::set_session_status(&st.pool, u.id, id, &s.status).await?))
}

#[derive(Deserialize)]
// Contact à inscrire comme stagiaire.
pub struct AddParticipant {
    pub partner_id: i64,
}

// POST /missions/{id}/participants
pub async fn add_participant(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(p): Json<AddParticipant>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::add_participant(&st.pool, u.id, id, p.partner_id).await?))
}

// PUT /participants/{id} : suivi pédagogique (progression, évaluation, commentaire).
pub async fn update_participant(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(p): Json<service::ParticipantInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::update_participant(&st.pool, u.id, id, p).await?))
}

// DELETE /participants/{id}
pub async fn remove_participant(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::remove_participant(&st.pool, u.id, id).await?))
}

// POST /missions/{id}/expenses
pub async fn add_expense(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<ExpenseInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::add_expense(&st.pool, u.id, id, i).await?))
}

// PUT /expenses/{id}
pub async fn update_expense(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<ExpenseInput>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::update_expense(&st.pool, u.id, id, i).await?))
}

// DELETE /expenses/{id}
pub async fn delete_expense(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::delete_expense(&st.pool, u.id, id).await?))
}

// ---------- Documents ----------

// Taille maximale d'une pièce jointe (20 Mo).
const MAX_DOC_BYTES: usize = 20 * 1024 * 1024;

// Nom de fichier affiché : sans chemin, caractères dangereux remplacés, longueur bornée.
fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("document");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')') { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "document".into() } else { cleaned.chars().take(120).collect() }
}

// POST /missions/{id}/documents : fichier stocké sous un nom aléatoire dans documents/missions/<id>/,
// empreinte SHA-256 conservée ; le fichier est effacé si l'écriture en base échoue.
pub async fn upload_document(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, mut mp: Multipart) -> AppResult<Json<MissionDetail>> {
    u.require(Permission::ManageBusinessData)?;
    let (mut category, mut file): (String, Option<(String, Vec<u8>)>) = ("AUTRE".into(), None);
    while let Some(field) = mp.next_field().await.map_err(|_| AppError::Validation("Envoi du fichier invalide (20 Mo max)".into()))? {
        match field.name() {
            Some("category") => category = field.text().await.unwrap_or_default(),
            Some("file") => {
                let name = field.file_name().unwrap_or("document").to_string();
                let data = field.bytes().await.map_err(|_| AppError::Validation("Fichier trop volumineux (20 Mo max)".into()))?;
                file = Some((name, data.to_vec()));
            }
            _ => {}
        }
    }
    if !DOC_CATEGORIES.contains(&category.as_str()) {
        return Err(AppError::Validation("Catégorie de document inconnue".into()));
    }
    let (name, data) = file.ok_or_else(|| AppError::Validation("Aucun fichier reçu".into()))?;
    if data.is_empty() || data.len() > MAX_DOC_BYTES {
        return Err(AppError::Validation("Fichier vide ou trop volumineux (20 Mo max)".into()));
    }
    let file_name = safe_file_name(&name);
    let mut rnd = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut rnd);
    let storage_name = format!("missions/{id}/{}_{}", hex::encode(rnd), file_name.replace(' ', "_"));
    let mime = mime_guess::from_path(&file_name).first_or_octet_stream().to_string();
    let sha = hex::encode(Sha256::digest(&data));

    let mut tx = st.pool.begin().await?;
    service::record(&mut tx, id).await?;
    let path = st.paths.documents.join(&storage_name);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(AppError::internal)?;
    }
    tokio::fs::write(&path, &data).await.map_err(AppError::internal)?;
    sqlx::query(
        "INSERT INTO documents (owner_type, owner_id, category, file_name, storage_name, mime_type, size, sha256, uploaded_by)
         VALUES ('MISSION', ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id).bind(&category).bind(&file_name).bind(&storage_name).bind(&mime).bind(data.len() as i64).bind(&sha).bind(u.id)
    .execute(&mut *tx)
    .await?;
    audit::record(&mut tx, Some(u.id), "AJOUT_DOCUMENT", service::ENTITY, Some(id.to_string()), Some(json!({ "file": file_name, "category": category }))).await?;
    let d = service::load(&mut tx, id).await?;
    if let Err(e) = tx.commit().await {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(e.into());
    }
    Ok(Json(d))
}

// GET /documents/{id} : renvoie le fichier avec son nom d'origine (version ASCII pour l'en-tête HTTP).
pub async fn download_document(State(st): State<AppState>, _u: CurrentUser, Path(id): Path<i64>) -> AppResult<impl IntoResponse> {
    let row: Option<(String, String, String)> = sqlx::query_as("SELECT file_name, storage_name, mime_type FROM documents WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await?;
    let (name, storage, mime) = row.ok_or_else(|| AppError::NotFound("Document introuvable".into()))?;
    let bytes = tokio::fs::read(st.paths.documents.join(&storage)).await.map_err(|_| AppError::NotFound("Fichier introuvable sur le disque".into()))?;
    let ascii: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{ascii}\"")),
        ],
        bytes,
    ))
}

// DELETE /documents/{id} : enregistrement supprimé puis fichier effacé.
pub async fn delete_document(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    u.require(Permission::ManageBusinessData)?;
    let mut tx = st.pool.begin().await?;
    let row: Option<(i64, String, String)> = sqlx::query_as("SELECT owner_id, file_name, storage_name FROM documents WHERE id = ? AND owner_type = 'MISSION'")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let (mission_id, name, storage) = row.ok_or_else(|| AppError::NotFound("Document introuvable".into()))?;
    sqlx::query("DELETE FROM documents WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(u.id), "SUPPRESSION_DOCUMENT", service::ENTITY, Some(mission_id.to_string()), Some(json!({ "file": name }))).await?;
    tx.commit().await?;
    let _ = tokio::fs::remove_file(st.paths.documents.join(storage)).await;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::safe_file_name;

    #[test]
    fn noms_de_fichiers() {
        assert_eq!(safe_file_name("..\\..\\windows\\evil.exe"), "evil.exe");
        assert_eq!(safe_file_name("../.ssh"), "ssh");
        assert_eq!(safe_file_name("Convention signée.pdf"), "Convention signée.pdf");
        assert_eq!(safe_file_name("a<b>.pdf"), "a_b_.pdf");
    }
}
