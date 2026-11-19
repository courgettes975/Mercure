//! Liste de référence des centres de formation (Senza, One Learn, BT Formation...).
//! Un centre est un contact de type ENTREPRISE ou ORGANISME marqué `is_training_center`.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::CurrentUser,
    domain::{roles::Permission, validation::normalize_spaces},
    error::{AppError, AppResult},
    modules::{audit, contacts},
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
// Un centre tel qu'affiché dans la liste, avec le nombre de missions qui le référencent.
pub struct TrainingCenter {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub city: String,
    pub missions_count: i64,
}

// Centres actifs (non archivés), triés par nom.
const SELECT: &str = "SELECT p.id, p.name, p.kind, p.city, (SELECT COUNT(*) FROM missions m WHERE m.center_id = p.id) AS missions_count
    FROM partners p WHERE p.is_training_center = 1 AND p.archived_at IS NULL ORDER BY p.name COLLATE NOCASE";

// Liste lisible par tous les rôles (alimente le menu déroulant des missions).
pub async fn list(State(st): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<TrainingCenter>>> {
    Ok(Json(sqlx::query_as::<_, TrainingCenter>(SELECT).fetch_all(&st.pool).await?))
}

#[derive(Deserialize, Default)]
#[serde(default)]
// Deux façons d'ajouter : un contact existant OU un nouveau nom d'organisme.
pub struct CenterInput {
    /// Contact existant (société ou organisme) à marquer comme centre.
    pub partner_id: Option<i64>,
    /// Sinon : nom d'un nouvel organisme à créer dans les contacts.
    pub name: String,
}

// Ajout à la liste (création du contact si besoin), avec contrôle du type et du doublon.
pub async fn create(State(st): State<AppState>, user: CurrentUser, Json(req): Json<CenterInput>) -> AppResult<Json<TrainingCenter>> {
    user.require(Permission::ManageReferenceLists)?;
    let partner_id = match req.partner_id {
        Some(id) => id,
        None => {
            let name = normalize_spaces(&req.name);
            if name.is_empty() {
                return Err(AppError::Validation("Choisissez un contact existant ou saisissez un nom".into()));
            }
            let input = contacts::model::ContactInput { kind: "ORGANISME".into(), name, is_client: true, ..Default::default() };
            contacts::service::create(&st.pool, user.id, input).await?.record.id
        }
    };
    let mut tx = st.pool.begin().await?;
    // Un particulier ne peut pas être un centre.
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM partners WHERE id = ? AND archived_at IS NULL").bind(partner_id).fetch_optional(&mut *tx).await?;
    match kind.as_deref() {
        None => return Err(AppError::NotFound("Contact introuvable".into())),
        Some("PARTICULIER") => return Err(AppError::Validation("Un centre de formation doit être une société ou un organisme".into())),
        _ => {}
    }
    let changed = sqlx::query("UPDATE partners SET is_training_center = 1 WHERE id = ? AND is_training_center = 0").bind(partner_id).execute(&mut *tx).await?;
    if changed.rows_affected() == 0 {
        return Err(AppError::Conflict("Ce contact est déjà dans la liste des centres".into()));
    }
    audit::record(&mut tx, Some(user.id), "CREATION", "training_center", Some(partner_id.to_string()), Some(json!({ "partner_id": partner_id }))).await?;
    tx.commit().await?;
    let row = sqlx::query_as::<_, TrainingCenter>(SELECT).fetch_all(&st.pool).await?.into_iter().find(|c| c.id == partner_id)
        .ok_or_else(|| AppError::Internal("centre non relu".into()))?;
    Ok(Json(row))
}

/// Retire le contact de la liste (le contact et les missions qui le référencent sont conservés).
pub async fn remove(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<serde_json::Value>> {
    user.require(Permission::ManageReferenceLists)?;
    let mut tx = st.pool.begin().await?;
    let changed = sqlx::query("UPDATE partners SET is_training_center = 0 WHERE id = ? AND is_training_center = 1").bind(id).execute(&mut *tx).await?;
    if changed.rows_affected() == 0 {
        return Err(AppError::NotFound("Centre introuvable".into()));
    }
    audit::record(&mut tx, Some(user.id), "SUPPRESSION", "training_center", Some(id.to_string()), None).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
