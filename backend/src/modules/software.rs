//! Liste de référence des logiciels enseignés (AutoCAD, Revit, Fusion, SolidWorks...).

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
    modules::audit,
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
// Un logiciel : `active` = proposé dans les nouvelles missions (archivé sinon).
pub struct Software {
    pub id: i64,
    pub name: String,
    pub active: bool,
}

// Liste complète (actifs et archivés) ; l'interface filtre selon le contexte.
pub async fn list(State(st): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<Software>>> {
    let rows = sqlx::query_as::<_, Software>("SELECT id, name, active FROM software ORDER BY name COLLATE NOCASE")
        .fetch_all(&st.pool)
        .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
// Création (name) ou modification partielle (name et/ou active).
pub struct SoftwareInput {
    pub name: Option<String>,
    pub active: Option<bool>,
}

// Message clair quand le nom existe déjà (contrainte UNIQUE).
fn conflict_msg(e: sqlx::Error) -> AppError {
    match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("Ce logiciel existe déjà".into()),
        other => other,
    }
}

// Ajout d'un logiciel (GESTIONNAIRE ou ADMIN), journalisé.
pub async fn create(
    State(st): State<AppState>,
    user: CurrentUser,
    Json(req): Json<SoftwareInput>,
) -> AppResult<Json<Software>> {
    user.require(Permission::ManageReferenceLists)?;
    let name = normalize_spaces(req.name.as_deref().unwrap_or(""));
    if name.is_empty() {
        return Err(AppError::Validation("Le nom est obligatoire".into()));
    }
    let mut tx = st.pool.begin().await?;
    let row = sqlx::query_as::<_, Software>("INSERT INTO software (name) VALUES (?) RETURNING id, name, active")
        .bind(&name)
        .fetch_one(&mut *tx)
        .await
        .map_err(conflict_msg)?;
    audit::record(&mut tx, Some(user.id), "CREATION", "software", Some(row.id.to_string()), Some(json!({ "name": name }))).await?;
    tx.commit().await?;
    Ok(Json(row))
}

// Renommage et/ou archivage ; le journal garde la différence avant/après.
pub async fn update(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<SoftwareInput>,
) -> AppResult<Json<Software>> {
    user.require(Permission::ManageReferenceLists)?;
    let mut tx = st.pool.begin().await?;
    let before = sqlx::query_as::<_, Software>("SELECT id, name, active FROM software WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Logiciel introuvable".into()))?;
    let name = match req.name {
        Some(n) => {
            let n = normalize_spaces(&n);
            if n.is_empty() {
                return Err(AppError::Validation("Le nom est obligatoire".into()));
            }
            n
        }
        None => before.name.clone(),
    };
    let active = req.active.unwrap_or(before.active);
    let after = sqlx::query_as::<_, Software>(
        "UPDATE software SET name = ?, active = ? WHERE id = ? RETURNING id, name, active",
    )
    .bind(&name)
    .bind(active)
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(conflict_msg)?;
    let changes = audit::diff(&json!({ "name": before.name, "active": before.active }), &json!({ "name": after.name, "active": after.active }));
    audit::record(&mut tx, Some(user.id), "MODIFICATION", "software", Some(id.to_string()), Some(changes)).await?;
    tx.commit().await?;
    Ok(Json(after))
}
