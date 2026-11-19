//! Lots 5 et 6 : factures, avoirs, paiements, facturation des temps et frais.
//!
//! Règles :
//! - Tous les montants sont calculés ici (le navigateur n'envoie que quantités, prix et taux).
//! - Le numéro est attribué à la validation, dans une transaction BEGIN IMMEDIATE (pas de doublon ni de trou).
//! - Une facture validée est figée (triggers SQL en plus de ce code) ; correction = avoir.
//! - Un temps ou un frais ne peut être lié qu'à une seule ligne de facture (invoice_line_id).

use chrono::Duration;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::{
    domain::{
        money::Money,
        validation::{normalize_spaces, parse_date, search_key},
    },
    error::{AppError, AppResult},
    modules::{audit, missions, sequence, settings},
};

// Type d'entité dans le journal d'audit.
pub const ENTITY: &str = "invoice";

// Raccourci : erreur de validation.
fn err<T>(m: impl Into<String>) -> AppResult<T> {
    Err(AppError::Validation(m.into()))
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
// Ligne saisie : quantité en millièmes, prix unitaire en centimes, taux de TVA facultatif ; `id` pour une ligne existante.
pub struct LineInput {
    pub id: Option<i64>,
    /// Produit de la liste de référence dont la ligne est issue (facultatif, pour les statistiques).
    pub product_id: Option<i64>,
    pub description: String,
    pub quantity_milli: i64,
    pub unit: String,
    pub unit_price: i64,
    pub vat_rate_bp: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
// En-tête saisi ; dates et conditions facultatives (défauts tirés des paramètres).
pub struct InvoiceInput {
    pub partner_id: i64,
    /// Catégorie d'opération (facture électronique) : SERVICES, BIENS ou MIXTE ; vide = défaut de l'entreprise.
    pub operation_category: String,
    pub project_id: Option<i64>,
    pub invoice_date: Option<String>,
    pub due_date: Option<String>,
    pub payment_terms: Option<String>,
    pub notes: String,
    pub lines: Vec<LineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Ligne enregistrée avec ses totaux calculés ; `linked_items` = temps/frais rattachés ; `source_type` = MANUEL/TEMPS/FRAIS/FORFAIT.
pub struct Line {
    pub id: i64,
    pub position: i64,
    pub description: String,
    pub quantity_milli: i64,
    pub unit: String,
    pub unit_price: i64,
    pub vat_rate_bp: i64,
    pub total_ht: i64,
    pub total_vat: i64,
    pub total_ttc: i64,
    pub source_type: String,
    pub product_id: Option<i64>,
    pub linked_items: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Paiement reçu.
pub struct Payment {
    pub id: i64,
    pub payment_date: String,
    pub amount: i64,
    pub method: String,
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Facture ou avoir tel que lu en base ; `billing_*` = coordonnées figées à la validation ; `amount_due` calculé.
pub struct InvoiceRow {
    pub id: i64,
    pub number: Option<String>,
    pub kind: String,
    pub reversed_invoice_id: Option<i64>,
    pub reversed_number: Option<String>,
    pub partner_id: i64,
    pub partner_name: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub mission_id: Option<i64>,
    pub billing_name: String,
    pub billing_address: String,
    pub billing_vat_number: String,
    pub invoice_date: String,
    pub due_date: String,
    pub currency: String,
    pub status: String,
    pub total_ht: i64,
    pub total_vat: i64,
    pub total_ttc: i64,
    pub amount_paid: i64,
    pub amount_due: i64,
    pub payment_terms: String,
    pub vat_mention: String,
    pub legal_mentions: String,
    pub notes: String,
    pub validated_at: Option<String>,
    pub last_reminder_at: Option<String>,
    /// Date d'envoi au client (marquée à la main : "Marquer envoyée").
    pub sent_at: Option<String>,
    /// Facturation électronique : SIREN client figé, catégorie d'opération, statut de transmission et suivi.
    pub billing_siren: String,
    pub operation_category: String,
    pub einvoice_status: Option<String>,
    pub einvoice_ref: String,
    pub einvoice_message: String,
    pub einvoice_updated_at: Option<String>,
    pub credit_note_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
// Facture complète : en-tête + lignes + paiements.
pub struct InvoiceDetail {
    #[serde(flatten)]
    pub row: InvoiceRow,
    pub lines: Vec<Line>,
    pub payments: Vec<Payment>,
}

// Lecture commune (liste et fiche) avec partenaire, projet, mission et avoir lié.
macro_rules! select_invoice {
    ($tail:literal) => {
        concat!(
            "SELECT i.id, i.number, i.kind, i.reversed_invoice_id, r.number AS reversed_number, i.partner_id,
                TRIM(c.name || ' ' || c.first_name) AS partner_name, i.project_id, p.name AS project_name, m.id AS mission_id,
                i.billing_name, i.billing_address, i.billing_vat_number, i.invoice_date, i.due_date, i.currency, i.status,
                i.total_ht, i.total_vat, i.total_ttc, i.amount_paid,
                CASE WHEN i.kind = 'FACTURE' AND i.status IN ('VALIDEE', 'PAYEE') THEN i.total_ttc - i.amount_paid ELSE 0 END AS amount_due,
                i.payment_terms, i.vat_mention, i.legal_mentions, i.notes, i.validated_at, i.last_reminder_at, i.sent_at,
                i.billing_siren, i.operation_category, i.einvoice_status, i.einvoice_ref, i.einvoice_message, i.einvoice_updated_at,
                (SELECT a.id FROM invoices a WHERE a.reversed_invoice_id = i.id LIMIT 1) AS credit_note_id, i.created_at
             FROM invoices i JOIN partners c ON c.id = i.partner_id LEFT JOIN projects p ON p.id = i.project_id
             LEFT JOIN missions m ON m.project_id = p.id LEFT JOIN invoices r ON r.id = i.reversed_invoice_id ",
            $tail
        )
    };
}

// Charge une facture complète.
pub async fn load(conn: &mut SqliteConnection, id: i64) -> AppResult<InvoiceDetail> {
    let row = sqlx::query_as::<_, InvoiceRow>(select_invoice!("WHERE i.id = ?"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Facture introuvable".into()))?;
    let lines = sqlx::query_as::<_, Line>(
        "SELECT l.id, l.position, l.description, l.quantity_milli, l.unit, l.unit_price, l.vat_rate_bp, l.total_ht, l.total_vat,
            l.total_ttc, l.source_type, l.product_id,
            (SELECT COUNT(*) FROM timesheets t WHERE t.invoice_line_id = l.id)
              + (SELECT COUNT(*) FROM mission_expenses e WHERE e.invoice_line_id = l.id) AS linked_items
         FROM invoice_lines l WHERE l.invoice_id = ? ORDER BY l.position, l.id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let payments = sqlx::query_as::<_, Payment>("SELECT id, payment_date, amount, method, reference FROM payments WHERE invoice_id = ? ORDER BY payment_date, id")
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(InvoiceDetail { row, lines, payments })
}

/// Calcul d'une ligne : HT = quantité x prix, TVA par ligne, TTC = HT + TVA (arrondis au centime).
// HT = quantité x PU (arrondi au centime), TVA = HT x taux, TTC = HT + TVA.
pub fn compute_line(quantity_milli: i64, unit_price: i64, vat_rate_bp: i64) -> (i64, i64, i64) {
    let ht = Money(unit_price).times_milli_qty(quantity_milli);
    let vat = ht.vat(vat_rate_bp);
    (ht.0, vat.0, (ht + vat).0)
}

// Totaux de l'en-tête = somme des lignes.
async fn recompute_totals(conn: &mut SqliteConnection, id: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE invoices SET
            total_ht = COALESCE((SELECT SUM(total_ht) FROM invoice_lines WHERE invoice_id = ?1), 0),
            total_vat = COALESCE((SELECT SUM(total_vat) FROM invoice_lines WHERE invoice_id = ?1), 0),
            total_ttc = COALESCE((SELECT SUM(total_ttc) FROM invoice_lines WHERE invoice_id = ?1), 0),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?1",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(())
}

// Date du jour AAAA-MM-JJ.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// Date + n jours (échéance).
fn add_days(date: &str, days: i64) -> String {
    parse_date(date).map(|d| (d + Duration::days(days)).format("%Y-%m-%d").to_string()).unwrap_or_else(|| date.to_string())
}

/// Taux de TVA d'une ligne selon le régime de l'entreprise.
fn line_vat(cs: &settings::CompanySettings, requested: Option<i64>) -> AppResult<i64> {
    // Taux demandé sur la ligne, sinon taux par défaut de l'entreprise (0 en franchise en base).
    // Un taux explicite est accepté même en franchise : l'utilisateur décide d'appliquer la TVA sur cette facture.
    let r = requested.unwrap_or(cs.default_vat_rate_bp);
    if !(0..=10_000).contains(&r) {
        return err("Taux de TVA invalide");
    }
    Ok(r)
}

fn normalize_line(l: &mut LineInput) -> AppResult<()> {
    l.description = l.description.trim().to_string();
    l.unit = normalize_spaces(&l.unit);
    if l.description.is_empty() {
        return err("Chaque ligne doit avoir une description");
    }
    if l.quantity_milli <= 0 {
        return err("La quantité doit être supérieure à 0");
    }
    if l.unit_price < 0 {
        return err("Le prix unitaire doit être positif ou nul");
    }
    Ok(())
}

// Les temps et frais rattachés à une ligne redeviennent facturables (ligne supprimée ou avoir).
async fn release_line_items(conn: &mut SqliteConnection, line_id: i64) -> AppResult<()> {
    sqlx::query("UPDATE timesheets SET invoice_line_id = NULL, invoiced_at = NULL WHERE invoice_line_id = ?").bind(line_id).execute(&mut *conn).await?;
    sqlx::query("UPDATE mission_expenses SET invoice_line_id = NULL, invoiced_at = NULL WHERE invoice_line_id = ?").bind(line_id).execute(&mut *conn).await?;
    Ok(())
}

// Insère une ligne avec ses totaux calculés côté serveur.
async fn insert_line(conn: &mut SqliteConnection, invoice_id: i64, pos: i64, l: &LineInput, vat: i64, source: &str) -> AppResult<i64> {
    let (ht, tva, ttc) = compute_line(l.quantity_milli, l.unit_price, vat);
    Ok(sqlx::query_scalar(
        "INSERT INTO invoice_lines (invoice_id, position, description, quantity_milli, unit, unit_price, vat_rate_bp, total_ht, total_vat, total_ttc, source_type, product_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(invoice_id).bind(pos).bind(&l.description).bind(l.quantity_milli).bind(&l.unit).bind(l.unit_price).bind(vat)
    .bind(ht).bind(tva).bind(ttc).bind(source).bind(l.product_id)
    .fetch_one(conn)
    .await?)
}

// Dates par défaut et contrôles d'en-tête : client existant, projet appartenant au client, échéance >= date.
async fn check_header(conn: &mut SqliteConnection, i: &mut InvoiceInput, cs: &settings::CompanySettings) -> AppResult<()> {
    let date = i.invoice_date.clone().filter(|d| !d.is_empty()).unwrap_or_else(today);
    if parse_date(&date).is_none() {
        return err("Date de facture invalide");
    }
    let due = i.due_date.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| add_days(&date, cs.default_due_days));
    if parse_date(&due).is_none() {
        return err("Date d'échéance invalide");
    }
    if due < date {
        return err("L'échéance ne peut pas précéder la date de facture");
    }
    i.invoice_date = Some(date);
    if i.operation_category.is_empty() {
        i.operation_category = cs.default_operation_category.clone();
    }
    if !["SERVICES", "BIENS", "MIXTE"].contains(&i.operation_category.as_str()) {
        return err("Catégorie d'opération inconnue (SERVICES, BIENS ou MIXTE)");
    }
    i.due_date = Some(due);
    let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM partners WHERE id = ?").bind(i.partner_id).fetch_optional(&mut *conn).await?;
    if ok.is_none() {
        return err("Client introuvable");
    }
    if let Some(p) = i.project_id {
        let client: Option<i64> = sqlx::query_scalar("SELECT client_id FROM projects WHERE id = ?").bind(p).fetch_optional(&mut *conn).await?;
        match client {
            None => return err("Projet introuvable"),
            Some(c) if c != i.partner_id => return err("Le projet appartient à un autre client"),
            _ => {}
        }
    }
    Ok(())
}

// Création d'un brouillon manuel avec ses lignes.
pub async fn create(pool: &SqlitePool, user_id: i64, mut i: InvoiceInput) -> AppResult<InvoiceDetail> {
    let cs = settings::load(pool).await?;
    for l in i.lines.iter_mut() {
        normalize_line(l)?;
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_header(&mut tx, &mut i, &cs).await?;
    let id = insert_draft(&mut tx, user_id, &i, &cs).await?;
    for (pos, l) in i.lines.iter().enumerate() {
        insert_line(&mut tx, id, pos as i64, l, line_vat(&cs, l.vat_rate_bp)?, "MANUEL").await?;
    }
    recompute_totals(&mut tx, id).await?;
    let d = load(&mut tx, id).await?;
    audit::record(&mut tx, Some(user_id), "CREATION", ENTITY, Some(id.to_string()), Some(json!({ "client": d.row.partner_name, "total_ttc": d.row.total_ttc }))).await?;
    tx.commit().await?;
    Ok(d)
}

// Insère l'en-tête d'un brouillon (statut BROUILLON, pas de numéro).
async fn insert_draft(conn: &mut SqliteConnection, user_id: i64, i: &InvoiceInput, cs: &settings::CompanySettings) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO invoices (partner_id, project_id, invoice_date, due_date, payment_terms, notes, created_by, operation_category) VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(i.partner_id)
    .bind(i.project_id)
    .bind(i.invoice_date.as_deref().unwrap_or_default())
    .bind(i.due_date.as_deref().unwrap_or_default())
    .bind(i.payment_terms.clone().unwrap_or_else(|| cs.payment_terms.clone()))
    .bind(i.notes.trim())
    .bind(user_id)
    .bind(&i.operation_category)
    .fetch_one(conn)
    .await?)
}

// Charge la facture et refuse toute modification si elle n'est plus en brouillon.
async fn require_draft(conn: &mut SqliteConnection, id: i64) -> AppResult<InvoiceDetail> {
    let d = load(conn, id).await?;
    if d.row.status != "BROUILLON" {
        return err("Facture validée : elle ne peut plus être modifiée. Établissez un avoir pour la corriger.");
    }
    Ok(d)
}

// Modification d'un brouillon : en-tête, puis lignes mises à jour / ajoutées / supprimées.
pub async fn update(pool: &SqlitePool, user_id: i64, id: i64, mut i: InvoiceInput) -> AppResult<InvoiceDetail> {
    let cs = settings::load(pool).await?;
    for l in i.lines.iter_mut() {
        normalize_line(l)?;
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = require_draft(&mut tx, id).await?;
    if before.row.project_id.is_some() && i.project_id != before.row.project_id && before.lines.iter().any(|l| l.linked_items > 0) {
        return err("Cette facture contient des temps ou frais liés : son projet ne peut pas changer");
    }
    if i.partner_id != before.row.partner_id && before.lines.iter().any(|l| l.linked_items > 0) {
        return err("Cette facture contient des temps ou frais liés : son client ne peut pas changer");
    }
    check_header(&mut tx, &mut i, &cs).await?;
    sqlx::query("UPDATE invoices SET partner_id=?, project_id=?, invoice_date=?, due_date=?, payment_terms=?, notes=?, operation_category=? WHERE id = ?")
        .bind(i.partner_id).bind(i.project_id).bind(&i.invoice_date).bind(&i.due_date)
        .bind(i.payment_terms.clone().unwrap_or(before.row.payment_terms.clone())).bind(i.notes.trim()).bind(&i.operation_category).bind(id)
        .execute(&mut *tx)
        .await?;
    // Lignes : mises à jour, ajoutées ou supprimées (les temps/frais d'une ligne supprimée redeviennent facturables).
    let kept: Vec<i64> = i.lines.iter().filter_map(|l| l.id).collect();
    for old in &before.lines {
        if !kept.contains(&old.id) {
            release_line_items(&mut tx, old.id).await?;
            sqlx::query("DELETE FROM invoice_lines WHERE id = ?").bind(old.id).execute(&mut *tx).await?;
        }
    }
    for (pos, l) in i.lines.iter().enumerate() {
        let vat = line_vat(&cs, l.vat_rate_bp)?;
        match l.id {
            Some(lid) => {
                if !before.lines.iter().any(|o| o.id == lid) {
                    return err("Ligne inconnue");
                }
                let (ht, tva, ttc) = compute_line(l.quantity_milli, l.unit_price, vat);
                sqlx::query(
                    "UPDATE invoice_lines SET position=?, description=?, quantity_milli=?, unit=?, unit_price=?, vat_rate_bp=?, total_ht=?, total_vat=?, total_ttc=?, product_id=? WHERE id = ?",
                )
                .bind(pos as i64).bind(&l.description).bind(l.quantity_milli).bind(&l.unit).bind(l.unit_price).bind(vat)
                .bind(ht).bind(tva).bind(ttc).bind(l.product_id).bind(lid)
                .execute(&mut *tx)
                .await?;
            }
            None => {
                insert_line(&mut tx, id, pos as i64, l, vat, "MANUEL").await?;
            }
        }
    }
    recompute_totals(&mut tx, id).await?;
    let after = load(&mut tx, id).await?;
    audit::record(&mut tx, Some(user_id), "MODIFICATION", ENTITY, Some(id.to_string()),
        Some(audit::diff(&json!({ "total_ttc": before.row.total_ttc, "invoice_date": before.row.invoice_date, "lines": before.lines.len() }),
                         &json!({ "total_ttc": after.row.total_ttc, "invoice_date": after.row.invoice_date, "lines": after.lines.len() })))).await?;
    tx.commit().await?;
    Ok(after)
}

// Suppression d'un brouillon ; ses temps et frais redeviennent facturables.
pub async fn delete_draft(pool: &SqlitePool, user_id: i64, id: i64) -> AppResult<()> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let d = require_draft(&mut tx, id).await?;
    for l in &d.lines {
        release_line_items(&mut tx, l.id).await?;
    }
    sqlx::query("DELETE FROM invoices WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "SUPPRESSION_BROUILLON", ENTITY, Some(id.to_string()), Some(json!({ "client": d.row.partner_name, "total_ttc": d.row.total_ttc }))).await?;
    tx.commit().await?;
    Ok(())
}

// Adresse de facturation sur plusieurs lignes (pays omis si France).
fn billing_address(p: &(String, String, String, String, String, String, String, Option<String>)) -> String {
    let (_, street, street2, zip, city, country, _, _) = p;
    let mut lines: Vec<String> = vec![street.clone(), street2.clone(), format!("{zip} {city}").trim().to_string()];
    if !country.is_empty() && country != "France" {
        lines.push(country.clone());
    }
    lines.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// Valide une facture ou un avoir : numéro définitif, données figées.
pub async fn validate(pool: &SqlitePool, user_id: i64, id: i64) -> AppResult<InvoiceDetail> {
    let cs = settings::load(pool).await?;
    if cs.legal_name.trim().is_empty() {
        return err("Renseignez d'abord la raison sociale dans Paramètres > Entreprise");
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    validate_in_tx(&mut tx, user_id, id, &cs).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

// Cœur de la validation : contrôles, chronologie, coordonnées figées, numéro de séquence, statut mission.
async fn validate_in_tx(conn: &mut SqliteConnection, user_id: i64, id: i64, cs: &settings::CompanySettings) -> AppResult<String> {
    let d = require_draft(conn, id).await?;
    if d.lines.is_empty() {
        return err("La facture ne contient aucune ligne");
    }
    if d.row.total_ttc <= 0 {
        return err("Le total de la facture doit être supérieur à 0");
    }
    let year: i32 = d.row.invoice_date[..4].parse().map_err(|_| AppError::Validation("Date de facture invalide".into()))?;
    let prefix = if d.row.kind == "AVOIR" { "AV".to_string() } else { cs.invoice_prefix.clone() };
    // Chronologie : pas de date antérieure à la dernière facture validée de la même série.
    let last: Option<String> = sqlx::query_scalar("SELECT MAX(invoice_date) FROM invoices WHERE kind = ? AND number IS NOT NULL")
        .bind(&d.row.kind)
        .fetch_one(&mut *conn)
        .await?;
    if let Some(last) = last
        && d.row.invoice_date < last {
            return err(format!("La date ({}) est antérieure à la dernière facture validée ({last}) : les numéros doivent suivre l'ordre chronologique", d.row.invoice_date));
        }
    // Coordonnées du client au moment de la validation, copiées dans la facture (une modification ultérieure
    // du contact ne change pas les factures émises).
    let partner: (String, String, String, String, String, String, String, Option<String>) = sqlx::query_as(
        "SELECT CASE WHEN p.kind = 'PARTICULIER' THEN TRIM(p.name || ' ' || p.first_name) ELSE p.name END,
            p.street, p.street2, p.zip, p.city, p.country, p.vat_number, pp.name
         FROM partners p LEFT JOIN partners pp ON pp.id = p.parent_id WHERE p.id = ?",
    )
    .bind(d.row.partner_id)
    .fetch_one(&mut *conn)
    .await?;
    let billing_name = match &partner.7 {
        Some(company) => format!("{company}\n{}", partner.0),
        None => partner.0.clone(),
    };
    // Facturation électronique : SIREN du client (le sien, sinon celui de sa société) et statut initial.
    let (kind, siren, siret, parent_siren, parent_siret): (String, String, String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT p.kind, p.siren, p.siret, pp.siren, pp.siret FROM partners p LEFT JOIN partners pp ON pp.id = p.parent_id WHERE p.id = ?",
    )
    .bind(d.row.partner_id)
    .fetch_one(&mut *conn)
    .await?;
    let pick = |a: &str, b: &str| if !a.is_empty() { a.to_string() } else { b.chars().take(9).collect() };
    let mut billing_siren = pick(&siren, &siret);
    if billing_siren.is_empty() {
        billing_siren = pick(parent_siren.as_deref().unwrap_or(""), parent_siret.as_deref().unwrap_or(""));
    }
    // Une personne rattachée à une société est facturée pour le compte de la société (B2B) ; un particulier seul est B2C.
    let b2b = kind != "PARTICULIER" || partner.7.is_some();
    let einvoice_status: Option<&str> = if !cs.einvoicing_enabled {
        None
    } else if b2b {
        if billing_siren.is_empty() {
            return err("Facturation électronique : le SIREN du client est obligatoire (fiche contact, ou celle de sa société)");
        }
        Some("A_TRANSMETTRE")
    } else {
        Some("HORS_CHAMP") // client particulier : hors e-invoicing, relève du e-reporting
    };
    // Numéro définitif : FAC-AAAA-NNNNNN (ou AV- pour un avoir), sans trou ni doublon.
    let n = sequence::next_value(conn, &format!("{prefix}-{year}")).await?;
    let number = sequence::format_reference(&prefix, year, n, cs.invoice_number_width as usize);
    // Mention d'exonération imprimée seulement si l'entreprise est en franchise ET que la facture ne porte aucune TVA.
    let vat_mention = if cs.vat_regime == "FRANCHISE" && d.row.total_vat == 0 { cs.vat_exemption_mention.clone() } else { String::new() };
    sqlx::query(
        "UPDATE invoices SET number = ?, status = 'VALIDEE', billing_name = ?, billing_address = ?, billing_vat_number = ?,
            vat_mention = ?, legal_mentions = ?, validated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), validated_by = ?,
            billing_siren = ?, einvoice_status = ?, einvoice_updated_at = CASE WHEN ? IS NULL THEN NULL ELSE strftime('%Y-%m-%dT%H:%M:%SZ', 'now') END
         WHERE id = ?",
    )
    .bind(&number).bind(&billing_name).bind(billing_address(&partner)).bind(&partner.6)
    .bind(vat_mention).bind(&cs.legal_mentions).bind(user_id)
    .bind(&billing_siren).bind(einvoice_status).bind(einvoice_status).bind(id)
    .execute(&mut *conn)
    .await?;
    audit::record(conn, Some(user_id), "VALIDATION", ENTITY, Some(id.to_string()), Some(json!({ "number": number, "total_ttc": d.row.total_ttc }))).await?;
    // Mission entièrement facturée -> statut FACTUREE.
    if d.row.kind == "FACTURE"
        && let Some(mid) = d.row.mission_id {
            let detail = missions::service::load(conn, mid).await?;
            // Facturée seulement si tout est réalisé (aucune session prévue) et que plus rien n'est à facturer.
            let pending_sessions = detail.sessions.iter().any(|s| s.status == "PREVUE");
            if detail.totals.uninvoiced_amount == 0 && !pending_sessions && detail.record.status != "ANNULEE" {
                sqlx::query("UPDATE missions SET status = 'FACTUREE' WHERE id = ?").bind(mid).execute(&mut *conn).await?;
                sqlx::query("UPDATE projects SET status = 'TERMINE' WHERE id = ?").bind(detail.record.project_id).execute(&mut *conn).await?;
                audit::record(conn, Some(user_id), "CHANGEMENT_STATUT", missions::service::ENTITY, Some(mid.to_string()),
                    Some(json!({ "status": { "old": detail.record.status, "new": "FACTUREE" }, "invoice": number }))).await?;
            }
        }
    Ok(number)
}

#[derive(Debug, Deserialize)]
// Paiement saisi (date par défaut : aujourd'hui, mode : virement).
pub struct PaymentInput {
    pub payment_date: Option<String>,
    pub amount: i64,
    pub method: Option<String>,
    pub reference: Option<String>,
}

// Enregistre un paiement partiel ou total ; la facture passe PAYEE quand le reste dû tombe à zéro.
pub async fn add_payment(pool: &SqlitePool, user_id: i64, id: i64, p: PaymentInput) -> AppResult<InvoiceDetail> {
    let date = p.payment_date.filter(|d| !d.is_empty()).unwrap_or_else(today);
    if parse_date(&date).is_none() {
        return err("Date de paiement invalide");
    }
    let method = p.method.unwrap_or_else(|| "VIREMENT".into());
    if !["VIREMENT", "CHEQUE", "ESPECES", "CARTE", "PRELEVEMENT", "AUTRE"].contains(&method.as_str()) {
        return err("Mode de paiement inconnu");
    }
    if p.amount <= 0 {
        return err("Le montant doit être supérieur à 0");
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let d = load(&mut tx, id).await?;
    if d.row.kind != "FACTURE" || d.row.status != "VALIDEE" {
        return err("Un paiement ne peut être enregistré que sur une facture validée non soldée");
    }
    if p.amount > d.row.amount_due {
        return err(format!("Le paiement dépasse le reste à payer ({})", Money(d.row.amount_due).format_fr()));
    }
    sqlx::query("INSERT INTO payments (invoice_id, payment_date, amount, method, reference, created_by) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(id).bind(&date).bind(p.amount).bind(&method).bind(p.reference.unwrap_or_default().trim()).bind(user_id)
        .execute(&mut *tx)
        .await?;
    refresh_paid(&mut tx, id).await?;
    audit::record(&mut tx, Some(user_id), "PAIEMENT", ENTITY, Some(id.to_string()), Some(json!({ "amount": p.amount, "date": date, "method": method }))).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

// Recalcule le montant payé et le statut à partir des paiements.
async fn refresh_paid(conn: &mut SqliteConnection, id: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE invoices SET amount_paid = COALESCE((SELECT SUM(amount) FROM payments WHERE invoice_id = ?1), 0),
            status = CASE WHEN COALESCE((SELECT SUM(amount) FROM payments WHERE invoice_id = ?1), 0) >= total_ttc THEN 'PAYEE' ELSE 'VALIDEE' END
         WHERE id = ?1 AND status IN ('VALIDEE', 'PAYEE')",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(())
}

// Supprime un paiement (sauf facture annulée) et recalcule le statut.
pub async fn delete_payment(pool: &SqlitePool, user_id: i64, payment_id: i64) -> AppResult<InvoiceDetail> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let row: Option<(i64, i64)> = sqlx::query_as("SELECT invoice_id, amount FROM payments WHERE id = ?").bind(payment_id).fetch_optional(&mut *tx).await?;
    let (invoice_id, amount) = row.ok_or_else(|| AppError::NotFound("Paiement introuvable".into()))?;
    let d = load(&mut tx, invoice_id).await?;
    if d.row.status == "ANNULEE" {
        return err("Facture annulée : ses paiements ne peuvent plus changer");
    }
    sqlx::query("DELETE FROM payments WHERE id = ?").bind(payment_id).execute(&mut *tx).await?;
    refresh_paid(&mut tx, invoice_id).await?;
    audit::record(&mut tx, Some(user_id), "SUPPRESSION_PAIEMENT", ENTITY, Some(invoice_id.to_string()), Some(json!({ "amount": amount }))).await?;
    let d = load(&mut tx, invoice_id).await?;
    tx.commit().await?;
    Ok(d)
}

/// Annule une facture validée par un avoir total (validé immédiatement).
/// Les temps et frais facturés redeviennent facturables.
pub async fn credit_note(pool: &SqlitePool, user_id: i64, id: i64, reason: &str) -> AppResult<InvoiceDetail> {
    let cs = settings::load(pool).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let d = load(&mut tx, id).await?;
    if d.row.kind != "FACTURE" || d.row.status != "VALIDEE" {
        return err("Seule une facture validée et non payée peut être annulée par un avoir");
    }
    if d.row.amount_paid > 0 {
        return err("Cette facture a des paiements : supprimez-les (ou gérez le remboursement) avant l'avoir");
    }
    let date = std::cmp::max(today(), d.row.invoice_date.clone());
    let avoir_id: i64 = sqlx::query_scalar(
        "INSERT INTO invoices (kind, reversed_invoice_id, partner_id, project_id, invoice_date, due_date, payment_terms, notes, created_by, operation_category)
         VALUES ('AVOIR', ?, ?, ?, ?, ?, '', ?, ?, ?) RETURNING id",
    )
    .bind(id).bind(d.row.partner_id).bind(d.row.project_id).bind(&date).bind(&date)
    .bind(format!("Avoir sur facture {}{}", d.row.number.clone().unwrap_or_default(), if reason.trim().is_empty() { String::new() } else { format!(" : {}", reason.trim()) }))
    .bind(user_id)
    .bind(&d.row.operation_category)
    .fetch_one(&mut *tx)
    .await?;
    for l in &d.lines {
        let input = LineInput { id: None, product_id: l.product_id, description: l.description.clone(), quantity_milli: l.quantity_milli, unit: l.unit.clone(), unit_price: l.unit_price, vat_rate_bp: Some(l.vat_rate_bp) };
        insert_line(&mut tx, avoir_id, l.position, &input, l.vat_rate_bp, &l.source_type).await?;
        release_line_items(&mut tx, l.id).await?;
    }
    recompute_totals(&mut tx, avoir_id).await?;
    validate_in_tx(&mut tx, user_id, avoir_id, &cs).await?;
    sqlx::query("UPDATE invoices SET status = 'ANNULEE' WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "ANNULATION", ENTITY, Some(id.to_string()), Some(json!({ "avoir": avoir_id, "motif": reason }))).await?;
    if let Some(mid) = d.row.mission_id {
        sqlx::query("UPDATE missions SET status = 'A_FACTURER' WHERE id = ? AND status = 'FACTUREE'").bind(mid).execute(&mut *tx).await?;
    }
    let out = load(&mut tx, avoir_id).await?;
    tx.commit().await?;
    Ok(out)
}

// Note la date de la dernière relance d'une facture impayée.
pub async fn mark_reminder(pool: &SqlitePool, user_id: i64, id: i64) -> AppResult<InvoiceDetail> {
    let mut tx = pool.begin().await?;
    let d = load(&mut tx, id).await?;
    if d.row.status != "VALIDEE" {
        return err("Seule une facture validée non soldée peut être relancée");
    }
    sqlx::query("UPDATE invoices SET last_reminder_at = ? WHERE id = ?").bind(today()).bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "RELANCE", ENTITY, Some(id.to_string()), None).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

/// Marque la facture envoyée au client (date du jour par défaut) ; brouillons exclus.
pub async fn mark_sent(pool: &SqlitePool, user_id: i64, id: i64, date: Option<String>) -> AppResult<InvoiceDetail> {
    let mut tx = pool.begin().await?;
    let d = load(&mut tx, id).await?;
    if d.row.status == "BROUILLON" {
        return err("Validez la facture avant de la marquer envoyée");
    }
    let date = date.filter(|d| !d.is_empty()).unwrap_or_else(today);
    if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return err("Date d'envoi invalide");
    }
    sqlx::query("UPDATE invoices SET sent_at = ? WHERE id = ?").bind(&date).bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "ENVOI", ENTITY, Some(id.to_string()), Some(json!({ "sent_at": date }))).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

/// Statuts de transmission possibles d'une facture électronique.
pub const EINVOICE_STATUSES: [&str; 5] = ["A_TRANSMETTRE", "TRANSMISE", "ACCEPTEE", "REFUSEE", "HORS_CHAMP"];

/// Met à jour le suivi de transmission (plateforme agréée) : statut, référence, message. Brouillons exclus.
pub async fn set_einvoice_status(pool: &SqlitePool, user_id: i64, id: i64, status: &str, reference: &str, message: &str) -> AppResult<InvoiceDetail> {
    if !EINVOICE_STATUSES.contains(&status) {
        return err("Statut de transmission inconnu");
    }
    let mut tx = pool.begin().await?;
    let d = load(&mut tx, id).await?;
    if d.row.status == "BROUILLON" {
        return err("Validez la facture avant de suivre sa transmission");
    }
    sqlx::query("UPDATE invoices SET einvoice_status = ?, einvoice_ref = ?, einvoice_message = ?, einvoice_updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
        .bind(status).bind(reference.trim()).bind(message.trim()).bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, Some(user_id), "TRANSMISSION", ENTITY, Some(id.to_string()), Some(json!({ "einvoice_status": status, "ref": reference.trim(), "message": message.trim() }))).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

// ---------- État des lieux ----------

/// Un compteur de l'état des lieux : nombre de factures et montant.
#[derive(Debug, Serialize, Default, sqlx::FromRow)]
pub struct Bucket {
    pub count: i64,
    pub amount: i64,
}

/// Reste dû par client (factures validées non soldées).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ClientDue {
    pub partner_id: i64,
    pub partner_name: String,
    pub count: i64,
    pub amount_due: i64,
    pub overdue_due: i64,
    pub oldest_due_date: Option<String>,
}

/// Facture en attente listée dans l'état des lieux (les plus urgentes).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PendingInvoice {
    pub id: i64,
    pub number: Option<String>,
    pub partner_id: i64,
    pub partner_name: String,
    pub invoice_date: String,
    pub due_date: String,
    pub sent_at: Option<String>,
    pub last_reminder_at: Option<String>,
    pub total_ttc: i64,
    pub amount_due: i64,
    pub days_late: i64,
}

/// État des lieux de la facturation.
#[derive(Debug, Serialize)]
pub struct Overview {
    pub year: i32,
    pub today: String,
    /// Brouillons (non numérotés).
    pub drafts: Bucket,
    /// Validées non envoyées (reste dû).
    pub not_sent: Bucket,
    /// Envoyées, dans les délais (reste dû).
    pub sent_waiting: Bucket,
    /// En retard de règlement (reste dû), tous états d'envoi.
    pub overdue: Bucket,
    /// Retard par ancienneté : 1-30, 31-60, 61-90, plus de 90 jours.
    pub overdue_1_30: Bucket,
    pub overdue_31_60: Bucket,
    pub overdue_61_90: Bucket,
    pub overdue_90_plus: Bucket,
    /// Payées dans l'année (TTC encaissé), avoirs de l'année, factures émises dans l'année (HT, avoirs déduits).
    pub paid_year: Bucket,
    pub credit_notes_year: Bucket,
    pub issued_year: Bucket,
    /// Encaissements de l'année et du mois (paiements par date de paiement).
    pub collected_year: i64,
    pub collected_month: i64,
    /// Délai moyen de paiement (jours entre date de facture et dernier paiement) sur l'année.
    pub avg_payment_days: Option<i64>,
    pub by_client: Vec<ClientDue>,
    pub pending: Vec<PendingInvoice>,
    /// Facturation électronique : factures de l'année par statut de transmission (si activée).
    pub einvoicing_enabled: bool,
    pub einvoice_to_send: Bucket,
    pub einvoice_sent: Bucket,
    pub einvoice_accepted: Bucket,
    pub einvoice_refused: Bucket,
    pub einvoice_out_of_scope: Bucket,
    /// Factures validées de l'année sans statut (émises avant l'activation).
    pub einvoice_untracked: Bucket,
}

/// Calcule l'état des lieux : factures en attente par état d'envoi et retard, encaissements, reste dû par client.
pub async fn overview(conn: &mut SqliteConnection, year: i32) -> AppResult<Overview> {
    let today = today();
    let (y0, y1) = (format!("{year}-01-01"), format!("{year}-12-31"));
    let month_start = format!("{}-01", &today[0..7]);
    // Reste dû = TTC - payé, sur les factures (hors avoirs) validées non soldées.
    const PENDING: &str = "FROM invoices i WHERE i.kind = 'FACTURE' AND i.status = 'VALIDEE'";
    async fn b(conn: &mut SqliteConnection, sql: &str, binds: &[&str]) -> AppResult<Bucket> {
        let mut q = sqlx::query_as::<_, Bucket>(sqlx::AssertSqlSafe(sql.to_string()));
        for v in binds {
            q = q.bind(v.to_string());
        }
        Ok(q.fetch_one(&mut *conn).await?)
    }
    let drafts = b(conn, "SELECT COUNT(*) AS count, COALESCE(SUM(total_ttc), 0) AS amount FROM invoices WHERE status = 'BROUILLON'", &[]).await?;
    let not_sent = b(conn, &format!("SELECT COUNT(*) AS count, COALESCE(SUM(i.total_ttc - i.amount_paid), 0) AS amount {PENDING} AND i.sent_at IS NULL AND i.due_date >= ?"), &[&today]).await?;
    let sent_waiting = b(conn, &format!("SELECT COUNT(*) AS count, COALESCE(SUM(i.total_ttc - i.amount_paid), 0) AS amount {PENDING} AND i.sent_at IS NOT NULL AND i.due_date >= ?"), &[&today]).await?;
    let overdue = b(conn, &format!("SELECT COUNT(*) AS count, COALESCE(SUM(i.total_ttc - i.amount_paid), 0) AS amount {PENDING} AND i.due_date < ?"), &[&today]).await?;
    let aging = |lo: i64, hi: Option<i64>| {
        let mut sql = format!("SELECT COUNT(*) AS count, COALESCE(SUM(i.total_ttc - i.amount_paid), 0) AS amount {PENDING} AND julianday(?) - julianday(i.due_date) >= {lo}");
        if let Some(h) = hi {
            sql.push_str(&format!(" AND julianday(?) - julianday(i.due_date) <= {h}"));
        }
        sql
    };
    let overdue_1_30 = b(conn, &aging(1, Some(30)), &[&today, &today]).await?;
    let overdue_31_60 = b(conn, &aging(31, Some(60)), &[&today, &today]).await?;
    let overdue_61_90 = b(conn, &aging(61, Some(90)), &[&today, &today]).await?;
    let overdue_90_plus = b(conn, &aging(91, None), &[&today]).await?;
    let paid_year = b(conn, "SELECT COUNT(*) AS count, COALESCE(SUM(total_ttc), 0) AS amount FROM invoices WHERE kind = 'FACTURE' AND status = 'PAYEE' AND invoice_date BETWEEN ? AND ?", &[&y0, &y1]).await?;
    let credit_notes_year = b(conn, "SELECT COUNT(*) AS count, COALESCE(SUM(total_ttc), 0) AS amount FROM invoices WHERE kind = 'AVOIR' AND number IS NOT NULL AND invoice_date BETWEEN ? AND ?", &[&y0, &y1]).await?;
    let issued_year = b(conn, "SELECT COUNT(*) AS count, COALESCE(SUM(CASE WHEN kind = 'AVOIR' THEN -total_ht ELSE total_ht END), 0) AS amount FROM invoices WHERE number IS NOT NULL AND status <> 'ANNULEE' AND invoice_date BETWEEN ? AND ?", &[&y0, &y1]).await?;
    let collected = |from: &str, to: &str| {
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(CASE WHEN i.kind = 'AVOIR' THEN -p.amount ELSE p.amount END), 0) FROM payments p JOIN invoices i ON i.id = p.invoice_id WHERE p.payment_date BETWEEN ? AND ?")
            .bind(from.to_string()).bind(to.to_string())
    };
    let collected_year = collected(&y0, &y1).fetch_one(&mut *conn).await?;
    let collected_month = collected(&month_start, &today).fetch_one(&mut *conn).await?;
    let avg_payment_days: Option<f64> = sqlx::query_scalar(
        "SELECT AVG(julianday((SELECT MAX(p.payment_date) FROM payments p WHERE p.invoice_id = i.id)) - julianday(i.invoice_date))
         FROM invoices i WHERE i.kind = 'FACTURE' AND i.status = 'PAYEE' AND i.invoice_date BETWEEN ? AND ?",
    )
    .bind(&y0).bind(&y1)
    .fetch_one(&mut *conn)
    .await?;
    let by_client = sqlx::query_as::<_, ClientDue>(
        "SELECT i.partner_id, TRIM(pa.name || ' ' || pa.first_name) AS partner_name, COUNT(*) AS count,
            SUM(i.total_ttc - i.amount_paid) AS amount_due,
            COALESCE(SUM(CASE WHEN i.due_date < ?1 THEN i.total_ttc - i.amount_paid ELSE 0 END), 0) AS overdue_due,
            MIN(i.due_date) AS oldest_due_date
         FROM invoices i JOIN partners pa ON pa.id = i.partner_id
         WHERE i.kind = 'FACTURE' AND i.status = 'VALIDEE'
         GROUP BY i.partner_id ORDER BY overdue_due DESC, amount_due DESC",
    )
    .bind(&today)
    .fetch_all(&mut *conn)
    .await?;
    let pending = sqlx::query_as::<_, PendingInvoice>(
        "SELECT i.id, i.number, i.partner_id, TRIM(pa.name || ' ' || pa.first_name) AS partner_name, i.invoice_date, i.due_date, i.sent_at,
            i.last_reminder_at, i.total_ttc, i.total_ttc - i.amount_paid AS amount_due,
            CAST(MAX(0, julianday(?1) - julianday(i.due_date)) AS INTEGER) AS days_late
         FROM invoices i JOIN partners pa ON pa.id = i.partner_id
         WHERE i.kind = 'FACTURE' AND i.status = 'VALIDEE'
         ORDER BY i.due_date, i.id LIMIT 200",
    )
    .bind(&today)
    .fetch_all(&mut *conn)
    .await?;
    let einvoicing_enabled: bool = sqlx::query_scalar("SELECT einvoicing_enabled FROM company_settings WHERE id = 1").fetch_one(&mut *conn).await?;
    let ei = |status: &str| format!("SELECT COUNT(*) AS count, COALESCE(SUM(total_ttc), 0) AS amount FROM invoices WHERE number IS NOT NULL AND invoice_date BETWEEN ? AND ? AND einvoice_status {status}");
    let einvoice_to_send = b(conn, &ei("= 'A_TRANSMETTRE'"), &[&y0, &y1]).await?;
    let einvoice_sent = b(conn, &ei("= 'TRANSMISE'"), &[&y0, &y1]).await?;
    let einvoice_accepted = b(conn, &ei("= 'ACCEPTEE'"), &[&y0, &y1]).await?;
    let einvoice_refused = b(conn, &ei("= 'REFUSEE'"), &[&y0, &y1]).await?;
    let einvoice_out_of_scope = b(conn, &ei("= 'HORS_CHAMP'"), &[&y0, &y1]).await?;
    let einvoice_untracked = b(conn, &ei("IS NULL"), &[&y0, &y1]).await?;
    Ok(Overview {
        year, today, drafts, not_sent, sent_waiting, overdue, overdue_1_30, overdue_31_60, overdue_61_90, overdue_90_plus,
        paid_year, credit_notes_year, issued_year, collected_year, collected_month,
        avg_payment_days: avg_payment_days.map(|d| d.round() as i64), by_client, pending,
        einvoicing_enabled, einvoice_to_send, einvoice_sent, einvoice_accepted, einvoice_refused, einvoice_out_of_scope, einvoice_untracked,
    })
}

// ---------- Liste ----------

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
// Filtres de la liste des factures (paramètres d'URL).
pub struct Filters {
    pub q: Option<String>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub partner_id: Option<i64>,
    pub project_id: Option<i64>,
    pub overdue: Option<bool>,
    /// Envoyées (true) ou non envoyées (false) au client.
    pub sent: Option<bool>,
    /// Statut de transmission électronique.
    pub einvoice: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

// Filtres en SQL paramétré (recherche, client, statut, type, retard, dates).
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &Filters) {
    if let Some(q) = f.q.as_deref() {
        for w in search_key(q).split(' ').filter(|w| !w.is_empty()) {
            let like = format!("%{}%", w.replace(['%', '_'], ""));
            qb.push(" AND (lower(COALESCE(i.number, '')) LIKE ").push_bind(like.clone())
                .push(" OR c.search_text LIKE ").push_bind(like.clone())
                .push(" OR lower(COALESCE(p.name, '')) LIKE ").push_bind(like).push(")");
        }
    }
    if let Some(s) = f.status.as_deref().filter(|s| ["BROUILLON", "VALIDEE", "PAYEE", "ANNULEE"].contains(s)) {
        qb.push(" AND i.status = ").push_bind(s.to_string());
    }
    if let Some(k) = f.kind.as_deref().filter(|k| ["FACTURE", "AVOIR"].contains(k)) {
        qb.push(" AND i.kind = ").push_bind(k.to_string());
    }
    if let Some(pid) = f.partner_id {
        qb.push(" AND i.partner_id = ").push_bind(pid);
    }
    if let Some(pid) = f.project_id {
        qb.push(" AND i.project_id = ").push_bind(pid);
    }
    if let Some(e) = f.einvoice.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND i.einvoice_status = ").push_bind(e.to_string());
    }
    match f.sent {
        Some(true) => { qb.push(" AND i.sent_at IS NOT NULL"); }
        Some(false) => { qb.push(" AND i.sent_at IS NULL AND i.status <> 'BROUILLON'"); }
        None => {}
    }
    if f.overdue == Some(true) {
        qb.push(" AND i.kind = 'FACTURE' AND i.status = 'VALIDEE' AND i.due_date < ").push_bind(today());
    }
    if let Some(d) = f.from.as_deref().filter(|d| parse_date(d).is_some()) {
        qb.push(" AND i.invoice_date >= ").push_bind(d.to_string());
    }
    if let Some(d) = f.to.as_deref().filter(|d| parse_date(d).is_some()) {
        qb.push(" AND i.invoice_date <= ").push_bind(d.to_string());
    }
}

#[derive(Debug, Serialize)]
// Liste paginée avec sommes HT / TTC / reste dû de l'ensemble filtré.
pub struct ListResponse {
    pub items: Vec<InvoiceRow>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub sum_ht: i64,
    pub sum_ttc: i64,
    pub sum_due: i64,
}

// GET /invoices : total, sommes puis page.
pub async fn list(pool: &SqlitePool, f: &Filters) -> AppResult<ListResponse> {
    let per_page = f.per_page.unwrap_or(80).clamp(1, 500);
    let page = f.page.unwrap_or(1).max(1);
    let mut count = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN i.kind = 'AVOIR' THEN -i.total_ht ELSE i.total_ht END), 0),
            COALESCE(SUM(CASE WHEN i.kind = 'AVOIR' THEN -i.total_ttc ELSE i.total_ttc END), 0),
            COALESCE(SUM(CASE WHEN i.kind = 'FACTURE' AND i.status IN ('VALIDEE', 'PAYEE') THEN i.total_ttc - i.amount_paid ELSE 0 END), 0)
         FROM invoices i JOIN partners c ON c.id = i.partner_id LEFT JOIN projects p ON p.id = i.project_id WHERE 1 = 1",
    );
    push_filters(&mut count, f);
    let (total, sum_ht, sum_ttc, sum_due): (i64, i64, i64, i64) = count.build_query_as().fetch_one(pool).await?;
    let mut qb = QueryBuilder::<Sqlite>::new(select_invoice!("WHERE 1 = 1"));
    push_filters(&mut qb, f);
    qb.push(" ORDER BY i.status = 'BROUILLON' DESC, i.invoice_date DESC, i.number DESC, i.id DESC LIMIT ").push_bind(per_page)
        .push(" OFFSET ").push_bind((page - 1) * per_page);
    let items = qb.build_query_as::<InvoiceRow>().fetch_all(pool).await?;
    Ok(ListResponse { items, total, page, per_page, sum_ht, sum_ttc, sum_due })
}

// ---------- Facturation des temps et frais (lot 6) ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
// Temps facturable en attente, avec son libellé de tâche.
pub struct BillableTime {
    pub id: i64,
    pub date: String,
    pub user_name: String,
    pub task_name: Option<String>,
    pub description: String,
    pub duration_minutes: i64,
    pub hourly_rate: i64,
    pub amount: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
// Frais facturable en attente.
pub struct BillableExpense {
    pub id: i64,
    pub kind: String,
    pub label: String,
    pub quantity_milli: i64,
    pub unit_price: i64,
    pub amount: i64,
}

#[derive(Debug, Serialize)]
// Tout ce qui reste à facturer sur un projet : temps, frais, forfait non facturé.
pub struct Billable {
    pub project_id: i64,
    pub project_name: String,
    pub client_id: i64,
    pub client_name: String,
    pub mission_id: Option<i64>,
    pub pricing_mode: String,
    pub flat_fee_pending: i64,
    pub timesheets: Vec<BillableTime>,
    pub expenses: Vec<BillableExpense>,
    pub total: i64,
}

// Calcule les éléments facturables d'un projet (écran "Facturer depuis le projet").
pub async fn billable(conn: &mut SqliteConnection, project_id: i64) -> AppResult<Billable> {
    let row: Option<(String, i64, String, Option<i64>, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT p.name, p.client_id, TRIM(c.name || ' ' || c.first_name), m.id, m.pricing_mode, m.flat_fee
         FROM projects p JOIN partners c ON c.id = p.client_id LEFT JOIN missions m ON m.project_id = p.id WHERE p.id = ?",
    )
    .bind(project_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (project_name, client_id, client_name, mission_id, mode, flat) = row.ok_or_else(|| AppError::NotFound("Projet introuvable".into()))?;
    let pricing_mode = mode.unwrap_or_else(|| "HORAIRE".into());
    let timesheets = sqlx::query_as::<_, BillableTime>(
        "SELECT t.id, t.date, u.display_name AS user_name, tk.name AS task_name, t.description, t.duration_minutes, t.hourly_rate, t.amount
         FROM timesheets t JOIN users u ON u.id = t.user_id LEFT JOIN tasks tk ON tk.id = t.task_id
         WHERE t.project_id = ? AND t.billable = 1 AND t.invoice_line_id IS NULL ORDER BY t.date, t.id",
    )
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?;
    let expenses = match mission_id {
        Some(mid) => sqlx::query_as::<_, BillableExpense>(
            "SELECT id, kind, label, quantity_milli, unit_price, amount FROM mission_expenses WHERE mission_id = ? AND billable = 1 AND invoice_line_id IS NULL ORDER BY id",
        )
        .bind(mid)
        .fetch_all(&mut *conn)
        .await?,
        None => vec![],
    };
    let flat_fee_pending = if pricing_mode == "FORFAIT" && !missions::service::forfait_invoiced(conn, project_id).await? { flat.unwrap_or(0) } else { 0 };
    let total = timesheets.iter().map(|t| t.amount).sum::<i64>() + expenses.iter().map(|e| e.amount).sum::<i64>() + flat_fee_pending;
    Ok(Billable { project_id, project_name, client_id, client_name, mission_id, pricing_mode, flat_fee_pending, timesheets, expenses, total })
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
// Sélection à facturer ; listes absentes = tout ce qui est en attente.
pub struct FromItemsInput {
    pub project_id: i64,
    /// Absent = tous les temps facturables non facturés du projet.
    pub timesheet_ids: Option<Vec<i64>>,
    pub expense_ids: Option<Vec<i64>>,
    pub include_flat_fee: Option<bool>,
    pub invoice_date: Option<String>,
}

// Libellé imprimable d'une nature de frais.
fn expense_label(kind: &str) -> &'static str {
    match kind {
        "DEPLACEMENT" => "Frais de déplacement",
        "KILOMETRIQUE" => "Frais kilométriques",
        "HEBERGEMENT" => "Hébergement",
        "RESTAURATION" => "Restauration",
        _ => "Frais",
    }
}

// "2026-11-18" -> "18/11/2026".
fn fr_date(d: &str) -> String {
    parse_date(d).map(|d| d.format("%d/%m/%Y").to_string()).unwrap_or_else(|| d.to_string())
}

/// Crée une facture brouillon à partir des temps et frais non facturés d'un projet ou d'une mission.
// Facture d'un projet depuis ses temps et frais : une ligne par tâche et taux, une par frais, forfait éventuel.
// Chaque temps/frais est marqué facturé dans la même transaction (impossible de facturer deux fois).
pub async fn from_items(pool: &SqlitePool, user_id: i64, f: FromItemsInput) -> AppResult<InvoiceDetail> {
    let cs = settings::load(pool).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let b = billable(&mut tx, f.project_id).await?;
    let ts: Vec<&BillableTime> = match &f.timesheet_ids {
        Some(ids) => {
            for id in ids {
                if !b.timesheets.iter().any(|t| t.id == *id) {
                    return err("Un des temps sélectionnés est déjà facturé, non facturable ou d'un autre projet");
                }
            }
            b.timesheets.iter().filter(|t| ids.contains(&t.id)).collect()
        }
        None => b.timesheets.iter().collect(),
    };
    let ex: Vec<&BillableExpense> = match &f.expense_ids {
        Some(ids) => {
            for id in ids {
                if !b.expenses.iter().any(|e| e.id == *id) {
                    return err("Un des frais sélectionnés est déjà facturé ou d'une autre mission");
                }
            }
            b.expenses.iter().filter(|e| ids.contains(&e.id)).collect()
        }
        None => b.expenses.iter().collect(),
    };
    let flat = if f.include_flat_fee.unwrap_or(true) { b.flat_fee_pending } else { 0 };
    if ts.is_empty() && ex.is_empty() && flat == 0 {
        return err("Rien à facturer : aucun temps ni frais facturable en attente");
    }
    let mut header = InvoiceInput { partner_id: b.client_id, project_id: Some(f.project_id), invoice_date: f.invoice_date.clone(), ..Default::default() };
    check_header(&mut tx, &mut header, &cs).await?;
    let id = insert_draft(&mut tx, user_id, &header, &cs).await?;
    let vat = line_vat(&cs, None)?;
    let mut pos = 0;

    if flat > 0 {
        let line = LineInput { id: None, product_id: None, description: format!("{} - forfait", b.project_name), quantity_milli: 1000, unit: "forfait".into(), unit_price: flat, vat_rate_bp: None };
        insert_line(&mut tx, id, pos, &line, vat, "FORFAIT").await?;
        pos += 1;
    }
    // Temps regroupés par tâche et par taux : "Formation AutoCAD  14 h x 80 €".
    let mut groups: Vec<((Option<String>, i64), Vec<&BillableTime>)> = Vec::new();
    for t in &ts {
        let key = (t.task_name.clone(), t.hourly_rate);
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(t),
            None => groups.push((key, vec![t])),
        }
    }
    for ((task, rate), items) in groups {
        let minutes: i64 = items.iter().map(|t| t.duration_minutes).sum();
        let (first, last) = (items.iter().map(|t| t.date.clone()).min().unwrap_or_default(), items.iter().map(|t| t.date.clone()).max().unwrap_or_default());
        let period = if first == last { format!("le {}", fr_date(&first)) } else { format!("du {} au {}", fr_date(&first), fr_date(&last)) };
        let title = match &task { Some(tn) => format!("{} - {tn}", b.project_name), None => b.project_name.clone() };
        let line = LineInput {
            id: None,
            product_id: None,
            description: format!("{title} ({period})"),
            quantity_milli: minutes * 1000 / 60,
            unit: "h".into(),
            unit_price: rate,
            vat_rate_bp: None,
        };
        // Quantité en heures (millièmes) : le total doit égaler la somme exacte des temps.
        // Les heures sont arrondies au millième : si le total diffère de la somme exacte des temps, la ligne
        // est alignée sur la somme exacte (fiabilité métier avant élégance).
        let exact: i64 = items.iter().map(|t| t.amount).sum();
        let lid = insert_line(&mut tx, id, pos, &line, vat, "TEMPS").await?;
        let (ht, _, _) = compute_line(line.quantity_milli, rate, vat);
        if ht != exact {
            // Écart d'arrondi (minutes non multiples de 3,6 s) : la ligne est recalculée sur le montant exact.
            let tva = Money(exact).vat(vat).0;
            sqlx::query("UPDATE invoice_lines SET total_ht = ?, total_vat = ?, total_ttc = ? WHERE id = ?")
                .bind(exact).bind(tva).bind(exact + tva).bind(lid).execute(&mut *tx).await?;
        }
        for t in &items {
            let n = sqlx::query("UPDATE timesheets SET invoice_line_id = ?, invoiced_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ? AND invoice_line_id IS NULL")
                .bind(lid).bind(t.id).execute(&mut *tx).await?.rows_affected();
            if n != 1 {
                return err("Un temps vient d'être facturé par ailleurs : recommencez");
            }
        }
        pos += 1;
    }
    // Forfait : les temps de la mission sont rattachés à la ligne forfait (non facturables, mais tracés).
    for e in &ex {
        let label = if e.label.is_empty() { expense_label(&e.kind).to_string() } else { format!("{} - {}", expense_label(&e.kind), e.label) };
        let unit = if e.kind == "KILOMETRIQUE" { "km" } else { "" };
        let line = LineInput { id: None, product_id: None, description: label, quantity_milli: e.quantity_milli, unit: unit.into(), unit_price: e.unit_price, vat_rate_bp: None };
        let lid = insert_line(&mut tx, id, pos, &line, vat, "FRAIS").await?;
        let n = sqlx::query("UPDATE mission_expenses SET invoice_line_id = ?, invoiced_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ? AND invoice_line_id IS NULL")
            .bind(lid).bind(e.id).execute(&mut *tx).await?.rows_affected();
        if n != 1 {
            return err("Un frais vient d'être facturé par ailleurs : recommencez");
        }
        pos += 1;
    }
    recompute_totals(&mut tx, id).await?;
    let d = load(&mut tx, id).await?;
    audit::record(&mut tx, Some(user_id), "CREATION_DEPUIS_TEMPS", ENTITY, Some(id.to_string()),
        Some(json!({ "project": b.project_name, "timesheets": ts.len(), "expenses": ex.len(), "flat_fee": flat, "total_ttc": d.row.total_ttc }))).await?;
    tx.commit().await?;
    Ok(d)
}

// Résumé court d'une facture (réponses groupées).
pub fn summary_json(d: &InvoiceDetail) -> Value {
    json!({ "id": d.row.id, "number": d.row.number, "total_ttc": d.row.total_ttc, "status": d.row.status })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calcul_ligne() {
        // 14 h x 80 € = 1 120 € HT, sans TVA.
        assert_eq!(compute_line(14_000, 8000, 0), (112_000, 0, 112_000));
        // 7 h x 80 € à 20 % : 560 HT, 112 TVA, 672 TTC.
        assert_eq!(compute_line(7_000, 8000, 2000), (56_000, 11_200, 67_200));
        // 1,5 x 33,33 € à 5,5 % : 49,995 -> 50,00 ; TVA 2,75.
        assert_eq!(compute_line(1_500, 3333, 550), (5_000, 275, 5_275));
    }

    #[test]
    fn dates() {
        assert_eq!(add_days("2026-11-18", 30), "2026-12-18");
        assert_eq!(fr_date("2027-01-12"), "12/01/2027");
    }
}

// ---------------------------------------------------------------------------
// Facturation mensuelle par centre de formation (ou client direct)
// ---------------------------------------------------------------------------

/// Temps facturable non facturé, avec son rattachement (centre ou client direct).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Temps facturable non facturé, avec son rattachement (centre ou client direct).
pub struct MonthlyItem {
    pub id: i64,
    pub date: String,
    pub project_id: i64,
    pub project_name: String,
    pub mission_id: Option<i64>,
    pub mission_reference: Option<String>,
    pub client_id: i64,
    pub client_name: String,
    pub center_id: Option<i64>,
    pub center_name: Option<String>,
    pub task_name: Option<String>,
    pub user_name: String,
    pub duration_minutes: i64,
    pub hourly_rate: i64,
    pub amount: i64,
}

#[derive(Debug, Serialize)]
// Agrégat par tâche et taux horaire.
pub struct MonthlyTask {
    pub task_name: Option<String>,
    pub hourly_rate: i64,
    pub minutes: i64,
    pub amount: i64,
    pub count: i64,
}

#[derive(Debug, Serialize)]
// Agrégat par mission, avec ses tâches et les identifiants des temps concernés.
pub struct MonthlyMission {
    pub project_id: i64,
    pub mission_id: Option<i64>,
    pub reference: Option<String>,
    pub title: String,
    pub client_name: String,
    pub tasks: Vec<MonthlyTask>,
    pub minutes: i64,
    pub amount: i64,
    pub expenses_total: i64,
    pub timesheet_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
// Un bloc du récapitulatif = un partenaire à facturer.
pub struct MonthlyGroup {
    /// Partenaire facturé : le centre de formation, sinon le client de la mission.
    pub partner_id: i64,
    pub partner_name: String,
    pub is_center: bool,
    pub missions: Vec<MonthlyMission>,
    pub minutes: i64,
    pub amount: i64,
    pub expenses_total: i64,
    pub total: i64,
    /// Minutes datées avant le mois demandé (arriérés inclus dans le total).
    pub minutes_before: i64,
}

#[derive(Debug, Serialize)]
// Récapitulatif complet d'un mois.
pub struct MonthlyReport {
    pub month: String,
    pub month_label: String,
    pub groups: Vec<MonthlyGroup>,
    pub total: i64,
}

// Noms des mois pour les libellés.
const MONTHS_FR: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/// Vérifie "AAAA-MM" et retourne (premier jour, dernier jour, libellé).
pub fn month_bounds(month: &str) -> AppResult<(String, String, String)> {
    let parts: Vec<&str> = month.split('-').collect();
    let (y, m) = match (parts.first().and_then(|y| y.parse::<i32>().ok()), parts.get(1).and_then(|m| m.parse::<u32>().ok())) {
        (Some(y), Some(m)) if parts.len() == 2 && (1..=12).contains(&m) && (2000..=2100).contains(&y) => (y, m),
        _ => return err("Mois attendu au format AAAA-MM"),
    };
    let first = chrono::NaiveDate::from_ymd_opt(y, m, 1).ok_or_else(|| AppError::Validation("Mois invalide".into()))?;
    let next = if m == 12 { chrono::NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { chrono::NaiveDate::from_ymd_opt(y, m + 1, 1) }.unwrap();
    let last = next.pred_opt().unwrap();
    Ok((first.format("%Y-%m-%d").to_string(), last.format("%Y-%m-%d").to_string(), format!("{} {y}", MONTHS_FR[(m - 1) as usize])))
}

// Tous les temps facturables non facturés datés jusqu'à `until` inclus.
async fn monthly_items(conn: &mut SqliteConnection, until: &str) -> AppResult<Vec<MonthlyItem>> {
    Ok(sqlx::query_as::<_, MonthlyItem>(
        "SELECT t.id, t.date, t.project_id, p.name AS project_name, m.id AS mission_id, m.reference AS mission_reference,
                p.client_id, TRIM(c.name || ' ' || c.first_name) AS client_name, m.center_id, ce.name AS center_name,
                tk.name AS task_name, u.display_name AS user_name, t.duration_minutes, t.hourly_rate, t.amount
         FROM timesheets t JOIN projects p ON p.id = t.project_id JOIN partners c ON c.id = p.client_id
         JOIN users u ON u.id = t.user_id LEFT JOIN missions m ON m.project_id = p.id
         LEFT JOIN partners ce ON ce.id = m.center_id LEFT JOIN tasks tk ON tk.id = t.task_id
         WHERE t.billable = 1 AND t.invoice_line_id IS NULL AND p.billable = 1 AND t.date <= ?
         ORDER BY t.date, t.id",
    )
    .bind(until)
    .fetch_all(&mut *conn)
    .await?)
}

// Frais refacturables non facturés d'une mission.
async fn mission_expenses_pending(conn: &mut SqliteConnection, mission_id: i64) -> AppResult<Vec<BillableExpense>> {
    Ok(sqlx::query_as::<_, BillableExpense>(
        "SELECT id, kind, label, quantity_milli, unit_price, amount FROM mission_expenses WHERE mission_id = ? AND billable = 1 AND invoice_line_id IS NULL ORDER BY id",
    )
    .bind(mission_id)
    .fetch_all(&mut *conn)
    .await?)
}

/// Récapitulatif des temps à facturer jusqu'à la fin du mois, groupés par centre, mission et tâche.
pub async fn monthly_report(conn: &mut SqliteConnection, month: &str) -> AppResult<MonthlyReport> {
    let (first, last, label) = month_bounds(month)?;
    let items = monthly_items(conn, &last).await?;
    let mut groups: Vec<MonthlyGroup> = Vec::new();
    for it in &items {
        // Clé de regroupement : le centre s'il existe, sinon le client de la mission.
        let (pid, pname, is_center) = match (it.center_id, &it.center_name) {
            (Some(c), Some(n)) => (c, n.clone(), true),
            _ => (it.client_id, it.client_name.clone(), false),
        };
        let g = match groups.iter_mut().position(|g| g.partner_id == pid) {
            Some(i) => &mut groups[i],
            None => {
                groups.push(MonthlyGroup { partner_id: pid, partner_name: pname, is_center, missions: vec![], minutes: 0, amount: 0, expenses_total: 0, total: 0, minutes_before: 0 });
                groups.last_mut().unwrap()
            }
        };
        let mi = match g.missions.iter_mut().position(|m| m.project_id == it.project_id) {
            Some(i) => &mut g.missions[i],
            None => {
                g.missions.push(MonthlyMission {
                    project_id: it.project_id, mission_id: it.mission_id, reference: it.mission_reference.clone(), title: it.project_name.clone(),
                    client_name: it.client_name.clone(), tasks: vec![], minutes: 0, amount: 0, expenses_total: 0, timesheet_ids: vec![],
                });
                g.missions.last_mut().unwrap()
            }
        };
        let task = match mi.tasks.iter_mut().position(|t| t.task_name == it.task_name && t.hourly_rate == it.hourly_rate) {
            Some(i) => &mut mi.tasks[i],
            None => {
                mi.tasks.push(MonthlyTask { task_name: it.task_name.clone(), hourly_rate: it.hourly_rate, minutes: 0, amount: 0, count: 0 });
                mi.tasks.last_mut().unwrap()
            }
        };
        task.minutes += it.duration_minutes;
        task.amount += it.amount;
        task.count += 1;
        mi.minutes += it.duration_minutes;
        mi.amount += it.amount;
        mi.timesheet_ids.push(it.id);
        g.minutes += it.duration_minutes;
        g.amount += it.amount;
        if it.date < first {
            g.minutes_before += it.duration_minutes;
        }
    }
    for g in &mut groups {
        for m in &mut g.missions {
            if let Some(mid) = m.mission_id {
                m.expenses_total = mission_expenses_pending(conn, mid).await?.iter().map(|e| e.amount).sum();
                g.expenses_total += m.expenses_total;
            }
        }
        g.total = g.amount + g.expenses_total;
    }
    groups.sort_by_key(|g| g.partner_name.to_lowercase());
    let total = groups.iter().map(|g| g.total).sum();
    Ok(MonthlyReport { month: month.to_string(), month_label: label, groups, total })
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
// Demande de facture mensuelle pour un partenaire.
pub struct MonthlyInvoiceInput {
    pub partner_id: i64,
    pub month: String,
    pub invoice_date: Option<String>,
    /// Par défaut true : les frais en attente des missions concernées sont ajoutés.
    pub include_expenses: Option<bool>,
}

/// Une facture par centre (ou client direct) pour tous les temps non facturés jusqu'à la fin du mois.
pub async fn monthly_invoice(pool: &SqlitePool, user_id: i64, f: MonthlyInvoiceInput) -> AppResult<InvoiceDetail> {
    let cs = settings::load(pool).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let report = monthly_report(&mut tx, &f.month).await?;
    let g = report.groups.into_iter().find(|g| g.partner_id == f.partner_id)
        .ok_or_else(|| AppError::Validation("Rien à facturer pour ce centre sur cette période".into()))?;
    // Le projet n'est rattaché que pour un client direct à mission unique (la facture d'un centre couvre d'autres clients).
    let single_project = if g.missions.len() == 1 && !g.is_center { Some(g.missions[0].project_id) } else { None };
    let mut header = InvoiceInput {
        partner_id: g.partner_id, project_id: single_project, invoice_date: f.invoice_date.clone(),
        notes: format!("Prestations de formation - {}", report.month_label), ..Default::default()
    };
    check_header(&mut tx, &mut header, &cs).await?;
    let id = insert_draft(&mut tx, user_id, &header, &cs).await?;
    let vat = line_vat(&cs, None)?;
    // Relecture des temps dans la transaction : ce qui a été facturé entre-temps est exclu.
    let items = monthly_items(&mut tx, &month_bounds(&f.month)?.1).await?;
    let mut pos = 0;
    let mut count = 0usize;
    for m in &g.missions {
        let mine: Vec<&MonthlyItem> = items.iter().filter(|i| m.timesheet_ids.contains(&i.id)).collect();
        for t in &m.tasks {
            let sel: Vec<&MonthlyItem> = mine.iter().copied().filter(|i| i.task_name == t.task_name && i.hourly_rate == t.hourly_rate).collect();
            if sel.is_empty() {
                continue;
            }
            let (first, last) = (sel.iter().map(|i| i.date.clone()).min().unwrap_or_default(), sel.iter().map(|i| i.date.clone()).max().unwrap_or_default());
            let period = if first == last { format!("le {}", fr_date(&first)) } else { format!("du {} au {}", fr_date(&first), fr_date(&last)) };
            // Libellé : "MF-2026-0003 Intitulé - Client - Tâche (du .. au ..)" ; le client n'est précisé que pour un centre.
            let mut title = match &m.reference { Some(r) => format!("{r} {}", m.title), None => m.title.clone() };
            if g.is_center {
                title = format!("{title} - {}", m.client_name);
            }
            if let Some(tn) = &t.task_name {
                title = format!("{title} - {tn}");
            }
            let line = LineInput { id: None, product_id: None, description: format!("{title} ({period})"), quantity_milli: t.minutes * 1000 / 60, unit: "h".into(), unit_price: t.hourly_rate, vat_rate_bp: None };
            let lid = insert_line(&mut tx, id, pos, &line, vat, "TEMPS").await?;
            let (ht, _, _) = compute_line(line.quantity_milli, t.hourly_rate, vat);
            if ht != t.amount {
                let tva = Money(t.amount).vat(vat).0;
                sqlx::query("UPDATE invoice_lines SET total_ht = ?, total_vat = ?, total_ttc = ? WHERE id = ?")
                    .bind(t.amount).bind(tva).bind(t.amount + tva).bind(lid).execute(&mut *tx).await?;
            }
            for i in &sel {
                let n = sqlx::query("UPDATE timesheets SET invoice_line_id = ?, invoiced_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ? AND invoice_line_id IS NULL")
                    .bind(lid).bind(i.id).execute(&mut *tx).await?.rows_affected();
                if n != 1 {
                    return err("Un temps vient d'être facturé par ailleurs : recommencez");
                }
                count += 1;
            }
            pos += 1;
        }
        if f.include_expenses.unwrap_or(true)
            && let Some(mid) = m.mission_id
        {
            for e in mission_expenses_pending(&mut tx, mid).await? {
                let base = if e.label.is_empty() { expense_label(&e.kind).to_string() } else { format!("{} - {}", expense_label(&e.kind), e.label) };
                let unit = if e.kind == "KILOMETRIQUE" { "km" } else { "" };
                let line = LineInput { id: None, product_id: None, description: format!("{} - {base}", m.title), quantity_milli: e.quantity_milli, unit: unit.into(), unit_price: e.unit_price, vat_rate_bp: None };
                let lid = insert_line(&mut tx, id, pos, &line, vat, "FRAIS").await?;
                let n = sqlx::query("UPDATE mission_expenses SET invoice_line_id = ?, invoiced_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ? AND invoice_line_id IS NULL")
                    .bind(lid).bind(e.id).execute(&mut *tx).await?.rows_affected();
                if n != 1 {
                    return err("Un frais vient d'être facturé par ailleurs : recommencez");
                }
                pos += 1;
            }
        }
    }
    if pos == 0 {
        return err("Rien à facturer pour ce centre sur cette période");
    }
    recompute_totals(&mut tx, id).await?;
    let d = load(&mut tx, id).await?;
    audit::record(&mut tx, Some(user_id), "CREATION_MENSUELLE", ENTITY, Some(id.to_string()),
        Some(json!({ "partner": g.partner_name, "month": f.month, "missions": g.missions.len(), "timesheets": count, "total_ttc": d.row.total_ttc }))).await?;
    tx.commit().await?;
    Ok(d)
}
