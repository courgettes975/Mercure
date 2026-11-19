//! Lot 4 : feuilles de temps.
//! - Un UTILISATEUR ne voit et ne saisit que ses propres temps.
//! - Les temps générés par une session se modifient depuis la session (pas de double saisie).
//! - Un temps facturé est verrouillé.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

use crate::{
    auth::CurrentUser,
    domain::{
        money::Money,
        roles::Permission,
        validation::{parse_date, search_key},
    },
    error::{AppError, AppResult},
    modules::{audit, missions::model::effective_hourly_rate},
    state::AppState,
};

// Type d'entité dans le journal d'audit.
pub const ENTITY: &str = "timesheet";

#[derive(Debug, Serialize, sqlx::FromRow)]
// Ligne de temps enrichie (projet, client, centre, tâche, facture) telle qu'affichée.
pub struct TimesheetRow {
    pub id: i64,
    pub date: String,
    pub user_id: i64,
    pub user_name: String,
    pub project_id: i64,
    pub project_name: String,
    pub project_reference: String,
    pub client_id: i64,
    pub client_name: String,
    pub task_id: Option<i64>,
    pub task_name: Option<String>,
    // Renseigné quand la ligne a été générée par une session de formation réalisée.
    pub session_id: Option<i64>,
    pub mission_id: Option<i64>,
    pub center_id: Option<i64>,
    pub center_name: Option<String>,
    pub description: String,
    pub duration_minutes: i64,
    pub billable: bool,
    // Taux et montant figés au moment de la saisie (un changement de tarif ultérieur ne les modifie pas).
    pub hourly_rate: i64,
    pub amount: i64,
    pub status: String,
    // Lien vers la ligne de facture : non nul = temps facturé, verrouillé.
    pub invoice_line_id: Option<i64>,
    pub invoice_id: Option<i64>,
    pub invoice_number: Option<String>,
    pub invoiced_at: Option<String>,
}

// Lecture commune : toutes les jointures nécessaires à l'affichage.
macro_rules! select_ts {
    ($tail:literal) => {
        concat!(
            "SELECT t.id, t.date, t.user_id, u.display_name AS user_name, t.project_id, p.name AS project_name,
                p.reference AS project_reference, p.client_id, TRIM(c.name || ' ' || c.first_name) AS client_name,
                t.task_id, tk.name AS task_name, t.session_id, m.id AS mission_id, m.center_id, ce.name AS center_name,
                t.description, t.duration_minutes,
                t.billable, t.hourly_rate, t.amount, t.status, t.invoice_line_id, il.invoice_id, i.number AS invoice_number,
                t.invoiced_at
             FROM timesheets t JOIN users u ON u.id = t.user_id JOIN projects p ON p.id = t.project_id
             JOIN partners c ON c.id = p.client_id LEFT JOIN tasks tk ON tk.id = t.task_id
             LEFT JOIN missions m ON m.project_id = p.id LEFT JOIN partners ce ON ce.id = m.center_id
             LEFT JOIN invoice_lines il ON il.id = t.invoice_line_id
             LEFT JOIN invoices i ON i.id = il.invoice_id ",
            $tail
        )
    };
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
// Filtres de la liste (paramètres d'URL).
pub struct Filters {
    pub from: Option<String>,
    pub to: Option<String>,
    pub user_id: Option<i64>,
    pub project_id: Option<i64>,
    pub client_id: Option<i64>,
    pub q: Option<String>,
    pub billable: Option<bool>,
    /// "oui" | "non"
    pub invoiced: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, Default)]
// Totaux calculés sur l'ensemble filtré (pas seulement la page affichée).
pub struct Totals {
    pub minutes: i64,
    pub billable_minutes: i64,
    pub billable_amount: i64,
    pub uninvoiced_amount: i64,
}

#[derive(Debug, Serialize)]
// Liste + totaux ; `truncated` signale que la limite a été atteinte.
pub struct ListResponse {
    pub items: Vec<TimesheetRow>,
    pub totals: Totals,
    pub truncated: bool,
}

// Traduit les filtres en SQL paramétré ; applique la restriction de rôle.
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &Filters, user: &CurrentUser) {
    // Un utilisateur simple ne voit que ses temps, quoi qu'il demande.
    let user_filter = if user.role.can(Permission::ManageBusinessData) { f.user_id } else { Some(user.id) };
    if let Some(u) = user_filter {
        qb.push(" AND t.user_id = ").push_bind(u);
    }
    if let Some(d) = f.from.as_deref().filter(|d| parse_date(d).is_some()) {
        qb.push(" AND t.date >= ").push_bind(d.to_string());
    }
    if let Some(d) = f.to.as_deref().filter(|d| parse_date(d).is_some()) {
        qb.push(" AND t.date <= ").push_bind(d.to_string());
    }
    if let Some(p) = f.project_id {
        qb.push(" AND t.project_id = ").push_bind(p);
    }
    if let Some(c) = f.client_id {
        qb.push(" AND p.client_id = ").push_bind(c);
    }
    if let Some(b) = f.billable {
        qb.push(" AND t.billable = ").push_bind(b);
    }
    match f.invoiced.as_deref() {
        Some("oui") => { qb.push(" AND t.invoice_line_id IS NOT NULL"); }
        Some("non") => { qb.push(" AND t.invoice_line_id IS NULL"); }
        _ => {}
    }
    if let Some(s) = f.status.as_deref().filter(|s| ["SAISI", "VALIDE"].contains(s)) {
        qb.push(" AND t.status = ").push_bind(s.to_string());
    }
    if let Some(q) = f.q.as_deref() {
        for w in search_key(q).split(' ').filter(|w| !w.is_empty()) {
            let like = format!("%{}%", w.replace(['%', '_'], ""));
            qb.push(" AND (lower(t.description) LIKE ").push_bind(like.clone())
                .push(" OR lower(p.name) LIKE ").push_bind(like.clone())
                .push(" OR c.search_text LIKE ").push_bind(like.clone())
                .push(" OR lower(COALESCE(tk.name, '')) LIKE ").push_bind(like).push(")");
        }
    }
}

// GET /timesheets : liste (limite +1 pour détecter la troncature) puis totaux sur les mêmes filtres.
pub async fn list(State(st): State<AppState>, user: CurrentUser, Query(f): Query<Filters>) -> AppResult<Json<ListResponse>> {
    let limit = f.limit.unwrap_or(2000).clamp(1, 10_000);
    let mut qb = QueryBuilder::<Sqlite>::new(select_ts!("WHERE 1 = 1"));
    push_filters(&mut qb, &f, &user);
    qb.push(" ORDER BY t.date DESC, t.id DESC LIMIT ").push_bind(limit + 1);
    let mut items: Vec<TimesheetRow> = qb.build_query_as().fetch_all(&st.pool).await?;
    let truncated = items.len() as i64 > limit;
    items.truncate(limit as usize);

    let mut tq = QueryBuilder::<Sqlite>::new(
        "SELECT COALESCE(SUM(t.duration_minutes), 0), COALESCE(SUM(CASE WHEN t.billable = 1 THEN t.duration_minutes END), 0),
            COALESCE(SUM(CASE WHEN t.billable = 1 THEN t.amount END), 0),
            COALESCE(SUM(CASE WHEN t.billable = 1 AND t.invoice_line_id IS NULL THEN t.amount END), 0)
         FROM timesheets t JOIN projects p ON p.id = t.project_id JOIN partners c ON c.id = p.client_id
         LEFT JOIN tasks tk ON tk.id = t.task_id WHERE 1 = 1",
    );
    push_filters(&mut tq, &f, &user);
    let (minutes, billable_minutes, billable_amount, uninvoiced_amount): (i64, i64, i64, i64) = tq.build_query_as().fetch_one(&st.pool).await?;
    Ok(Json(ListResponse { items, totals: Totals { minutes, billable_minutes, billable_amount, uninvoiced_amount }, truncated }))
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
// Saisie d'une ligne ; `user_id` permet à un gestionnaire de saisir pour un autre.
pub struct TimesheetInput {
    pub date: String,
    pub project_id: i64,
    pub task_id: Option<i64>,
    pub description: String,
    pub duration_minutes: i64,
    pub billable: Option<bool>,
    pub user_id: Option<i64>,
}

/// Taux et facturabilité par défaut du projet.
async fn project_rate(conn: &mut SqliteConnection, project_id: i64) -> AppResult<(i64, bool, String)> {
    let row: Option<(i64, bool, String, Option<String>, Option<i64>, String)> = sqlx::query_as(
        "SELECT p.hourly_rate, p.billable, p.status, m.pricing_mode, m.daily_rate, p.kind
         FROM projects p LEFT JOIN missions m ON m.project_id = p.id WHERE p.id = ?",
    )
    .bind(project_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (hourly, billable, status, mode, daily, _kind) = row.ok_or_else(|| AppError::Validation("Projet introuvable".into()))?;
    // Mission au tarif journalier : converti en taux horaire (1 jour = 7 h) ; forfait : temps non facturable.
    let mode = mode.unwrap_or_else(|| "HORAIRE".into());
    let rate = effective_hourly_rate(&mode, hourly, daily.unwrap_or(0));
    Ok((rate, billable && mode != "FORFAIT", status))
}

// Contrôles de saisie ; renvoie l'utilisateur cible (soi-même, ou un autre si gestionnaire).
async fn validate_input(conn: &mut SqliteConnection, i: &mut TimesheetInput, user: &CurrentUser) -> AppResult<i64> {
    i.description = i.description.trim().to_string();
    if parse_date(&i.date).is_none() {
        return Err(AppError::Validation("Date invalide".into()));
    }
    if i.duration_minutes <= 0 {
        return Err(AppError::Validation("La durée doit être supérieure à 0".into()));
    }
    if i.duration_minutes > 24 * 60 {
        return Err(AppError::Validation("La durée ne peut pas dépasser 24 h".into()));
    }
    let target_user = match i.user_id {
        Some(u) if u != user.id => {
            user.require(Permission::ManageBusinessData)?;
            u
        }
        _ => user.id,
    };
    if let Some(t) = i.task_id {
        let pid: Option<i64> = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = ?").bind(t).fetch_optional(&mut *conn).await?;
        if pid != Some(i.project_id) {
            return Err(AppError::Validation("La tâche n'appartient pas à ce projet".into()));
        }
    }
    Ok(target_user)
}

// Relit une ligne complète après écriture.
async fn load_row(conn: &mut SqliteConnection, id: i64) -> AppResult<TimesheetRow> {
    sqlx::query_as::<_, TimesheetRow>(select_ts!("WHERE t.id = ?"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Ligne de temps introuvable".into()))
}

/// Contrôle d'accès et verrous avant modification.
fn check_editable(row: &TimesheetRow, user: &CurrentUser) -> AppResult<()> {
    if row.user_id != user.id && !user.role.can(Permission::ManageBusinessData) {
        return Err(AppError::Forbidden);
    }
    if row.invoice_line_id.is_some() {
        return Err(AppError::Validation("Temps facturé : il ne peut plus être modifié".into()));
    }
    if row.session_id.is_some() {
        return Err(AppError::Validation("Temps généré par une session de formation : modifiez la session depuis la mission".into()));
    }
    if row.status == "VALIDE" && !user.role.can(Permission::ManageBusinessData) {
        return Err(AppError::Validation("Temps validé : seul un gestionnaire peut le modifier".into()));
    }
    Ok(())
}

// POST /timesheets : montant = minutes / 60 x taux du projet, figé dans la ligne.
pub async fn create(State(st): State<AppState>, user: CurrentUser, Json(mut i): Json<TimesheetInput>) -> AppResult<Json<TimesheetRow>> {
    user.require(Permission::EnterOwnTime)?;
    let mut tx = st.pool.begin().await?;
    let uid = validate_input(&mut tx, &mut i, &user).await?;
    let (rate, project_billable, status) = project_rate(&mut tx, i.project_id).await?;
    if status == "ANNULE" {
        return Err(AppError::Validation("Projet annulé : saisie impossible".into()));
    }
    // Une ligne ne peut pas être facturable si le projet ne l'est pas.
    let billable = i.billable.unwrap_or(project_billable) && project_billable;
    let amount = if billable { Money::for_minutes(Money(rate), i.duration_minutes).0 } else { 0 };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO timesheets (date, user_id, project_id, task_id, description, duration_minutes, billable, hourly_rate, amount)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&i.date).bind(uid).bind(i.project_id).bind(i.task_id).bind(&i.description).bind(i.duration_minutes)
    .bind(billable).bind(rate).bind(amount)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(&mut tx, Some(user.id), "CREATION", ENTITY, Some(id.to_string()),
        Some(json!({ "date": i.date, "project": i.project_id, "minutes": i.duration_minutes }))).await?;
    let row = load_row(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(row))
}

// PUT /timesheets/{id} : mêmes contrôles qu'à la création, après vérification des verrous.
pub async fn update(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(mut i): Json<TimesheetInput>) -> AppResult<Json<TimesheetRow>> {
    user.require(Permission::EnterOwnTime)?;
    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = load_row(&mut tx, id).await?;
    check_editable(&before, &user)?;
    if i.user_id.is_none() {
        i.user_id = Some(before.user_id);
    }
    let uid = validate_input(&mut tx, &mut i, &user).await?;
    let (rate, project_billable, _) = project_rate(&mut tx, i.project_id).await?;
    let billable = i.billable.unwrap_or(before.billable) && project_billable;
    let amount = if billable { Money::for_minutes(Money(rate), i.duration_minutes).0 } else { 0 };
    sqlx::query(
        "UPDATE timesheets SET date=?, user_id=?, project_id=?, task_id=?, description=?, duration_minutes=?, billable=?, hourly_rate=?,
            amount=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
    )
    .bind(&i.date).bind(uid).bind(i.project_id).bind(i.task_id).bind(&i.description).bind(i.duration_minutes)
    .bind(billable).bind(rate).bind(amount).bind(id)
    .execute(&mut *tx)
    .await?;
    let after = load_row(&mut tx, id).await?;
    let changes = audit::diff(
        &json!({ "date": before.date, "project": before.project_name, "task": before.task_name, "description": before.description, "minutes": before.duration_minutes, "billable": before.billable }),
        &json!({ "date": after.date, "project": after.project_name, "task": after.task_name, "description": after.description, "minutes": after.duration_minutes, "billable": after.billable }),
    );
    audit::record(&mut tx, Some(user.id), "MODIFICATION", ENTITY, Some(id.to_string()), Some(changes)).await?;
    tx.commit().await?;
    Ok(Json(after))
}

// DELETE /timesheets/{id} : refusé si facturé, issu d'une session, ou validé (pour un utilisateur).
pub async fn delete(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    user.require(Permission::EnterOwnTime)?;
    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let row = load_row(&mut tx, id).await?;
    check_editable(&row, &user)?;
    sqlx::query("DELETE FROM timesheets WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user.id), "SUPPRESSION", ENTITY, Some(id.to_string()),
        Some(json!({ "date": row.date, "project": row.project_name, "minutes": row.duration_minutes }))).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
// Date cible de la copie (par défaut la même date).
pub struct DuplicateInput {
    pub date: Option<String>,
}

// POST /timesheets/{id}/duplicate : recopie une ligne (gain de temps pour les saisies répétitives).
pub async fn duplicate(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(d): Json<DuplicateInput>) -> AppResult<Json<TimesheetRow>> {
    let row = {
        let mut conn = st.pool.acquire().await?;
        load_row(&mut conn, id).await?
    };
    if row.user_id != user.id && !user.role.can(Permission::ManageBusinessData) {
        return Err(AppError::Forbidden);
    }
    let input = TimesheetInput {
        date: d.date.unwrap_or(row.date),
        project_id: row.project_id,
        task_id: row.task_id,
        description: row.description,
        duration_minutes: row.duration_minutes,
        billable: Some(row.billable),
        user_id: Some(row.user_id),
    };
    create(State(st), user, Json(input)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
// Une case de la grille hebdomadaire : projet / tâche / jour -> total de minutes voulu.
pub struct CellInput {
    pub date: String,
    pub project_id: i64,
    pub task_id: Option<i64>,
    pub minutes: i64,
}

/// Saisie depuis la grille : fixe le total d'une case (projet / tâche / jour) pour l'utilisateur courant.
pub async fn set_cell(State(st): State<AppState>, user: CurrentUser, Json(c): Json<CellInput>) -> AppResult<Json<Value>> {
    user.require(Permission::EnterOwnTime)?;
    if parse_date(&c.date).is_none() {
        return Err(AppError::Validation("Date invalide".into()));
    }
    if !(0..=24 * 60).contains(&c.minutes) {
        return Err(AppError::Validation("Durée invalide (entre 0 et 24 h)".into()));
    }
    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    // Lignes existantes de la case ; les verrouillées comptent dans le total mais ne bougent pas.
    let lines: Vec<(i64, i64, bool)> = sqlx::query_as(
        "SELECT id, duration_minutes, (invoice_line_id IS NOT NULL OR session_id IS NOT NULL OR status = 'VALIDE') AS locked
         FROM timesheets WHERE user_id = ? AND date = ? AND project_id = ? AND task_id IS ?",
    )
    .bind(user.id).bind(&c.date).bind(c.project_id).bind(c.task_id)
    .fetch_all(&mut *tx)
    .await?;
    let locked: i64 = lines.iter().filter(|l| l.2).map(|l| l.1).sum();
    let editable: Vec<&(i64, i64, bool)> = lines.iter().filter(|l| !l.2).collect();
    let wanted = c.minutes - locked;
    if wanted < 0 {
        return Err(AppError::Validation(format!("Cette case contient déjà {} min verrouillées (facturées, validées ou issues d'une session)", locked)));
    }
    if editable.len() > 1 {
        return Err(AppError::Validation("Plusieurs lignes pour ce jour : modifiez-les depuis la vue liste".into()));
    }
    let (rate, project_billable, status) = project_rate(&mut tx, c.project_id).await?;
    // Selon le cas : suppression (0 min), mise à jour de la ligne libre, ou création.
    match editable.first() {
        Some((id, _, _)) if wanted == 0 => {
            sqlx::query("DELETE FROM timesheets WHERE id = ?").bind(id).execute(&mut *tx).await?;
        }
        Some((id, _, _)) => {
            let amount: i64 = sqlx::query_scalar("SELECT CASE WHEN billable = 1 THEN ? ELSE 0 END FROM timesheets WHERE id = ?")
                .bind(Money::for_minutes(Money(rate), wanted).0).bind(id).fetch_one(&mut *tx).await?;
            sqlx::query("UPDATE timesheets SET duration_minutes = ?, hourly_rate = ?, amount = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
                .bind(wanted).bind(rate).bind(amount).bind(id).execute(&mut *tx).await?;
        }
        None if wanted > 0 => {
            if status == "ANNULE" {
                return Err(AppError::Validation("Projet annulé : saisie impossible".into()));
            }
            if let Some(t) = c.task_id {
                let pid: Option<i64> = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = ?").bind(t).fetch_optional(&mut *tx).await?;
                if pid != Some(c.project_id) {
                    return Err(AppError::Validation("La tâche n'appartient pas à ce projet".into()));
                }
            }
            sqlx::query(
                "INSERT INTO timesheets (date, user_id, project_id, task_id, duration_minutes, billable, hourly_rate, amount) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&c.date).bind(user.id).bind(c.project_id).bind(c.task_id).bind(wanted).bind(project_billable).bind(rate)
            .bind(if project_billable { Money::for_minutes(Money(rate), wanted).0 } else { 0 })
            .execute(&mut *tx)
            .await?;
        }
        None => {}
    }
    audit::record(&mut tx, Some(user.id), "SAISIE_GRILLE", ENTITY, None,
        Some(json!({ "date": c.date, "project": c.project_id, "task": c.task_id, "minutes": c.minutes }))).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
// Validation en lot : identifiants et statut cible (VALIDE par défaut).
pub struct ValidateInput {
    pub ids: Vec<i64>,
    pub status: Option<String>,
}

/// Validation (ou retour en saisie) de lignes par un gestionnaire.
pub async fn validate(State(st): State<AppState>, user: CurrentUser, Json(v): Json<ValidateInput>) -> AppResult<Json<Value>> {
    user.require(Permission::ManageBusinessData)?;
    let status = v.status.unwrap_or_else(|| "VALIDE".into());
    if !["SAISI", "VALIDE"].contains(&status.as_str()) {
        return Err(AppError::Validation("Statut inconnu".into()));
    }
    let mut tx = st.pool.begin().await?;
    let mut n = 0;
    for id in &v.ids {
        n += sqlx::query("UPDATE timesheets SET status = ? WHERE id = ? AND invoice_line_id IS NULL AND session_id IS NULL")
            .bind(&status).bind(id).execute(&mut *tx).await?.rows_affected();
    }
    audit::record(&mut tx, Some(user.id), if status == "VALIDE" { "VALIDATION" } else { "DEVALIDATION" }, ENTITY, None, Some(json!({ "ids": v.ids, "count": n }))).await?;
    tx.commit().await?;
    Ok(Json(json!({ "updated": n })))
}

/// Projets et tâches proposés à la saisie.
#[derive(Debug, Serialize, sqlx::FromRow)]
// Un couple projet / tâche proposé dans les listes de saisie.
pub struct Choice {
    pub project_id: i64,
    pub project_name: String,
    pub client_name: String,
    pub kind: String,
    pub task_id: Option<i64>,
    pub task_name: Option<String>,
}

// GET /timesheets/choices : projets ouverts et leurs tâches actives.
pub async fn choices(State(st): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<Choice>>> {
    Ok(Json(
        sqlx::query_as::<_, Choice>(
            "SELECT p.id AS project_id, p.name AS project_name, TRIM(c.name || ' ' || c.first_name) AS client_name, p.kind,
                t.id AS task_id, t.name AS task_name
             FROM projects p JOIN partners c ON c.id = p.client_id LEFT JOIN tasks t ON t.project_id = p.id AND t.active = 1
             WHERE p.archived_at IS NULL AND p.status IN ('BROUILLON', 'EN_COURS')
             ORDER BY c.name COLLATE NOCASE, p.name COLLATE NOCASE, t.name COLLATE NOCASE",
        )
        .fetch_all(&st.pool)
        .await?,
    ))
}
