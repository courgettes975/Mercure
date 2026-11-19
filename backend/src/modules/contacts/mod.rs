//! Module Contacts : routes HTTP. Les règles sont dans `service`.

// import Odoo (XLSX / CSV)
pub mod import;
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
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::CurrentUser,
    domain::{roles::Permission, validation::normalize_spaces},
    error::{AppError, AppResult},
    modules::audit::{self, Page},
    state::AppState,
};
use model::*;

// GET /contacts : liste paginée et filtrée (lecture ouverte à tous les rôles).
pub async fn list(
    State(st): State<AppState>,
    _user: CurrentUser,
    Query(f): Query<ContactFilters>,
) -> AppResult<Json<Page<ContactListItem>>> {
    let per_page = f.per_page.unwrap_or(80).clamp(1, 500);
    let page = f.page.unwrap_or(1).max(1);
    let (items, total) = service::list(&st.pool, &f, per_page, (page - 1) * per_page).await?;
    Ok(Json(Page { items, total, page, per_page }))
}

// GET /contacts/export : mêmes filtres que la liste, sortie CSV pour Excel.
pub async fn export(
    State(st): State<AppState>,
    user: CurrentUser,
    Query(f): Query<ContactFilters>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::ManageBusinessData)?;
    let csv = service::export_csv(&st.pool, &f).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"contacts.csv\""),
        ],
        csv,
    ))
}

// GET /contacts/{id}
pub async fn get(State(st): State<AppState>, _user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<ContactDetail>> {
    let mut conn = st.pool.acquire().await?;
    Ok(Json(service::load_detail(&mut conn, id).await?))
}

// POST /contacts
pub async fn create(
    State(st): State<AppState>,
    user: CurrentUser,
    Json(input): Json<ContactInput>,
) -> AppResult<Json<ContactDetail>> {
    user.require(Permission::ManageBusinessData)?;
    Ok(Json(service::create(&st.pool, user.id, input).await?))
}

// PUT /contacts/{id}
pub async fn update(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<ContactInput>,
) -> AppResult<Json<ContactDetail>> {
    user.require(Permission::ManageBusinessData)?;
    Ok(Json(service::update(&st.pool, user.id, id, input).await?))
}

// POST /contacts/{id}/archive : le contact sort des listes mais reste référencé par l'historique.
pub async fn archive(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<ContactDetail>> {
    user.require(Permission::ManageBusinessData)?;
    Ok(Json(service::set_archived(&st.pool, user.id, id, true).await?))
}

// POST /contacts/{id}/unarchive
pub async fn unarchive(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<ContactDetail>> {
    user.require(Permission::ManageBusinessData)?;
    Ok(Json(service::set_archived(&st.pool, user.id, id, false).await?))
}

// DELETE /contacts/{id} : seulement si rien ne le référence (sinon archiver).
pub async fn delete(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    user.require(Permission::ManageBusinessData)?;
    service::delete(&st.pool, user.id, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// POST /contacts/{id}/duplicate : copie "(copie)" pour créer un contact proche.
pub async fn duplicate(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<ContactDetail>> {
    user.require(Permission::ManageBusinessData)?;
    Ok(Json(service::duplicate(&st.pool, user.id, id).await?))
}

// GET /contacts/{id}/activity : notes et historique d'audit du contact.
pub async fn activity(State(st): State<AppState>, _user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let (notes, history) = service::activity(&st.pool, id).await?;
    Ok(Json(json!({ "notes": notes, "history": history })))
}

#[derive(Deserialize)]
// Corps d'une note libre.
pub struct NoteInput {
    pub body: String,
}

// POST /contacts/{id}/notes
pub async fn add_note(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(n): Json<NoteInput>,
) -> AppResult<Json<Note>> {
    user.require(Permission::ManageBusinessData)?;
    Ok(Json(service::add_note(&st.pool, user.id, id, &n.body).await?))
}

// GET /tags : étiquettes disponibles.
pub async fn list_tags(State(st): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<Tag>>> {
    Ok(Json(
        sqlx::query_as::<_, Tag>("SELECT id, name FROM tags ORDER BY name COLLATE NOCASE").fetch_all(&st.pool).await?,
    ))
}

#[derive(Deserialize)]
// Nom d'une nouvelle étiquette.
pub struct TagInput {
    pub name: String,
}

// POST /tags : nom unique.
pub async fn create_tag(State(st): State<AppState>, user: CurrentUser, Json(t): Json<TagInput>) -> AppResult<Json<Tag>> {
    user.require(Permission::ManageBusinessData)?;
    let name = normalize_spaces(&t.name);
    if name.is_empty() {
        return Err(AppError::Validation("Le nom de l'étiquette est obligatoire".into()));
    }
    let mut tx = st.pool.begin().await?;
    let tag = sqlx::query_as::<_, Tag>("INSERT INTO tags (name) VALUES (?) RETURNING id, name")
        .bind(&name)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match AppError::from(e) {
            AppError::Conflict(_) => AppError::Conflict("Cette étiquette existe déjà".into()),
            other => other,
        })?;
    audit::record(&mut tx, Some(user.id), "CREATION", "tag", Some(tag.id.to_string()), Some(json!({ "name": name }))).await?;
    tx.commit().await?;
    Ok(Json(tag))
}

// Lit le champ "file" d'un envoi multipart (nom + octets).
async fn read_upload(mut multipart: Multipart) -> AppResult<(String, Vec<u8>)> {
    while let Some(field) = multipart.next_field().await.map_err(|_| AppError::Validation("Envoi du fichier invalide".into()))? {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("import.xlsx").to_string();
            let data = field.bytes().await.map_err(|_| AppError::Validation("Fichier trop volumineux (5 Mo max)".into()))?;
            return Ok((name, data.to_vec()));
        }
    }
    Err(AppError::Validation("Aucun fichier reçu".into()))
}

// Fichier -> lignes -> plan d'import -> détection des contacts déjà importés.
async fn build_plan(st: &AppState, multipart: Multipart) -> AppResult<Vec<import::PlannedContact>> {
    let (name, data) = read_upload(multipart).await?;
    let rows = import::read_rows(&data, &name)?;
    let mut items = import::plan(&rows);
    import::mark_existing(&st.pool, &mut items).await?;
    Ok(items)
}

/// Aperçu de l'import : rien n'est enregistré.
pub async fn import_preview(State(st): State<AppState>, user: CurrentUser, multipart: Multipart) -> AppResult<Json<import::ImportPlan>> {
    user.require(Permission::ManageBusinessData)?;
    Ok(Json(import::summarize(build_plan(&st, multipart).await?)))
}

// Exécution de l'import : le plan est recalculé côté serveur à partir du même fichier (pas de confiance au navigateur).
pub async fn import_execute(State(st): State<AppState>, user: CurrentUser, multipart: Multipart) -> AppResult<Json<import::ImportResult>> {
    user.require(Permission::ManageBusinessData)?;
    let items = build_plan(&st, multipart).await?;
    Ok(Json(import::execute(&st.pool, user.id, items).await?))
}
