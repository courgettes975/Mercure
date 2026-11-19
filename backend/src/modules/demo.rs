//! Lot 9 : jeu de données de démonstration, clairement fictif et supprimable.
//! Tout est rattaché à des contacts marqués is_demo = 1.

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::{
    auth::CurrentUser,
    domain::roles::Permission,
    error::{AppError, AppResult},
    modules::{audit, contacts, invoices, projects, timesheets},
    state::AppState,
};

// Indique à l'interface si le jeu de démonstration est chargé.
pub async fn status(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    user.require(Permission::ManageSettings)?;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partners WHERE is_demo = 1").fetch_one(&st.pool).await?;
    Ok(Json(json!({ "loaded": n > 0 })))
}

// Crée un client fictif, un stagiaire, un projet, un temps de 7 h et une facture brouillon.
pub async fn load(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    user.require(Permission::ManageSettings)?;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partners WHERE is_demo = 1").fetch_one(&st.pool).await?;
    if n > 0 {
        return Err(AppError::Conflict("Les données de démonstration sont déjà chargées".into()));
    }
    let company = contacts::service::create(&st.pool, user.id, contacts::model::ContactInput {
        kind: "ENTREPRISE".into(), name: "Demo Industrie SAS (FICTIF)".into(), is_client: true,
        street: "1 rue de la Démonstration".into(), zip: "33000".into(), city: "Bordeaux".into(), country: "France".into(),
        notes: "Données de démonstration fictives.".into(), ..Default::default()
    }).await?;
    let person = contacts::service::create(&st.pool, user.id, contacts::model::ContactInput {
        kind: "PARTICULIER".into(), name: "MARTIN (FICTIF)".into(), first_name: "Jean".into(), parent_id: Some(company.record.id),
        email: "jean.martin@exemple.invalid".into(), is_stagiaire: true, country: "France".into(), ..Default::default()
    }).await?;
    sqlx::query("UPDATE partners SET is_demo = 1 WHERE id IN (?, ?)").bind(company.record.id).bind(person.record.id).execute(&st.pool).await?;

    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let project_id = projects::insert(&mut tx, user.id, "PROJET", &projects::ProjectInput {
        name: "Formation Microsoft 365 (démo)".into(), client_id: company.record.id, status: "EN_COURS".into(),
        billable: true, hourly_rate: 8000, member_ids: vec![user.id], ..Default::default()
    }).await?;
    tx.commit().await?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let _ = timesheets::create(State(st.clone()), user.clone(), Json(timesheets::TimesheetInput {
        date: today, project_id, description: "Formation Teams (démo)".into(), duration_minutes: 420, ..Default::default()
    })).await?;
    invoices::service::from_items(&st.pool, user.id, invoices::service::FromItemsInput { project_id, ..Default::default() }).await?;
    let mut conn = st.pool.acquire().await?;
    audit::record(&mut conn, Some(user.id), "CHARGEMENT_DEMO", "demo", None, None).await?;
    Ok(Json(json!({ "ok": true })))
}

// Supprime tout le jeu de démonstration, sauf si une facture démo a été validée (numéro légal attribué).
pub async fn remove(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<Value>> {
    user.require(Permission::ManageSettings)?;
    let mut tx = st.pool.begin_with("BEGIN IMMEDIATE").await?;
    let validated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invoices WHERE number IS NOT NULL AND partner_id IN (SELECT id FROM partners WHERE is_demo = 1)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if validated > 0 {
        return Err(AppError::Conflict("Une facture de démonstration a été validée : elle ne peut plus être supprimée (numérotation légale)".into()));
    }
    // Suppression dans l'ordre des dépendances (temps -> factures -> missions -> projets -> contacts).
    let demo = "(SELECT id FROM partners WHERE is_demo = 1)";
    let projects_of = format!("(SELECT id FROM projects WHERE client_id IN {demo})");
    for sql in [
        format!("UPDATE timesheets SET invoice_line_id = NULL, invoiced_at = NULL WHERE project_id IN {projects_of}"),
        format!("UPDATE mission_expenses SET invoice_line_id = NULL, invoiced_at = NULL WHERE mission_id IN (SELECT id FROM missions WHERE project_id IN {projects_of})"),
        format!("DELETE FROM invoices WHERE partner_id IN {demo}"),
        format!("DELETE FROM timesheets WHERE project_id IN {projects_of}"),
        format!("DELETE FROM documents WHERE owner_type = 'MISSION' AND owner_id IN (SELECT id FROM missions WHERE project_id IN {projects_of})"),
        format!("DELETE FROM missions WHERE project_id IN {projects_of}"),
        format!("DELETE FROM projects WHERE client_id IN {demo}"),
        format!("DELETE FROM mission_participants WHERE partner_id IN {demo}"),
        "UPDATE partners SET parent_id = NULL WHERE is_demo = 1".to_string(),
        "DELETE FROM partners WHERE is_demo = 1".to_string(),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *tx).await.map_err(|e| match AppError::from(e) {
            AppError::Conflict(_) => AppError::Conflict("Des données réelles utilisent les contacts de démonstration : suppression impossible".into()),
            o => o,
        })?;
    }
    audit::record(&mut tx, Some(user.id), "SUPPRESSION_DEMO", "demo", None, None).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
