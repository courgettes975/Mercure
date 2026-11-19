//! Lot 8 : tableau de bord général (indicateurs calculés sur les données réelles).

use axum::{extract::State, Json};
use chrono::Datelike;
use serde::Serialize;

use crate::{auth::CurrentUser, domain::roles::Permission, error::AppResult, state::AppState};

#[derive(Debug, Serialize, Default)]
// Indicateurs du tableau de bord ; les champs financiers restent à 0 pour un simple UTILISATEUR.
pub struct Dashboard {
    // Contacts actifs et clients parmi eux.
    pub contacts: i64,
    pub clients: i64,
    // Temps du mois (tous, ou les siens pour un utilisateur).
    pub month_minutes: i64,
    pub month_billable_minutes: i64,
    // Temps et frais facturables pas encore facturés.
    pub uninvoiced_amount: i64,
    // Factures validées en attente de paiement (nombre et reste dû)...
    pub pending_invoices: i64,
    pub pending_amount: i64,
    // ... dont celles dont l'échéance est dépassée.
    pub overdue_invoices: i64,
    pub overdue_amount: i64,
    pub paid_invoices_year: i64,
    // Chiffre d'affaires HT de l'année et du mois (avoirs déduits).
    pub revenue_year: i64,
    pub revenue_month: i64,
    /// Dépenses TTC de l'année et du mois, et montant des dépenses à payer.
    pub expenses_year: i64,
    pub expenses_month: i64,
    pub expenses_to_pay: i64,
    pub draft_invoices: i64,
    pub missions_to_invoice: i64,
    pub sessions_this_week: i64,
    /// Chiffre d'affaires HT des 12 derniers mois : (AAAA-MM, centimes).
    pub revenue_by_month: Vec<(String, i64)>,
    pub can_see_finance: bool,
}

// Calcule tous les indicateurs en quelques requêtes agrégées.
pub async fn get(State(st): State<AppState>, user: CurrentUser) -> AppResult<Json<Dashboard>> {
    let pool = &st.pool;
    let today = chrono::Local::now().date_naive();
    let month_start = today.with_day(1).unwrap_or(today).format("%Y-%m-%d").to_string();
    let year_start = format!("{}-01-01", today.year());
    let today_s = today.format("%Y-%m-%d").to_string();
    let finance = user.role.can(Permission::ManageBusinessData);
    // Temps : tous pour un gestionnaire, les siens pour un utilisateur.
    let uid: Option<i64> = if finance { None } else { Some(user.id) };

    let mut d = Dashboard { can_see_finance: finance, ..Default::default() };
    (d.contacts, d.clients) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(is_client), 0) FROM partners WHERE archived_at IS NULL",
    )
    .fetch_one(pool)
    .await?;
    (d.month_minutes, d.month_billable_minutes) = sqlx::query_as(
        "SELECT COALESCE(SUM(duration_minutes), 0), COALESCE(SUM(CASE WHEN billable = 1 THEN duration_minutes END), 0)
         FROM timesheets WHERE date >= ? AND (?2 IS NULL OR user_id = ?2)",
    )
    .bind(&month_start)
    .bind(uid)
    .fetch_one(pool)
    .await?;
    let (mon, sun) = crate::modules::missions::service::week_bounds();
    d.sessions_this_week = sqlx::query_scalar("SELECT COUNT(*) FROM training_sessions WHERE status <> 'ANNULEE' AND date BETWEEN ? AND ?")
        .bind(mon)
        .bind(sun)
        .fetch_one(pool)
        .await?;
    if !finance {
        return Ok(Json(d));
    }
    d.uninvoiced_amount = sqlx::query_scalar(
        "SELECT COALESCE((SELECT SUM(amount) FROM timesheets WHERE billable = 1 AND invoice_line_id IS NULL), 0)
              + COALESCE((SELECT SUM(amount) FROM mission_expenses WHERE billable = 1 AND invoice_line_id IS NULL), 0)",
    )
    .fetch_one(pool)
    .await?;
    (d.pending_invoices, d.pending_amount) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(total_ttc - amount_paid), 0) FROM invoices WHERE kind = 'FACTURE' AND status = 'VALIDEE'",
    )
    .fetch_one(pool)
    .await?;
    (d.overdue_invoices, d.overdue_amount) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(total_ttc - amount_paid), 0) FROM invoices WHERE kind = 'FACTURE' AND status = 'VALIDEE' AND due_date < ?",
    )
    .bind(&today_s)
    .fetch_one(pool)
    .await?;
    d.paid_invoices_year = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE kind = 'FACTURE' AND status = 'PAYEE' AND invoice_date >= ?")
        .bind(&year_start)
        .fetch_one(pool)
        .await?;
    // CA HT : factures validées (y compris celles annulées depuis) moins les avoirs.
    let revenue = |from: String| {
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(CASE WHEN kind = 'AVOIR' THEN -total_ht ELSE total_ht END), 0) FROM invoices WHERE number IS NOT NULL AND invoice_date >= ?",
        )
        .bind(from)
        .fetch_one(pool)
    };
    d.revenue_year = revenue(year_start.clone()).await?;
    let spent = |from: String| sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(amount_ttc), 0) FROM expenses WHERE date >= ?").bind(from).fetch_one(pool);
    d.expenses_year = spent(year_start).await?;
    d.expenses_month = spent(month_start.clone()).await?;
    d.expenses_to_pay = sqlx::query_scalar("SELECT COALESCE(SUM(amount_ttc), 0) FROM expenses WHERE status = 'A_PAYER'").fetch_one(pool).await?;
    d.revenue_month = revenue(month_start).await?;
    d.draft_invoices = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE status = 'BROUILLON'").fetch_one(pool).await?;
    d.missions_to_invoice = sqlx::query_scalar("SELECT COUNT(*) FROM missions WHERE status = 'A_FACTURER'").fetch_one(pool).await?;

    // Les 12 derniers mois, du plus ancien au plus récent, avec 0 pour les mois sans facture.
    let mut months = Vec::new();
    let mut y = today.year();
    let mut m = today.month() as i32;
    for _ in 0..12 {
        months.push(format!("{y}-{m:02}"));
        m -= 1;
        if m == 0 {
            m = 12;
            y -= 1;
        }
    }
    months.reverse();
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT substr(invoice_date, 1, 7) AS ym, SUM(CASE WHEN kind = 'AVOIR' THEN -total_ht ELSE total_ht END)
         FROM invoices WHERE number IS NOT NULL AND invoice_date >= ? GROUP BY ym",
    )
    .bind(format!("{}-01", months[0]))
    .fetch_all(pool)
    .await?;
    d.revenue_by_month = months.into_iter().map(|ym| { let v = rows.iter().find(|r| r.0 == ym).map(|r| r.1).unwrap_or(0); (ym, v) }).collect();
    Ok(Json(d))
}
