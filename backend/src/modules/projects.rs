//! Lot 3 : projets, tâches et intervenants.
//! Une mission de formation est un projet de type FORMATION (voir module missions).

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::{
    auth::CurrentUser,
    domain::{
        roles::Permission,
        validation::{normalize_spaces, parse_date, search_key},
    },
    error::{AppError, AppResult},
    modules::{audit::{self, Page}, sequence},
    state::AppState,
};

// Type d'entité utilisé dans le journal d'audit.
pub const ENTITY: &str = "project";
// Statuts possibles d'un projet.
pub const STATUSES: [&str; 4] = ["BROUILLON", "EN_COURS", "TERMINE", "ANNULE"];

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
// Données saisies pour créer ou modifier un projet (les missions passent par ce même type).
pub struct ProjectInput {
    pub name: String,
    pub client_id: i64,
    pub description: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub status: String,
    pub billable: bool,
    // Taux horaire en centimes, appliqué aux temps saisis sur le projet.
    pub hourly_rate: i64,
    // Intervenants autorisés à saisir du temps.
    pub member_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Projet tel que listé : colonnes de la table + totaux de temps calculés en SQL.
pub struct ProjectRow {
    pub id: i64,
    pub reference: String,
    pub kind: String,
    pub name: String,
    pub client_id: i64,
    pub client_name: String,
    pub description: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub status: String,
    pub billable: bool,
    pub hourly_rate: i64,
    pub archived_at: Option<String>,
    // Renseigné si le projet est une mission de formation.
    pub mission_id: Option<i64>,
    // Temps saisis, facturables, et montant facturable pas encore facturé.
    pub minutes_total: i64,
    pub minutes_billable: i64,
    pub amount_uninvoiced: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Intervenant (utilisateur) d'un projet.
pub struct Member {
    pub id: i64,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Tâche d'un projet ; `active` = encore proposée à la saisie de temps.
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Serialize)]
// Fiche complète : projet + intervenants + tâches.
pub struct ProjectDetail {
    #[serde(flatten)]
    pub row: ProjectRow,
    pub members: Vec<Member>,
    pub tasks: Vec<Task>,
}

// Requête de lecture commune (liste et fiche), complétée par une clause WHERE.
macro_rules! select_project {
    ($tail:literal) => {
        concat!(
            "SELECT p.id, p.reference, p.kind, p.name, p.client_id,
                TRIM(c.name || ' ' || c.first_name) AS client_name, p.description, p.start_date, p.end_date,
                p.status, p.billable, p.hourly_rate, p.archived_at, m.id AS mission_id,
                COALESCE((SELECT SUM(t.duration_minutes) FROM timesheets t WHERE t.project_id = p.id), 0) AS minutes_total,
                COALESCE((SELECT SUM(t.duration_minutes) FROM timesheets t WHERE t.project_id = p.id AND t.billable = 1), 0) AS minutes_billable,
                COALESCE((SELECT SUM(t.amount) FROM timesheets t WHERE t.project_id = p.id AND t.billable = 1 AND t.invoice_line_id IS NULL), 0) AS amount_uninvoiced,
                p.created_at
             FROM projects p JOIN partners c ON c.id = p.client_id LEFT JOIN missions m ON m.project_id = p.id ",
            $tail
        )
    };
}

// Nettoie et valide la saisie ; renvoie le premier défaut rencontré.
pub fn normalize(i: &mut ProjectInput) -> AppResult<()> {
    i.name = normalize_spaces(&i.name);
    i.description = i.description.trim().to_string();
    i.start_date = i.start_date.take().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    i.end_date = i.end_date.take().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    i.member_ids.sort_unstable();
    i.member_ids.dedup();
    if i.status.is_empty() {
        i.status = "EN_COURS".into();
    }
    let err = |m: &str| Err(AppError::Validation(m.into()));
    if i.name.is_empty() {
        return err("Le nom est obligatoire");
    }
    if i.client_id <= 0 {
        return err("Le client est obligatoire");
    }
    if !STATUSES.contains(&i.status.as_str()) {
        return err("Statut inconnu");
    }
    for d in [&i.start_date, &i.end_date].into_iter().flatten() {
        if parse_date(d).is_none() {
            return err("Date invalide (format AAAA-MM-JJ)");
        }
    }
    if let (Some(s), Some(e)) = (&i.start_date, &i.end_date)
        && e < s {
            return err("La date de fin est antérieure à la date de début");
        }
    if i.hourly_rate < 0 {
        return err("Le taux horaire doit être positif ou nul");
    }
    Ok(())
}

// Vérifie que le client et les intervenants existent (les ids viennent du navigateur).
async fn check_refs(conn: &mut SqliteConnection, i: &ProjectInput) -> AppResult<()> {
    let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM partners WHERE id = ?").bind(i.client_id).fetch_optional(&mut *conn).await?;
    if ok.is_none() {
        return Err(AppError::Validation("Client introuvable".into()));
    }
    for u in &i.member_ids {
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE id = ?").bind(u).fetch_optional(&mut *conn).await?;
        if ok.is_none() {
            return Err(AppError::Validation("Intervenant introuvable".into()));
        }
    }
    Ok(())
}

// Charge la fiche complète d'un projet.
pub async fn load(conn: &mut SqliteConnection, id: i64) -> AppResult<ProjectDetail> {
    let row = sqlx::query_as::<_, ProjectRow>(select_project!("WHERE p.id = ?"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Projet introuvable".into()))?;
    let members = sqlx::query_as::<_, Member>(
        "SELECT u.id, u.display_name FROM project_members pm JOIN users u ON u.id = pm.user_id WHERE pm.project_id = ? ORDER BY u.display_name",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let tasks = sqlx::query_as::<_, Task>("SELECT id, project_id, name, active FROM tasks WHERE project_id = ? ORDER BY name COLLATE NOCASE")
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(ProjectDetail { row, members, tasks })
}

// Vue simplifiée stockée dans le journal (noms lisibles plutôt que des ids).
fn audit_view(d: &ProjectDetail) -> Value {
    json!({
        "name": d.row.name, "client": d.row.client_name, "description": d.row.description,
        "start_date": d.row.start_date, "end_date": d.row.end_date, "status": d.row.status,
        "billable": d.row.billable, "hourly_rate": d.row.hourly_rate,
        "members": d.members.iter().map(|m| m.display_name.clone()).collect::<Vec<_>>(),
    })
}

// Écrit les champs modifiables et remplace la liste des intervenants.
async fn write(conn: &mut SqliteConnection, id: i64, i: &ProjectInput) -> AppResult<()> {
    sqlx::query(
        "UPDATE projects SET name=?, client_id=?, description=?, start_date=?, end_date=?, status=?, billable=?,
            hourly_rate=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
    )
    .bind(&i.name).bind(i.client_id).bind(&i.description).bind(&i.start_date).bind(&i.end_date)
    .bind(&i.status).bind(i.billable).bind(i.hourly_rate).bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM project_members WHERE project_id = ?").bind(id).execute(&mut *conn).await?;
    for u in &i.member_ids {
        sqlx::query("INSERT INTO project_members (project_id, user_id) VALUES (?, ?)").bind(id).bind(u).execute(&mut *conn).await?;
    }
    Ok(())
}

/// Création (dans une transaction BEGIN IMMEDIATE ouverte par l'appelant).
pub async fn insert(conn: &mut SqliteConnection, user_id: i64, kind: &str, i: &ProjectInput) -> AppResult<i64> {
    check_refs(conn, i).await?;
    let year = chrono::Local::now().format("%Y").to_string();
    let n = sequence::next_value(conn, &format!("PRJ-{year}")).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO projects (reference, kind, name, client_id, created_by) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(sequence::format_reference("PRJ", year.parse().unwrap_or(2026), n, 4))
    .bind(kind)
    .bind(&i.name)
    .bind(i.client_id)
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await?;
    write(conn, id, i).await?;
    let d = load(conn, id).await?;
    audit::record(conn, Some(user_id), "CREATION", ENTITY, Some(id.to_string()), Some(audit_view(&d))).await?;
    Ok(id)
}

// Mise à jour avec contrôle : le client ne change plus dès qu'une facture existe.
pub async fn update_in_tx(conn: &mut SqliteConnection, user_id: i64, id: i64, i: &ProjectInput) -> AppResult<()> {
    let before = load(conn, id).await?;
    check_refs(conn, i).await?;
    if before.row.client_id != i.client_id {
        let invoiced: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE project_id = ?").bind(id).fetch_one(&mut *conn).await?;
        if invoiced > 0 {
            return Err(AppError::Validation("Le client ne peut plus changer : des factures existent pour ce projet".into()));
        }
    }
    write(conn, id, i).await?;
    let after = load(conn, id).await?;
    let changes = audit::diff(&audit_view(&before), &audit_view(&after));
    if changes.as_object().is_some_and(|m| !m.is_empty()) {
        audit::record(conn, Some(user_id), "MODIFICATION", ENTITY, Some(id.to_string()), Some(changes)).await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
// Filtres de la liste (paramètres d'URL).
pub struct ProjectFilters {
    pub q: Option<String>,
    pub client_id: Option<i64>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub archived: Option<String>,
    // Uniquement les projets où l'utilisateur est intervenant.
    pub mine: Option<bool>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

// Traduit les filtres en clauses SQL paramétrées (QueryBuilder : pas de concaténation de valeurs).
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &ProjectFilters, user_id: i64) {
    match f.archived.as_deref() {
        Some("archives") => qb.push(" AND p.archived_at IS NOT NULL"),
        Some("tous") => qb.push(""),
        _ => qb.push(" AND p.archived_at IS NULL"),
    };
    if let Some(q) = f.q.as_deref() {
        for w in search_key(q).split(' ').filter(|w| !w.is_empty()) {
            // Chaque mot de la recherche doit apparaître dans le nom, la référence ou le client.
            let like = format!("%{}%", w.replace(['%', '_'], ""));
            qb.push(" AND (lower(p.name) LIKE ").push_bind(like.clone())
                .push(" OR lower(p.reference) LIKE ").push_bind(like.clone())
                .push(" OR lower(c.name) LIKE ").push_bind(like).push(")");
        }
    }
    if let Some(c) = f.client_id {
        qb.push(" AND p.client_id = ").push_bind(c);
    }
    if let Some(s) = f.status.as_deref().filter(|s| STATUSES.contains(s)) {
        qb.push(" AND p.status = ").push_bind(s.to_string());
    }
    if let Some(k) = f.kind.as_deref().filter(|k| ["PROJET", "FORMATION"].contains(k)) {
        qb.push(" AND p.kind = ").push_bind(k.to_string());
    }
    if f.mine == Some(true) {
        qb.push(" AND EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = p.id AND pm.user_id = ").push_bind(user_id).push(")");
    }
}

// Liste paginée : total puis page, les projets terminés en dernier.
pub async fn list(pool: &SqlitePool, f: &ProjectFilters, user_id: i64) -> AppResult<Page<ProjectRow>> {
    let per_page = f.per_page.unwrap_or(80).clamp(1, 500);
    let page = f.page.unwrap_or(1).max(1);
    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM projects p JOIN partners c ON c.id = p.client_id WHERE 1 = 1");
    push_filters(&mut count, f, user_id);
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;
    let mut qb = QueryBuilder::<Sqlite>::new(select_project!("WHERE 1 = 1"));
    push_filters(&mut qb, f, user_id);
    qb.push(" ORDER BY p.status = 'TERMINE', p.created_at DESC, p.id DESC LIMIT ").push_bind(per_page).push(" OFFSET ").push_bind((page - 1) * per_page);
    let items = qb.build_query_as::<ProjectRow>().fetch_all(pool).await?;
    Ok(Page { items, total, page, per_page })
}

// ---------- Routes ----------

// GET /projects
pub async fn list_handler(State(st): State<AppState>, user: CurrentUser, Query(f): Query<ProjectFilters>) -> AppResult<Json<Page<ProjectRow>>> {
    Ok(Json(list(&st.pool, &f, user.id).await?))
}

// GET /projects/{id}
pub async fn get_handler(State(st): State<AppState>, _user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<ProjectDetail>> {
    let mut conn = st.pool.acquire().await?;
    Ok(Json(load(&mut conn, id).await?))
}

// POST /projects : création d'un projet simple (kind PROJET).
pub async fn create_handler(State(st): State<AppState>, user: CurrentUser, Json(mut i): Json<ProjectInput>) -> AppResult<Json<ProjectDetail>> {
    user.require(Permission::ManageBusinessData)?;
    normalize(&mut i)?;
    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let id = insert(&mut tx, user.id, "PROJET", &i).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(d))
}

// PUT /projects/{id} ; une mission se modifie depuis sa propre fiche.
pub async fn update_handler(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(mut i): Json<ProjectInput>) -> AppResult<Json<ProjectDetail>> {
    user.require(Permission::ManageBusinessData)?;
    normalize(&mut i)?;
    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let kind: String = sqlx::query_scalar("SELECT kind FROM projects WHERE id = ?").bind(id).fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::NotFound("Projet introuvable".into()))?;
    if kind == "FORMATION" {
        return Err(AppError::Validation("Ce projet est une mission de formation : modifiez-le depuis la fiche mission".into()));
    }
    update_in_tx(&mut tx, user.id, id, &i).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(d))
}

#[derive(Deserialize)]
// Archiver (true) ou désarchiver (false).
pub struct ArchiveInput {
    pub archived: bool,
}

// POST /projects/{id}/archive : un projet archivé disparaît des listes mais garde son historique.
pub async fn archive_handler(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(a): Json<ArchiveInput>) -> AppResult<Json<ProjectDetail>> {
    user.require(Permission::ManageBusinessData)?;
    let mut tx = st.pool.begin().await?;
    load(&mut tx, id).await?;
    sqlx::query("UPDATE projects SET archived_at = CASE WHEN ? THEN strftime('%Y-%m-%dT%H:%M:%SZ', 'now') ELSE NULL END WHERE id = ?")
        .bind(a.archived).bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user.id), if a.archived { "ARCHIVAGE" } else { "DESARCHIVAGE" }, ENTITY, Some(id.to_string()), None).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(d))
}

// DELETE /projects/{id} : refusé s'il y a des temps ou des factures (archiver à la place).
pub async fn delete_handler(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    user.require(Permission::ManageBusinessData)?;
    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let d = load(&mut tx, id).await?;
    if d.row.mission_id.is_some() {
        return Err(AppError::Validation("Supprimez la mission depuis sa fiche".into()));
    }
    let used: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM timesheets WHERE project_id = ?1) + (SELECT COUNT(*) FROM invoices WHERE project_id = ?1)")
        .bind(id).fetch_one(&mut *tx).await?;
    if used > 0 {
        return Err(AppError::Conflict("Impossible de supprimer ce projet : il contient des temps ou des factures. Archivez-le plutôt.".into()));
    }
    sqlx::query("DELETE FROM projects WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user.id), "SUPPRESSION", ENTITY, Some(id.to_string()), Some(audit_view(&d))).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
// Création (name) ou modification partielle d'une tâche.
pub struct TaskInput {
    pub name: Option<String>,
    pub active: Option<bool>,
}

// POST /projects/{id}/tasks ; le nom est unique par projet.
pub async fn create_task(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(t): Json<TaskInput>) -> AppResult<Json<Task>> {
    user.require(Permission::ManageBusinessData)?;
    let name = normalize_spaces(t.name.as_deref().unwrap_or(""));
    if name.is_empty() {
        return Err(AppError::Validation("Le nom de la tâche est obligatoire".into()));
    }
    let mut tx = st.pool.begin().await?;
    load(&mut tx, id).await?;
    let task = sqlx::query_as::<_, Task>("INSERT INTO tasks (project_id, name) VALUES (?, ?) RETURNING id, project_id, name, active")
        .bind(id).bind(&name).fetch_one(&mut *tx).await
        .map_err(|e| match AppError::from(e) { AppError::Conflict(_) => AppError::Conflict("Cette tâche existe déjà".into()), o => o })?;
    audit::record(&mut tx, Some(user.id), "CREATION_TACHE", ENTITY, Some(id.to_string()), Some(json!({ "task": name }))).await?;
    tx.commit().await?;
    Ok(Json(task))
}

// PATCH /tasks/{id} : renommage ou (dés)activation.
pub async fn update_task(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(t): Json<TaskInput>) -> AppResult<Json<Task>> {
    user.require(Permission::ManageBusinessData)?;
    let mut tx = st.pool.begin().await?;
    let cur = sqlx::query_as::<_, Task>("SELECT id, project_id, name, active FROM tasks WHERE id = ?").bind(id).fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::NotFound("Tâche introuvable".into()))?;
    let name = t.name.map(|n| normalize_spaces(&n)).unwrap_or(cur.name.clone());
    if name.is_empty() {
        return Err(AppError::Validation("Le nom de la tâche est obligatoire".into()));
    }
    let task = sqlx::query_as::<_, Task>("UPDATE tasks SET name = ?, active = ? WHERE id = ? RETURNING id, project_id, name, active")
        .bind(&name).bind(t.active.unwrap_or(cur.active)).bind(id).fetch_one(&mut *tx).await?;
    audit::record(&mut tx, Some(user.id), "MODIFICATION_TACHE", ENTITY, Some(cur.project_id.to_string()),
        Some(audit::diff(&json!({"task": cur.name, "active": cur.active}), &json!({"task": task.name, "active": task.active})))).await?;
    tx.commit().await?;
    Ok(Json(task))
}

/// Liste simple des utilisateurs actifs (intervenants, formateurs).
pub async fn list_people(State(st): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<Member>>> {
    Ok(Json(sqlx::query_as::<_, Member>("SELECT id, display_name FROM users WHERE active = 1 ORDER BY display_name COLLATE NOCASE").fetch_all(&st.pool).await?))
}
