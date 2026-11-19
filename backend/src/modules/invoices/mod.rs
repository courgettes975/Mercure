//! Lots 5 à 7 : facturation (routes HTTP) et document imprimable.

// formats structurés Factur-X (CII) et UBL, métadonnées XMP
pub mod einvoice;
// génération du PDF de facture
pub mod pdf;
// règles métier et SQL
pub mod service;

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{auth::CurrentUser, domain::roles::Permission, error::{AppError, AppResult}, modules::settings, state::AppState};
use service::*;

// GET /invoices : liste filtrée avec totaux.
pub async fn list(State(st): State<AppState>, u: CurrentUser, Query(f): Query<Filters>) -> AppResult<Json<ListResponse>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::list(&st.pool, &f).await?))
}

// GET /invoices/{id}
pub async fn get(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    let mut conn = st.pool.acquire().await?;
    Ok(Json(service::load(&mut conn, id).await?))
}

// POST /invoices : brouillon manuel.
pub async fn create(State(st): State<AppState>, u: CurrentUser, Json(i): Json<InvoiceInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::create(&st.pool, u.id, i).await?))
}

// PUT /invoices/{id} : brouillon uniquement.
pub async fn update(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<InvoiceInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::update(&st.pool, u.id, id, i).await?))
}

// DELETE /invoices/{id} : brouillon uniquement.
pub async fn delete(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    u.require(Permission::ManageBusinessData)?;
    service::delete_draft(&st.pool, u.id, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// POST /invoices/{id}/validate : numéro définitif, facture figée.
pub async fn validate(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::validate(&st.pool, u.id, id).await?))
}

#[derive(Deserialize, Default)]
#[serde(default)]
// Motif de l'annulation (repris dans la note de l'avoir).
pub struct CancelInput {
    pub reason: String,
}

// POST /invoices/{id}/cancel : annulation par avoir total.
pub async fn cancel(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(c): Json<CancelInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::credit_note(&st.pool, u.id, id, &c.reason).await?))
}

// POST /invoices/{id}/payments
pub async fn add_payment(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(p): Json<PaymentInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::add_payment(&st.pool, u.id, id, p).await?))
}

// DELETE /payments/{id}
pub async fn delete_payment(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::delete_payment(&st.pool, u.id, id).await?))
}

// POST /invoices/{id}/reminder : trace d'une relance.
pub async fn reminder(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::mark_reminder(&st.pool, u.id, id).await?))
}

// GET /billing/projects/{id} : éléments en attente de facturation d'un projet.
pub async fn billable(State(st): State<AppState>, u: CurrentUser, Path(project_id): Path<i64>) -> AppResult<Json<Billable>> {
    u.require(Permission::ManageBusinessData)?;
    let mut conn = st.pool.acquire().await?;
    Ok(Json(service::billable(&mut conn, project_id).await?))
}

// POST /billing/from-timesheets : facture d'un projet depuis la sélection.
pub async fn from_items(State(st): State<AppState>, u: CurrentUser, Json(f): Json<FromItemsInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::from_items(&st.pool, u.id, f).await?))
}

/// Facture au format PDF.
// GET /invoices/{id}/pdf : PDF (brouillon filigrané si non validée) avec le logo s'il existe.
pub async fn pdf(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>) -> AppResult<impl IntoResponse> {
    u.require(Permission::ManageBusinessData)?;
    let d = {
        let mut conn = st.pool.acquire().await?;
        service::load(&mut conn, id).await?
    };
    let cs = settings::load(&st.pool).await?;
    let logo = match &cs.logo_file {
        Some(f) => tokio::fs::read(st.paths.documents.join(f)).await.ok(),
        None => None,
    };
    // Facture numérotée + facturation électronique active : PDF hybride Factur-X (XML CII embarqué).
    let facturx = cs.einvoicing_enabled && d.row.number.is_some();
    let bytes = if facturx {
        let xml = einvoice::cii(&d, &cs);
        let title = format!("{} {}", if d.row.kind == "AVOIR" { "Avoir" } else { "Facture" }, d.row.number.clone().unwrap_or_default());
        let xmp = einvoice::xmp(&d, &title);
        pdf::render_with(&d, &cs, logo.as_deref(), Some(pdf::Attachment { xml: xml.as_bytes(), xmp: &xmp }))?
    } else {
        pdf::render(&d, &cs, logo.as_deref())?
    };
    let name = d.row.number.clone().unwrap_or_else(|| format!("brouillon-{}", d.row.id));
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (header::CONTENT_DISPOSITION, format!("inline; filename=\"{name}.pdf\"")),
        ],
        bytes,
    ))
}

/// Format demandé pour la facture électronique : "cii" (Factur-X) ou "ubl".
#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct XmlQuery {
    pub format: Option<String>,
}

// GET /invoices/{id}/xml?format=cii|ubl : facture électronique au format structuré, à déposer sur la plateforme agréée.
pub async fn xml(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Query(q): Query<XmlQuery>) -> AppResult<impl IntoResponse> {
    u.require(Permission::ManageBusinessData)?;
    let d = {
        let mut conn = st.pool.acquire().await?;
        service::load(&mut conn, id).await?
    };
    einvoice::ensure_issued(&d)?;
    let cs = settings::load(&st.pool).await?;
    let (body, suffix) = match q.format.as_deref().unwrap_or("cii") {
        "ubl" => (einvoice::ubl(&d, &cs), "ubl"),
        "cii" | "facturx" => (einvoice::cii(&d, &cs), "cii"),
        _ => return Err(AppError::Validation("Format inconnu : cii ou ubl".into())),
    };
    let name = format!("{}-{suffix}.xml", d.row.number.clone().unwrap_or_default());
    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))],
        body,
    ))
}

/// Suivi de transmission saisi depuis l'interface.
#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct EinvoiceInput {
    pub status: String,
    pub reference: String,
    pub message: String,
}

// POST /invoices/{id}/einvoice : met à jour le statut de transmission à la plateforme agréée.
pub async fn einvoice_status(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<EinvoiceInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::set_einvoice_status(&st.pool, u.id, id, &i.status, &i.reference, &i.message).await?))
}

#[derive(serde::Deserialize)]
// Mois demandé "AAAA-MM".
pub struct MonthQuery {
    pub month: String,
}

// GET /billing/monthly : récapitulatif par centre.
pub async fn monthly_report(State(st): State<AppState>, u: CurrentUser, Query(q): Query<MonthQuery>) -> AppResult<Json<service::MonthlyReport>> {
    u.require(Permission::ManageBusinessData)?;
    let mut conn = st.pool.acquire().await?;
    Ok(Json(service::monthly_report(&mut conn, &q.month).await?))
}

// POST /billing/monthly : facture mensuelle d'un centre.
pub async fn monthly_invoice(State(st): State<AppState>, u: CurrentUser, Json(f): Json<service::MonthlyInvoiceInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::monthly_invoice(&st.pool, u.id, f).await?))
}

/// Date d'envoi facultative (aujourd'hui par défaut).
#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct SentInput {
    pub sent_at: Option<String>,
}

// POST /invoices/{id}/sent : marque la facture envoyée au client.
pub async fn mark_sent(State(st): State<AppState>, u: CurrentUser, Path(id): Path<i64>, Json(i): Json<SentInput>) -> AppResult<Json<InvoiceDetail>> {
    u.require(Permission::ManageBusinessData)?;
    Ok(Json(service::mark_sent(&st.pool, u.id, id, i.sent_at).await?))
}

/// Année de l'état des lieux (année en cours par défaut).
#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct YearQuery {
    pub year: Option<i32>,
}

// GET /invoices/overview : état des lieux (brouillons, non envoyées, en attente, en retard, payées, par client).
pub async fn overview(State(st): State<AppState>, u: CurrentUser, Query(q): Query<YearQuery>) -> AppResult<Json<service::Overview>> {
    u.require(Permission::ManageBusinessData)?;
    let year = q.year.unwrap_or_else(|| chrono::Local::now().format("%Y").to_string().parse().unwrap_or(2026));
    let mut conn = st.pool.acquire().await?;
    Ok(Json(service::overview(&mut conn, year).await?))
}
