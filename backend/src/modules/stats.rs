//! Statistiques : extraction des données par critères (période, centre, client, formateur, logiciel...)
//! et regroupement libre. Les faits sont lus en SQL puis agrégés ici pour garantir les mêmes règles
//! quelle que soit la dimension choisie.

use axum::{
    extract::{Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::{
    auth::CurrentUser,
    domain::roles::Permission,
    error::{AppError, AppResult},
    state::AppState,
};

/// Dimensions communes à tous les faits (inconnues = None).
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct Dims {
    pub center_id: Option<i64>,
    pub center_name: Option<String>,
    pub client_id: Option<i64>,
    pub client_name: Option<String>,
    pub trainer_id: Option<i64>,
    pub trainer_name: Option<String>,
    pub software_id: Option<i64>,
    pub software_name: Option<String>,
    pub modality: Option<String>,
    pub group_format: Option<String>,
    pub os: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
// Paramètres d'URL : période (année en cours par défaut), regroupement et filtres.
pub struct StatsQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub group_by: Option<String>,
    pub center_id: Option<i64>,
    pub client_id: Option<i64>,
    pub trainer_id: Option<i64>,
    pub software_id: Option<i64>,
    pub modality: Option<String>,
    pub group_format: Option<String>,
    pub os: Option<String>,
    /// Pour l'export : timesheets | missions | invoices | payments
    pub dataset: Option<String>,
}

#[derive(Debug, Serialize, Default, Clone)]
// Une ligne du tableau : agrégats de temps, missions, factures et paiements pour un groupe.
pub struct StatRow {
    pub key: String,
    pub label: String,
    pub minutes: i64,
    pub billable_minutes: i64,
    pub timesheet_amount: i64,
    pub missions: i64,
    pub participants: i64,
    pub sessions: i64,
    pub session_minutes: i64,
    pub invoices: i64,
    pub invoiced_ht: i64,
    pub invoiced_time: i64,
    pub invoiced_expenses: i64,
    pub invoiced_flat: i64,
    pub paid: i64,
    /// Dépenses TTC de l'entreprise (toutes pour mois/année/total ; seulement celles liées à une mission pour les autres regroupements).
    pub expenses_ttc: i64,
    #[serde(skip)]
    // Stagiaires distincts du groupe (non sérialisé : sert au comptage).
    participant_ids: std::collections::BTreeSet<i64>,
}

#[derive(Debug, Serialize)]
// Résultat : lignes triées + ligne de total.
pub struct StatsReport {
    pub from: String,
    pub to: String,
    pub group_by: String,
    pub rows: Vec<StatRow>,
    pub total: StatRow,
}

#[derive(sqlx::FromRow)]
// Fait "temps" : une ligne de feuille de temps.
struct TimeFact {
    date: String,
    minutes: i64,
    billable: bool,
    amount: i64,
    #[sqlx(flatten)]
    d: Dims,
}

#[derive(sqlx::FromRow)]
// Fait "mission" : date d'ancrage, sessions réalisées, participants.
struct MissionFact {
    reference: String,
    title: String,
    anchor: Option<String>,
    status: String,
    sessions: i64,
    session_minutes: i64,
    participants_csv: String,
    #[sqlx(flatten)]
    d: Dims,
}

#[derive(sqlx::FromRow)]
// Fait "facture" : montants signés (avoir négatif), ventilés par origine des lignes.
struct InvoiceFact {
    id: i64,
    number: Option<String>,
    date: String,
    kind: String,
    status: String,
    partner_name: String,
    total_ht: i64,
    ht_time: i64,
    ht_expenses: i64,
    ht_flat: i64,
    #[sqlx(flatten)]
    d: Dims,
}

// Fait "dépense" : montant TTC, dimensions via la mission liée (sinon None).
#[derive(sqlx::FromRow)]
struct ExpenseFact {
    date: String,
    label: String,
    category: String,
    supplier: String,
    amount_ttc: i64,
    has_project: bool,
    #[sqlx(flatten)]
    d: Dims,
}

// Fait "paiement".
#[derive(sqlx::FromRow)]
struct PaymentFact {
    date: String,
    amount: i64,
    number: Option<String>,
    partner_name: String,
    method: String,
    #[sqlx(flatten)]
    d: Dims,
}

/// Dimensions d'une mission (jointures communes). `m` = missions, `p` = projects.
const MISSION_DIMS: &str = "m.center_id, ce.name AS center_name, p.client_id, TRIM(c.name || ' ' || c.first_name) AS client_name,
    m.trainer_id, u.display_name AS trainer_name, m.software_id, s.name AS software_name, m.modality, m.group_format, NULLIF(m.os, '') AS os";
const MISSION_JOINS: &str = "LEFT JOIN partners ce ON ce.id = m.center_id JOIN partners c ON c.id = p.client_id
    LEFT JOIN users u ON u.id = m.trainer_id LEFT JOIN software s ON s.id = m.software_id";

/// Dimensions d'une facture : via la mission du projet, sinon via le partenaire (centre ou client direct).
const INVOICE_DIMS: &str = "COALESCE(m.center_id, CASE WHEN pa.is_training_center = 1 THEN pa.id END) AS center_id,
    COALESCE(ce.name, CASE WHEN pa.is_training_center = 1 THEN pa.name END) AS center_name,
    COALESCE(p.client_id, CASE WHEN pa.is_training_center = 0 THEN pa.id END) AS client_id,
    COALESCE(TRIM(c.name || ' ' || c.first_name), CASE WHEN pa.is_training_center = 0 THEN TRIM(pa.name || ' ' || pa.first_name) END) AS client_name,
    m.trainer_id, u.display_name AS trainer_name, m.software_id, s.name AS software_name, m.modality, m.group_format, NULLIF(m.os, '') AS os";
const INVOICE_JOINS: &str = "JOIN partners pa ON pa.id = i.partner_id LEFT JOIN projects p ON p.id = i.project_id
    LEFT JOIN missions m ON m.project_id = p.id LEFT JOIN partners ce ON ce.id = m.center_id LEFT JOIN partners c ON c.id = p.client_id
    LEFT JOIN users u ON u.id = m.trainer_id LEFT JOIN software s ON s.id = m.software_id";

// Période effective : bornes fournies ou année civile en cours ; contrôle du format et de l'ordre.
fn period(q: &StatsQuery) -> AppResult<(String, String)> {
    let year = chrono::Local::now().format("%Y").to_string();
    let from = q.from.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{year}-01-01"));
    let to = q.to.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{year}-12-31"));
    for d in [&from, &to] {
        if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
            return Err(AppError::Validation("Dates attendues au format AAAA-MM-JJ".into()));
        }
    }
    if from > to {
        return Err(AppError::Validation("La date de début doit précéder la date de fin".into()));
    }
    Ok((from, to))
}

// Applique les filtres aux dimensions d'un fait ; une dimension inconnue (None) ne passe pas un filtre.
fn keep(q: &StatsQuery, d: &Dims) -> bool {
    q.center_id.is_none_or(|v| d.center_id == Some(v))
        && q.client_id.is_none_or(|v| d.client_id == Some(v))
        && q.trainer_id.is_none_or(|v| d.trainer_id == Some(v))
        && q.software_id.is_none_or(|v| d.software_id == Some(v))
        && q.modality.as_deref().filter(|s| !s.is_empty()).is_none_or(|v| d.modality.as_deref() == Some(v))
        && q.group_format.as_deref().filter(|s| !s.is_empty()).is_none_or(|v| d.group_format.as_deref() == Some(v))
        && q.os.as_deref().filter(|s| !s.is_empty()).is_none_or(|v| d.os.as_deref() == Some(v))
}

// Regroupements possibles.
pub const GROUPS: [&str; 10] = ["centre", "client", "formateur", "logiciel", "modalite", "format", "os", "mois", "annee", "total"];

// "2026-03" -> "mars 2026".
fn month_label(ym: &str) -> String {
    const M: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
    match (ym.get(0..4), ym.get(5..7).and_then(|m| m.parse::<usize>().ok())) {
        (Some(y), Some(m)) if (1..=12).contains(&m) => format!("{} {y}", M[m - 1]),
        _ => ym.to_string(),
    }
}

// Libellé d'un code parmi une table (code inconnu rendu tel quel).
fn label_of(code: &str, labels: &[(&str, &str)]) -> String {
    labels.iter().find(|(c, _)| *c == code).map(|(_, l)| l.to_string()).unwrap_or_else(|| code.to_string())
}

/// Clé et libellé de regroupement d'un fait daté.
fn group_key(group_by: &str, d: &Dims, date: &str) -> (String, String) {
    let opt = |id: Option<i64>, name: &Option<String>, none: &str| match id {
        Some(i) => (i.to_string(), name.clone().unwrap_or_default()),
        None => ("-".into(), none.into()),
    };
    match group_by {
        "centre" => opt(d.center_id, &d.center_name, "Sans centre (client direct)"),
        "client" => opt(d.client_id, &d.client_name, "Sans client"),
        "formateur" => opt(d.trainer_id, &d.trainer_name, "Sans formateur"),
        "logiciel" => opt(d.software_id, &d.software_name, "Sans logiciel"),
        "modalite" => match &d.modality { Some(m) => (m.clone(), label_of(m, &[("PRESENTIEL", "Présentiel"), ("DISTANCIEL", "Distanciel"), ("HYBRIDE", "Hybride")])), None => ("-".into(), "Non renseigné".into()) },
        "format" => match &d.group_format { Some(m) => (m.clone(), label_of(m, &[("INDIVIDUEL", "Individuel"), ("GROUPE", "Groupe")])), None => ("-".into(), "Non renseigné".into()) },
        "os" => match &d.os { Some(m) => (m.clone(), if m == "MAC" { "Mac".into() } else { m.clone() }), None => ("-".into(), "Non renseigné".into()) },
        "mois" => { let ym = date.get(0..7).unwrap_or(date).to_string(); (ym.clone(), month_label(&ym)) }
        "annee" => { let y = date.get(0..4).unwrap_or(date).to_string(); (y.clone(), y) }
        _ => ("total".into(), "Total".into()),
    }
}

// Temps de la période ; le formateur est l'auteur du temps, les autres dimensions viennent de la mission.
async fn time_facts(conn: &mut SqliteConnection, from: &str, to: &str) -> AppResult<Vec<TimeFact>> {
    let sql = String::from(
        "SELECT t.date, t.duration_minutes AS minutes, t.billable, CASE WHEN t.billable = 1 THEN t.amount ELSE 0 END AS amount,
            m.center_id, ce.name AS center_name, p.client_id, TRIM(c.name || ' ' || c.first_name) AS client_name,
            t.user_id AS trainer_id, u.display_name AS trainer_name, m.software_id, s.name AS software_name, m.modality, m.group_format, NULLIF(m.os, '') AS os
         FROM timesheets t JOIN projects p ON p.id = t.project_id JOIN users u ON u.id = t.user_id JOIN partners c ON c.id = p.client_id
         LEFT JOIN missions m ON m.project_id = p.id LEFT JOIN partners ce ON ce.id = m.center_id LEFT JOIN software s ON s.id = m.software_id
         WHERE t.date BETWEEN ? AND ? ORDER BY t.date"
    );
    Ok(sqlx::query_as::<_, TimeFact>(sqlx::AssertSqlSafe(sql)).bind(from).bind(to).fetch_all(&mut *conn).await?)
}

// Missions non annulées dont la date d'ancrage (première session, sinon début, sinon création) est dans la période.
async fn mission_facts(conn: &mut SqliteConnection, from: &str, to: &str) -> AppResult<Vec<MissionFact>> {
    let sql = format!(
        "SELECT m.reference, p.name AS title, m.status,
            COALESCE((SELECT MIN(ts.date) FROM training_sessions ts WHERE ts.mission_id = m.id AND ts.status <> 'ANNULEE'), p.start_date, substr(p.created_at, 1, 10)) AS anchor,
            (SELECT COUNT(*) FROM training_sessions ts WHERE ts.mission_id = m.id AND ts.status = 'REALISEE') AS sessions,
            COALESCE((SELECT SUM(ts.duration_minutes) FROM training_sessions ts WHERE ts.mission_id = m.id AND ts.status = 'REALISEE'), 0) AS session_minutes,
            COALESCE((SELECT GROUP_CONCAT(mp.partner_id) FROM mission_participants mp WHERE mp.mission_id = m.id), '') AS participants_csv,
            {MISSION_DIMS}
         FROM missions m JOIN projects p ON p.id = m.project_id {MISSION_JOINS}
         WHERE m.status <> 'ANNULEE' AND anchor BETWEEN ? AND ? ORDER BY anchor"
    );
    Ok(sqlx::query_as::<_, MissionFact>(sqlx::AssertSqlSafe(sql)).bind(from).bind(to).fetch_all(&mut *conn).await?)
}

// Factures validées/payées de la période, avec ventilation temps / frais / forfait et dimensions.
async fn invoice_facts(conn: &mut SqliteConnection, from: &str, to: &str) -> AppResult<Vec<InvoiceFact>> {
    let sql = format!(
        "SELECT i.id, i.number, i.invoice_date AS date, i.kind, i.status, TRIM(pa.name || ' ' || pa.first_name) AS partner_name,
            CASE WHEN i.kind = 'AVOIR' THEN -i.total_ht ELSE i.total_ht END AS total_ht,
            CASE WHEN i.kind = 'AVOIR' THEN -1 ELSE 1 END * COALESCE((SELECT SUM(l.total_ht) FROM invoice_lines l WHERE l.invoice_id = i.id AND l.source_type = 'TEMPS'), 0) AS ht_time,
            CASE WHEN i.kind = 'AVOIR' THEN -1 ELSE 1 END * COALESCE((SELECT SUM(l.total_ht) FROM invoice_lines l WHERE l.invoice_id = i.id AND l.source_type = 'FRAIS'), 0) AS ht_expenses,
            CASE WHEN i.kind = 'AVOIR' THEN -1 ELSE 1 END * COALESCE((SELECT SUM(l.total_ht) FROM invoice_lines l WHERE l.invoice_id = i.id AND l.source_type = 'FORFAIT'), 0) AS ht_flat,
            {INVOICE_DIMS}
         FROM invoices i {INVOICE_JOINS}
         WHERE i.status IN ('VALIDEE', 'PAYEE') AND i.invoice_date BETWEEN ? AND ? ORDER BY i.invoice_date, i.id"
    );
    Ok(sqlx::query_as::<_, InvoiceFact>(sqlx::AssertSqlSafe(sql)).bind(from).bind(to).fetch_all(&mut *conn).await?)
}

// Paiements de la période (hors factures annulées).
async fn payment_facts(conn: &mut SqliteConnection, from: &str, to: &str) -> AppResult<Vec<PaymentFact>> {
    let sql = format!(
        "SELECT py.payment_date AS date, CASE WHEN i.kind = 'AVOIR' THEN -py.amount ELSE py.amount END AS amount, i.number,
            TRIM(pa.name || ' ' || pa.first_name) AS partner_name, py.method, {INVOICE_DIMS}
         FROM payments py JOIN invoices i ON i.id = py.invoice_id {INVOICE_JOINS}
         WHERE i.status <> 'ANNULEE' AND py.payment_date BETWEEN ? AND ? ORDER BY py.payment_date, py.id"
    );
    Ok(sqlx::query_as::<_, PaymentFact>(sqlx::AssertSqlSafe(sql)).bind(from).bind(to).fetch_all(&mut *conn).await?)
}

// Dépenses de la période avec les dimensions de la mission liée.
async fn expense_facts(conn: &mut SqliteConnection, from: &str, to: &str) -> AppResult<Vec<ExpenseFact>> {
    let sql = String::from(
        "SELECT e.date, e.label, e.category, COALESCE(TRIM(s.name || ' ' || s.first_name), e.supplier_name) AS supplier, e.amount_ttc,
            e.project_id IS NOT NULL AS has_project,
            m.center_id, ce.name AS center_name, p.client_id, TRIM(c.name || ' ' || c.first_name) AS client_name,
            m.trainer_id, u.display_name AS trainer_name, m.software_id, sw.name AS software_name, m.modality, m.group_format, NULLIF(m.os, '') AS os
         FROM expenses e LEFT JOIN partners s ON s.id = e.supplier_id LEFT JOIN projects p ON p.id = e.project_id
         LEFT JOIN missions m ON m.project_id = p.id LEFT JOIN partners ce ON ce.id = m.center_id LEFT JOIN partners c ON c.id = p.client_id
         LEFT JOIN users u ON u.id = m.trainer_id LEFT JOIN software sw ON sw.id = m.software_id
         WHERE e.date BETWEEN ? AND ? ORDER BY e.date, e.id"
    );
    Ok(sqlx::query_as::<_, ExpenseFact>(sqlx::AssertSqlSafe(sql)).bind(from).bind(to).fetch_all(&mut *conn).await?)
}

/// Les regroupements calendaires ou total acceptent toutes les dépenses ; les autres exigent une mission liée.
fn expense_fits(group_by: &str, f: &ExpenseFact) -> bool {
    matches!(group_by, "mois" | "annee" | "total") || f.has_project
}

// Calcule le tableau : lecture des cinq jeux de faits, filtrage, regroupement, totaux, tri.
pub async fn report(conn: &mut SqliteConnection, q: &StatsQuery) -> AppResult<StatsReport> {
    let (from, to) = period(q)?;
    let group_by = q.group_by.clone().filter(|g| GROUPS.contains(&g.as_str())).unwrap_or_else(|| "centre".into());
    let mut rows: Vec<StatRow> = Vec::new();
    // Trouve ou crée la ligne d'un groupe.
    fn row(rows: &mut Vec<StatRow>, key: String, label: String) -> &mut StatRow {
        match rows.iter().position(|r| r.key == key) {
            Some(i) => &mut rows[i],
            None => {
                rows.push(StatRow { key, label, ..Default::default() });
                rows.last_mut().unwrap()
            }
        }
    }
    for f in time_facts(conn, &from, &to).await?.iter().filter(|f| keep(q, &f.d)) {
        let (k, l) = group_key(&group_by, &f.d, &f.date);
        let r = row(&mut rows, k, l);
        r.minutes += f.minutes;
        if f.billable {
            r.billable_minutes += f.minutes;
            r.timesheet_amount += f.amount;
        }
    }
    for f in mission_facts(conn, &from, &to).await?.iter().filter(|f| keep(q, &f.d)) {
        let (k, l) = group_key(&group_by, &f.d, f.anchor.as_deref().unwrap_or(""));
        let r = row(&mut rows, k, l);
        r.missions += 1;
        r.sessions += f.sessions;
        r.session_minutes += f.session_minutes;
        for id in f.participants_csv.split(',').filter_map(|s| s.parse::<i64>().ok()) {
            r.participant_ids.insert(id);
        }
    }
    for f in invoice_facts(conn, &from, &to).await?.iter().filter(|f| keep(q, &f.d)) {
        let (k, l) = group_key(&group_by, &f.d, &f.date);
        let r = row(&mut rows, k, l);
        r.invoices += 1;
        r.invoiced_ht += f.total_ht;
        r.invoiced_time += f.ht_time;
        r.invoiced_expenses += f.ht_expenses;
        r.invoiced_flat += f.ht_flat;
    }
    for f in payment_facts(conn, &from, &to).await?.iter().filter(|f| keep(q, &f.d)) {
        let (k, l) = group_key(&group_by, &f.d, &f.date);
        row(&mut rows, k, l).paid += f.amount;
    }
    for f in expense_facts(conn, &from, &to).await?.iter().filter(|f| expense_fits(&group_by, f) && keep(q, &f.d)) {
        let (k, l) = group_key(&group_by, &f.d, &f.date);
        row(&mut rows, k, l).expenses_ttc += f.amount_ttc;
    }
    let mut total = StatRow { key: "total".into(), label: "Total".into(), ..Default::default() };
    for r in &mut rows {
        r.participants = r.participant_ids.len() as i64;
        total.minutes += r.minutes;
        total.billable_minutes += r.billable_minutes;
        total.timesheet_amount += r.timesheet_amount;
        total.missions += r.missions;
        total.sessions += r.sessions;
        total.session_minutes += r.session_minutes;
        total.invoices += r.invoices;
        total.invoiced_ht += r.invoiced_ht;
        total.invoiced_time += r.invoiced_time;
        total.invoiced_expenses += r.invoiced_expenses;
        total.invoiced_flat += r.invoiced_flat;
        total.paid += r.paid;
        total.expenses_ttc += r.expenses_ttc;
        total.participant_ids.extend(r.participant_ids.iter().copied());
    }
    total.participants = total.participant_ids.len() as i64;
    // Tri chronologique pour mois/année, sinon par facturé décroissant puis heures.
    if matches!(group_by.as_str(), "mois" | "annee") {
        rows.sort_by(|a, b| a.key.cmp(&b.key));
    } else {
        rows.sort_by(|a, b| b.invoiced_ht.cmp(&a.invoiced_ht).then(b.minutes.cmp(&a.minutes)).then(a.label.cmp(&b.label)));
    }
    Ok(StatsReport { from, to, group_by, rows, total })
}

// GET /stats
pub async fn get(State(st): State<AppState>, user: CurrentUser, Query(q): Query<StatsQuery>) -> AppResult<Json<StatsReport>> {
    user.require(Permission::ManageBusinessData)?;
    let mut conn = st.pool.acquire().await?;
    Ok(Json(report(&mut conn, &q).await?))
}

// Cellule CSV : guillemets si nécessaire.
fn cell(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}
// Centimes -> "1234,56" (Excel FR).
fn eur(c: i64) -> String {
    format!("{},{:02}", c / 100, (c % 100).abs())
}
// Minutes -> heures décimales "7,50".
fn hours(m: i64) -> String {
    format!("{},{:02}", m / 60, (m % 60) * 100 / 60)
}
// Les 7 dimensions d'un fait, dans l'ordre de DIMS_HEADER.
fn dims_cells(d: &Dims) -> Vec<String> {
    let o = |s: &Option<String>| s.clone().unwrap_or_default();
    vec![o(&d.center_name), o(&d.client_name), o(&d.trainer_name), o(&d.software_name), o(&d.modality), o(&d.group_format), o(&d.os)]
}
// En-têtes des colonnes de dimensions.
const DIMS_HEADER: &str = "Centre;Client;Formateur;Logiciel;Modalité;Format;OS";

/// Export CSV (Excel, séparateur ;) : synthèse par regroupement ou détail d'un jeu de données.
pub async fn export(State(st): State<AppState>, user: CurrentUser, Query(q): Query<StatsQuery>) -> AppResult<impl IntoResponse> {
    user.require(Permission::ManageBusinessData)?;
    let mut conn = st.pool.acquire().await?;
    let (from, to) = period(&q)?;
    let mut out = String::from("\u{feff}");
    let dataset = q.dataset.clone().unwrap_or_else(|| "synthese".into());
    let mut push = |cells: Vec<String>| {
        out.push_str(&cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
    };
    match dataset.as_str() {
        "timesheets" => {
            push(format!("Date;Heures;Facturable;Montant HT;{DIMS_HEADER}").split(';').map(String::from).collect());
            for f in time_facts(&mut conn, &from, &to).await?.iter().filter(|f| keep(&q, &f.d)) {
                let mut c = vec![f.date.clone(), hours(f.minutes), (if f.billable { "oui" } else { "non" }).into(), eur(f.amount)];
                c.extend(dims_cells(&f.d));
                push(c);
            }
        }
        "missions" => {
            push(format!("Référence;Intitulé;Date;Statut;Sessions réalisées;Heures réalisées;Stagiaires;{DIMS_HEADER}").split(';').map(String::from).collect());
            for f in mission_facts(&mut conn, &from, &to).await?.iter().filter(|f| keep(&q, &f.d)) {
                let n = f.participants_csv.split(',').filter(|s| !s.is_empty()).count();
                let mut c = vec![f.reference.clone(), f.title.clone(), f.anchor.clone().unwrap_or_default(), f.status.clone(), f.sessions.to_string(), hours(f.session_minutes), n.to_string()];
                c.extend(dims_cells(&f.d));
                push(c);
            }
        }
        "invoices" => {
            push(format!("Numéro;Date;Type;Statut;Facturé à;Total HT;dont temps;dont frais;dont forfait;{DIMS_HEADER}").split(';').map(String::from).collect());
            for f in invoice_facts(&mut conn, &from, &to).await?.iter().filter(|f| keep(&q, &f.d)) {
                let mut c = vec![f.number.clone().unwrap_or_else(|| f.id.to_string()), f.date.clone(), f.kind.clone(), f.status.clone(), f.partner_name.clone(), eur(f.total_ht), eur(f.ht_time), eur(f.ht_expenses), eur(f.ht_flat)];
                c.extend(dims_cells(&f.d));
                push(c);
            }
        }
        "payments" => {
            push(format!("Date;Montant;Facture;Payé par;Mode;{DIMS_HEADER}").split(';').map(String::from).collect());
            for f in payment_facts(&mut conn, &from, &to).await?.iter().filter(|f| keep(&q, &f.d)) {
                let mut c = vec![f.date.clone(), eur(f.amount), f.number.clone().unwrap_or_default(), f.partner_name.clone(), f.method.clone()];
                c.extend(dims_cells(&f.d));
                push(c);
            }
        }
        "expenses" => {
            push(format!("Date;Libellé;Catégorie;Fournisseur;TTC;{DIMS_HEADER}").split(';').map(String::from).collect());
            for f in expense_facts(&mut conn, &from, &to).await?.iter().filter(|f| keep(&q, &f.d)) {
                let mut c = vec![f.date.clone(), f.label.clone(), f.category.clone(), f.supplier.clone(), eur(f.amount_ttc)];
                c.extend(dims_cells(&f.d));
                push(c);
            }
        }
        _ => {
            let r = report(&mut conn, &q).await?;
            push("Regroupement;Heures travaillées;Heures facturables;Montant des temps HT;Formations;Stagiaires;Sessions réalisées;Heures de session;Factures;Facturé HT;dont temps;dont frais;dont forfait;Encaissé;Dépenses TTC".split(';').map(String::from).collect());
            for x in r.rows.iter().chain(std::iter::once(&r.total)) {
                push(vec![x.label.clone(), hours(x.minutes), hours(x.billable_minutes), eur(x.timesheet_amount), x.missions.to_string(), x.participants.to_string(), x.sessions.to_string(),
                    hours(x.session_minutes), x.invoices.to_string(), eur(x.invoiced_ht), eur(x.invoiced_time), eur(x.invoiced_expenses), eur(x.invoiced_flat), eur(x.paid), eur(x.expenses_ttc)]);
            }
        }
    }
    let name = format!("statistiques-{dataset}-{from}-{to}.csv");
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], out))
}
