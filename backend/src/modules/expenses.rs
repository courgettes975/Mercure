//! Gestion des dépenses de l'entreprise : achats, abonnements, déplacements, cotisations...
//! Chaque dépense porte un montant HT / TVA / TTC, une catégorie, un fournisseur, un statut de paiement,
//! un rattachement facultatif à une mission et un justificatif (fichier). Distinct des frais refacturables
//! des missions (`mission_expenses`), qui finissent sur les factures clients.

use axum::{
    extract::{Multipart, Path, Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

use crate::{
    auth::CurrentUser,
    domain::{roles::Permission, validation::normalize_spaces},
    error::{AppError, AppResult},
    modules::audit,
    state::AppState,
};

/// Type d'entité dans le journal d'audit.
pub const ENTITY: &str = "expense";
/// Catégories de dépenses et libellés.
pub const CATEGORIES: [(&str, &str); 13] = [
    ("LOGICIEL", "Logiciels et abonnements"),
    ("MATERIEL", "Matériel informatique"),
    ("FOURNITURES", "Fournitures"),
    ("DEPLACEMENT", "Déplacements"),
    ("REPAS", "Repas"),
    ("HEBERGEMENT", "Hébergement"),
    ("TELECOM", "Téléphone et internet"),
    ("ASSURANCE", "Assurances"),
    ("COTISATIONS", "Cotisations et taxes"),
    ("FORMATION", "Formation personnelle"),
    ("HONORAIRES", "Honoraires (comptable, juridique)"),
    ("MARKETING", "Communication et publicité"),
    ("AUTRE", "Autre"),
];
/// Modes de paiement.
pub const PAYMENT_METHODS: [&str; 5] = ["CARTE", "VIREMENT", "PRELEVEMENT", "CHEQUE", "ESPECES"];
/// Taille maximale d'un justificatif (10 Mo).
const MAX_RECEIPT_BYTES: usize = 10 * 1024 * 1024;

/// Dépense telle que renvoyée à l'interface.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Expense {
    pub id: i64,
    pub date: String,
    pub label: String,
    pub category: String,
    pub supplier_id: Option<i64>,
    pub supplier_name: String,
    /// Nom du fournisseur affiché : contact lié, sinon texte libre.
    pub supplier_display: String,
    pub amount_ht: i64,
    pub vat_amount: i64,
    pub amount_ttc: i64,
    pub payment_method: String,
    pub status: String,
    pub payment_date: Option<String>,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub mission_id: Option<i64>,
    pub deductible: bool,
    pub notes: String,
    pub receipt_name: Option<String>,
    pub receipt_size: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Lecture commune (liste et fiche).
macro_rules! select_expense {
    ($where:expr) => {
        concat!(
            "SELECT e.id, e.date, e.label, e.category, e.supplier_id, e.supplier_name,
                COALESCE(TRIM(s.name || ' ' || s.first_name), e.supplier_name) AS supplier_display,
                e.amount_ht, e.vat_amount, e.amount_ttc, e.payment_method, e.status, e.payment_date,
                e.project_id, p.name AS project_name, m.id AS mission_id, e.deductible, e.notes,
                e.receipt_name, e.receipt_size, e.created_at, e.updated_at
             FROM expenses e LEFT JOIN partners s ON s.id = e.supplier_id
             LEFT JOIN projects p ON p.id = e.project_id LEFT JOIN missions m ON m.project_id = p.id ",
            $where
        )
    };
}

/// Saisie d'une dépense. Le TTC est calculé (HT + TVA) ; `vat_amount` peut être déduit d'un taux côté interface.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ExpenseInput {
    pub date: String,
    pub label: String,
    pub category: String,
    pub supplier_id: Option<i64>,
    pub supplier_name: String,
    pub amount_ht: i64,
    pub vat_amount: i64,
    pub payment_method: String,
    pub status: String,
    pub payment_date: Option<String>,
    pub project_id: Option<i64>,
    pub deductible: Option<bool>,
    pub notes: String,
}

fn err<T>(msg: &str) -> AppResult<T> {
    Err(AppError::Validation(msg.into()))
}

/// Nettoie et contrôle la saisie ; applique les défauts (catégorie AUTRE, carte, payée à la date de la dépense).
fn normalize(mut i: ExpenseInput) -> AppResult<ExpenseInput> {
    i.label = normalize_spaces(&i.label);
    i.supplier_name = normalize_spaces(&i.supplier_name);
    i.notes = i.notes.trim().to_string();
    if i.label.is_empty() {
        return err("Le libellé est obligatoire");
    }
    if chrono::NaiveDate::parse_from_str(&i.date, "%Y-%m-%d").is_err() {
        return err("Date invalide (AAAA-MM-JJ)");
    }
    if i.category.is_empty() {
        i.category = "AUTRE".into();
    }
    if !CATEGORIES.iter().any(|(c, _)| *c == i.category) {
        return err("Catégorie inconnue");
    }
    if i.amount_ht < 0 || i.vat_amount < 0 {
        return err("Les montants doivent être positifs ou nuls");
    }
    if i.amount_ht == 0 && i.vat_amount == 0 {
        return err("Le montant est obligatoire");
    }
    if i.payment_method.is_empty() {
        i.payment_method = "CARTE".into();
    }
    if !PAYMENT_METHODS.contains(&i.payment_method.as_str()) {
        return err("Mode de paiement inconnu");
    }
    if i.status.is_empty() {
        i.status = "PAYEE".into();
    }
    match i.status.as_str() {
        // Payée : date de paiement = date de la dépense si absente.
        "PAYEE" => {
            let d = i.payment_date.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| i.date.clone());
            if chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").is_err() {
                return err("Date de paiement invalide");
            }
            i.payment_date = Some(d);
        }
        "A_PAYER" => i.payment_date = None,
        _ => return err("Statut inconnu"),
    }
    if i.supplier_id.is_some() {
        i.supplier_name = String::new();
    }
    Ok(i)
}

/// Vérifie l'existence du fournisseur et du projet référencés.
async fn check_refs(conn: &mut SqliteConnection, i: &ExpenseInput) -> AppResult<()> {
    if let Some(s) = i.supplier_id {
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM partners WHERE id = ?").bind(s).fetch_optional(&mut *conn).await?;
        if ok.is_none() {
            return err("Fournisseur introuvable");
        }
    }
    if let Some(p) = i.project_id {
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE id = ?").bind(p).fetch_optional(&mut *conn).await?;
        if ok.is_none() {
            return err("Mission ou projet introuvable");
        }
    }
    Ok(())
}

/// Charge une dépense complète.
pub async fn load(conn: &mut SqliteConnection, id: i64) -> AppResult<Expense> {
    sqlx::query_as::<_, Expense>(select_expense!("WHERE e.id = ?"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Dépense introuvable".into()))
}

/// Filtres de la liste (paramètres d'URL).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct Filters {
    pub q: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub project_id: Option<i64>,
    pub supplier_id: Option<i64>,
}

/// Filtres en SQL paramétré.
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &Filters) {
    if let Some(q) = f.q.as_deref().filter(|s| !s.trim().is_empty()) {
        for w in q.split_whitespace() {
            let like = format!("%{}%", w.replace(['%', '_'], ""));
            qb.push(" AND (e.label LIKE ").push_bind(like.clone())
                .push(" OR e.notes LIKE ").push_bind(like.clone())
                .push(" OR e.supplier_name LIKE ").push_bind(like.clone())
                .push(" OR s.search_text LIKE ").push_bind(like).push(")");
        }
    }
    if let Some(d) = f.from.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND e.date >= ").push_bind(d.to_string());
    }
    if let Some(d) = f.to.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND e.date <= ").push_bind(d.to_string());
    }
    if let Some(c) = f.category.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND e.category = ").push_bind(c.to_string());
    }
    if let Some(s) = f.status.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND e.status = ").push_bind(s.to_string());
    }
    if let Some(p) = f.project_id {
        qb.push(" AND e.project_id = ").push_bind(p);
    }
    if let Some(s) = f.supplier_id {
        qb.push(" AND e.supplier_id = ").push_bind(s);
    }
}

/// Total par catégorie (sur l'ensemble filtré).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CategoryTotal {
    pub category: String,
    pub count: i64,
    pub amount_ht: i64,
    pub amount_ttc: i64,
}

/// Total par mois (sur l'ensemble filtré).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MonthTotal {
    pub month: String,
    pub amount_ttc: i64,
}

/// Liste + totaux + ventilations.
#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub items: Vec<Expense>,
    pub total_ht: i64,
    pub total_vat: i64,
    pub total_ttc: i64,
    pub to_pay: i64,
    pub by_category: Vec<CategoryTotal>,
    pub by_month: Vec<MonthTotal>,
    pub truncated: bool,
}

/// GET /depenses : liste filtrée (2000 lignes max), totaux et ventilations par catégorie et par mois.
pub async fn list(State(st): State<AppState>, user: CurrentUser, Query(f): Query<Filters>) -> AppResult<Json<ListResponse>> {
    user.require(Permission::ManageBusinessData)?;
    let mut qb = QueryBuilder::<Sqlite>::new(select_expense!("WHERE 1 = 1"));
    push_filters(&mut qb, &f);
    qb.push(" ORDER BY e.date DESC, e.id DESC LIMIT 2001");
    let mut items: Vec<Expense> = qb.build_query_as().fetch_all(&st.pool).await?;
    let truncated = items.len() > 2000;
    items.truncate(2000);

    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT COALESCE(SUM(e.amount_ht), 0), COALESCE(SUM(e.vat_amount), 0), COALESCE(SUM(e.amount_ttc), 0),
                COALESCE(SUM(CASE WHEN e.status = 'A_PAYER' THEN e.amount_ttc ELSE 0 END), 0)
         FROM expenses e LEFT JOIN partners s ON s.id = e.supplier_id WHERE 1 = 1",
    );
    push_filters(&mut qb, &f);
    let (total_ht, total_vat, total_ttc, to_pay): (i64, i64, i64, i64) = qb.build_query_as().fetch_one(&st.pool).await?;

    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT e.category, COUNT(*) AS count, SUM(e.amount_ht) AS amount_ht, SUM(e.amount_ttc) AS amount_ttc
         FROM expenses e LEFT JOIN partners s ON s.id = e.supplier_id WHERE 1 = 1",
    );
    push_filters(&mut qb, &f);
    qb.push(" GROUP BY e.category ORDER BY amount_ttc DESC");
    let by_category: Vec<CategoryTotal> = qb.build_query_as().fetch_all(&st.pool).await?;

    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT substr(e.date, 1, 7) AS month, SUM(e.amount_ttc) AS amount_ttc
         FROM expenses e LEFT JOIN partners s ON s.id = e.supplier_id WHERE 1 = 1",
    );
    push_filters(&mut qb, &f);
    qb.push(" GROUP BY month ORDER BY month");
    let by_month: Vec<MonthTotal> = qb.build_query_as().fetch_all(&st.pool).await?;

    Ok(Json(ListResponse { items, total_ht, total_vat, total_ttc, to_pay, by_category, by_month, truncated }))
}

/// GET /depenses/{id}
pub async fn get(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Expense>> {
    user.require(Permission::ManageBusinessData)?;
    let mut conn = st.pool.acquire().await?;
    Ok(Json(load(&mut conn, id).await?))
}

/// POST /depenses
pub async fn create(State(st): State<AppState>, user: CurrentUser, Json(i): Json<ExpenseInput>) -> AppResult<Json<Expense>> {
    user.require(Permission::ManageBusinessData)?;
    let i = normalize(i)?;
    let mut tx = st.pool.begin().await?;
    check_refs(&mut tx, &i).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO expenses (date, label, category, supplier_id, supplier_name, amount_ht, vat_amount, amount_ttc, payment_method, status,
            payment_date, project_id, deductible, notes, created_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&i.date).bind(&i.label).bind(&i.category).bind(i.supplier_id).bind(&i.supplier_name)
    .bind(i.amount_ht).bind(i.vat_amount).bind(i.amount_ht + i.vat_amount).bind(&i.payment_method).bind(&i.status)
    .bind(&i.payment_date).bind(i.project_id).bind(i.deductible.unwrap_or(true)).bind(&i.notes).bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    let e = load(&mut tx, id).await?;
    audit::record(&mut tx, Some(user.id), "CREATION", ENTITY, Some(id.to_string()), Some(json!(e))).await?;
    tx.commit().await?;
    Ok(Json(e))
}

/// PUT /depenses/{id}
pub async fn update(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(i): Json<ExpenseInput>) -> AppResult<Json<Expense>> {
    user.require(Permission::ManageBusinessData)?;
    let i = normalize(i)?;
    let mut tx = st.pool.begin().await?;
    let before = load(&mut tx, id).await?;
    check_refs(&mut tx, &i).await?;
    sqlx::query(
        "UPDATE expenses SET date=?, label=?, category=?, supplier_id=?, supplier_name=?, amount_ht=?, vat_amount=?, amount_ttc=?,
            payment_method=?, status=?, payment_date=?, project_id=?, deductible=?, notes=?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(&i.date).bind(&i.label).bind(&i.category).bind(i.supplier_id).bind(&i.supplier_name)
    .bind(i.amount_ht).bind(i.vat_amount).bind(i.amount_ht + i.vat_amount).bind(&i.payment_method).bind(&i.status)
    .bind(&i.payment_date).bind(i.project_id).bind(i.deductible.unwrap_or(before.deductible)).bind(&i.notes).bind(id)
    .execute(&mut *tx)
    .await?;
    let after = load(&mut tx, id).await?;
    audit::record(&mut tx, Some(user.id), "MODIFICATION", ENTITY, Some(id.to_string()), Some(audit::diff(&json!(before), &json!(after)))).await?;
    tx.commit().await?;
    Ok(Json(after))
}

/// POST /depenses/{id}/pay : marque payée (date du jour si absente).
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct PayInput {
    pub payment_date: Option<String>,
}

pub async fn pay(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(p): Json<PayInput>) -> AppResult<Json<Expense>> {
    user.require(Permission::ManageBusinessData)?;
    let d = p.payment_date.filter(|d| !d.is_empty()).unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    if chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").is_err() {
        return err("Date de paiement invalide");
    }
    let mut tx = st.pool.begin().await?;
    load(&mut tx, id).await?;
    sqlx::query("UPDATE expenses SET status = 'PAYEE', payment_date = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
        .bind(&d).bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user.id), "PAIEMENT", ENTITY, Some(id.to_string()), Some(json!({ "payment_date": d }))).await?;
    let e = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(e))
}

/// DELETE /depenses/{id} : supprime la dépense et son justificatif.
pub async fn delete(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<serde_json::Value>> {
    user.require(Permission::ManageBusinessData)?;
    let mut tx = st.pool.begin().await?;
    let before = load(&mut tx, id).await?;
    let storage: Option<String> = sqlx::query_scalar("SELECT receipt_storage FROM expenses WHERE id = ?").bind(id).fetch_one(&mut *tx).await?;
    sqlx::query("DELETE FROM expenses WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user.id), "SUPPRESSION", ENTITY, Some(id.to_string()), Some(json!(before))).await?;
    tx.commit().await?;
    if let Some(s) = storage {
        let _ = tokio::fs::remove_file(st.paths.documents.join(s)).await;
    }
    Ok(Json(json!({ "ok": true })))
}

/// Nom de fichier affiché : sans chemin, caractères dangereux remplacés, longueur bornée.
fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base.chars().map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "justificatif".into() } else { cleaned.chars().take(120).collect() }
}

/// POST /depenses/{id}/receipt : justificatif (PDF, image...) stocké sous documents/depenses/<id>/ ; remplace le précédent.
pub async fn upload_receipt(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, mut mp: Multipart) -> AppResult<Json<Expense>> {
    user.require(Permission::ManageBusinessData)?;
    let mut file: Option<(String, Vec<u8>)> = None;
    while let Some(field) = mp.next_field().await.map_err(|_| AppError::Validation("Envoi du fichier invalide (10 Mo max)".into()))? {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("justificatif").to_string();
            let data = field.bytes().await.map_err(|_| AppError::Validation("Fichier trop volumineux (10 Mo max)".into()))?;
            file = Some((name, data.to_vec()));
        }
    }
    let (name, data) = file.ok_or_else(|| AppError::Validation("Aucun fichier reçu".into()))?;
    if data.is_empty() || data.len() > MAX_RECEIPT_BYTES {
        return err("Fichier vide ou trop volumineux (10 Mo max)");
    }
    let file_name = safe_file_name(&name);
    let mut rnd = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut rnd);
    let storage_name = format!("depenses/{id}/{}_{}", hex::encode(rnd), file_name.replace(' ', "_"));
    let mime = mime_guess::from_path(&file_name).first_or_octet_stream().to_string();

    let mut tx = st.pool.begin().await?;
    load(&mut tx, id).await?;
    let old: Option<String> = sqlx::query_scalar("SELECT receipt_storage FROM expenses WHERE id = ?").bind(id).fetch_one(&mut *tx).await?;
    let path = st.paths.documents.join(&storage_name);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(AppError::internal)?;
    }
    tokio::fs::write(&path, &data).await.map_err(AppError::internal)?;
    sqlx::query("UPDATE expenses SET receipt_name = ?, receipt_storage = ?, receipt_mime = ?, receipt_size = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
        .bind(&file_name).bind(&storage_name).bind(&mime).bind(data.len() as i64).bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, Some(user.id), "JUSTIFICATIF", ENTITY, Some(id.to_string()), Some(json!({ "file": file_name }))).await?;
    let e = load(&mut tx, id).await?;
    if let Err(e) = tx.commit().await {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(e.into());
    }
    // L'ancien justificatif n'est effacé qu'après validation de la transaction.
    if let Some(o) = old {
        let _ = tokio::fs::remove_file(st.paths.documents.join(o)).await;
    }
    Ok(Json(e))
}

/// GET /depenses/{id}/receipt : télécharge le justificatif avec son nom d'origine.
pub async fn download_receipt(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> AppResult<impl IntoResponse> {
    user.require(Permission::ManageBusinessData)?;
    let row: Option<(Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT receipt_name, receipt_storage, receipt_mime FROM expenses WHERE id = ?").bind(id).fetch_optional(&st.pool).await?;
    let (name, storage, mime) = match row {
        Some((Some(n), Some(s), Some(m))) => (n, s, m),
        Some(_) => return Err(AppError::NotFound("Aucun justificatif pour cette dépense".into())),
        None => return Err(AppError::NotFound("Dépense introuvable".into())),
    };
    let bytes = tokio::fs::read(st.paths.documents.join(&storage)).await.map_err(|_| AppError::NotFound("Fichier introuvable sur le disque".into()))?;
    let ascii: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    Ok((
        [(header::CONTENT_TYPE, mime), (header::CONTENT_DISPOSITION, format!("inline; filename=\"{ascii}\""))],
        bytes,
    ))
}

/// Libellé d'une catégorie.
pub fn category_label(code: &str) -> &str {
    CATEGORIES.iter().find(|(c, _)| *c == code).map(|(_, l)| *l).unwrap_or(code)
}

fn cell(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}
fn eur(c: i64) -> String {
    format!("{},{:02}", c / 100, (c % 100).abs())
}

/// GET /depenses/export.csv : mêmes filtres que la liste, CSV Excel (BOM, séparateur ;) pour le comptable.
pub async fn export(State(st): State<AppState>, user: CurrentUser, Query(f): Query<Filters>) -> AppResult<impl IntoResponse> {
    user.require(Permission::ManageBusinessData)?;
    let mut qb = QueryBuilder::<Sqlite>::new(select_expense!("WHERE 1 = 1"));
    push_filters(&mut qb, &f);
    qb.push(" ORDER BY e.date, e.id");
    let items: Vec<Expense> = qb.build_query_as().fetch_all(&st.pool).await?;
    let mut out = String::from("\u{feff}Date;Libellé;Catégorie;Fournisseur;HT;TVA;TTC;Mode;Statut;Date de paiement;Mission;Déductible;Justificatif;Notes\r\n");
    for e in &items {
        let cells = [
            e.date.clone(), e.label.clone(), category_label(&e.category).to_string(), e.supplier_display.clone(),
            eur(e.amount_ht), eur(e.vat_amount), eur(e.amount_ttc), e.payment_method.clone(),
            (if e.status == "PAYEE" { "payée" } else { "à payer" }).to_string(), e.payment_date.clone().unwrap_or_default(),
            e.project_name.clone().unwrap_or_default(), (if e.deductible { "oui" } else { "non" }).to_string(),
            e.receipt_name.clone().unwrap_or_default(), e.notes.clone(),
        ];
        out.push_str(&cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
    }
    Ok((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, "attachment; filename=\"depenses.csv\"".to_string())],
        out,
    ))
}
