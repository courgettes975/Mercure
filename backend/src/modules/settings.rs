//! Paramètres > Entreprise : identité, banque, TVA, mentions légales, numérotation.

use axum::{
    body::Bytes,
    extract::{Multipart, State},
    http::header,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::CurrentUser,
    domain::{
        roles::Permission,
        validation::{is_valid_email, is_valid_iban, is_valid_siren, is_valid_siret},
    },
    error::{AppError, AppResult},
    modules::audit,
    state::AppState,
};

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone, Debug)]
// Ligne unique de la table company_settings (id = 1). Sert aussi de corps de requête pour la mise à jour.
pub struct CompanySettings {
    pub legal_name: String,
    pub address_line1: String,
    pub address_line2: String,
    pub zip: String,
    pub city: String,
    pub country: String,
    pub siren: String,
    pub siret: String,
    pub vat_number: String,
    #[serde(default)]
    // Numéro de déclaration d'activité (organisme de formation), 11 chiffres.
    pub nda: String,
    pub email: String,
    pub phone: String,
    pub website: String,
    pub bank_name: String,
    pub iban: String,
    pub bic: String,
    // Conditions de règlement imprimées sur les factures.
    pub payment_terms: String,
    // Délai d'échéance par défaut en jours.
    pub default_due_days: i64,
    // FRANCHISE (art. 293 B) ou ASSUJETTI.
    pub vat_regime: String,
    // Taux de TVA par défaut en points de base (2000 = 20 %).
    pub default_vat_rate_bp: i64,
    pub vat_exemption_mention: String,
    pub legal_mentions: String,
    // Préfixe et largeur du numéro de facture : FAC-2026-000001.
    pub invoice_prefix: String,
    pub invoice_number_width: i64,
    #[serde(default, skip_deserializing)]
    // Nom du fichier logo dans le dossier documents ; jamais modifié par la mise à jour JSON.
    pub logo_file: Option<String>,
    /// Facturation électronique : activée (contrôles et statut de transmission), plateforme agréée choisie,
    /// option de paiement de la TVA d'après les débits, catégorie d'opération par défaut (SERVICES / BIENS / MIXTE).
    pub einvoicing_enabled: bool,
    pub einvoicing_platform: String,
    pub vat_on_debits: bool,
    pub default_operation_category: String,
}

// Lecture complète de la ligne de paramètres.
const SELECT: &str = "SELECT legal_name, address_line1, address_line2, zip, city, country, siren, siret,
    vat_number, nda, email, phone, website, bank_name, iban, bic, payment_terms, default_due_days, vat_regime,
    einvoicing_enabled, einvoicing_platform, vat_on_debits, default_operation_category,
    default_vat_rate_bp, vat_exemption_mention, legal_mentions, invoice_prefix, invoice_number_width, logo_file
    FROM company_settings WHERE id = 1";

// Chargement utilisé par les autres modules (facturation, PDF).
pub async fn load(pool: &sqlx::SqlitePool) -> AppResult<CompanySettings> {
    Ok(sqlx::query_as::<_, CompanySettings>(SELECT).fetch_one(pool).await?)
}

// Lecture par l'interface (tous les rôles : les mentions servent à l'affichage).
pub async fn get(State(st): State<AppState>, _user: CurrentUser) -> AppResult<Json<CompanySettings>> {
    Ok(Json(load(&st.pool).await?))
}

// Supprime les espaces en bordure.
fn clean(s: &mut String) {
    *s = s.trim().to_string();
}

// Normalise (espaces, majuscules des identifiants) puis contrôle chaque champ ; renvoie le premier défaut.
fn validate(s: &mut CompanySettings) -> AppResult<()> {
    for f in [
        &mut s.legal_name, &mut s.address_line1, &mut s.address_line2, &mut s.zip, &mut s.city,
        &mut s.country, &mut s.siren, &mut s.siret, &mut s.vat_number, &mut s.nda, &mut s.email, &mut s.phone,
        &mut s.website, &mut s.bank_name, &mut s.iban, &mut s.bic, &mut s.payment_terms,
        &mut s.vat_exemption_mention, &mut s.legal_mentions, &mut s.invoice_prefix,
    ] {
        clean(f);
    }
    // Identifiants stockés sans espaces et en majuscules pour comparaison et impression.
    s.siren = s.siren.replace(' ', "");
    s.siret = s.siret.replace(' ', "");
    s.iban = s.iban.replace(' ', "").to_uppercase();
    s.bic = s.bic.replace(' ', "").to_uppercase();
    s.vat_number = s.vat_number.replace(' ', "").to_uppercase();
    s.nda = s.nda.replace(' ', "");

    let err = |m: &str| Err(AppError::Validation(m.into()));
    if s.legal_name.is_empty() {
        return err("La raison sociale est obligatoire");
    }
    if !s.email.is_empty() && !is_valid_email(&s.email) {
        return err("Adresse email invalide");
    }
    if !s.siren.is_empty() && !is_valid_siren(&s.siren) {
        return err("SIREN invalide (9 chiffres, clé de contrôle)");
    }
    if !s.siret.is_empty() && !is_valid_siret(&s.siret) {
        return err("SIRET invalide (14 chiffres, clé de contrôle)");
    }
    if !s.siren.is_empty() && !s.siret.is_empty() && !s.siret.starts_with(&s.siren) {
        return err("Le SIRET doit commencer par le SIREN");
    }
    if !s.nda.is_empty() && !(s.nda.len() == 11 && s.nda.chars().all(|c| c.is_ascii_digit())) {
        return err("NDA invalide (11 chiffres)");
    }
    if !s.iban.is_empty() && !is_valid_iban(&s.iban) {
        return err("IBAN invalide");
    }
    if !s.bic.is_empty() && !(s.bic.len() == 8 || s.bic.len() == 11) {
        return err("BIC invalide (8 ou 11 caractères)");
    }
    if !(0..=365).contains(&s.default_due_days) {
        return err("Le délai d'échéance doit être compris entre 0 et 365 jours");
    }
    // Cohérence du régime de TVA et du taux par défaut.
    s.einvoicing_platform = s.einvoicing_platform.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.default_operation_category.is_empty() {
        s.default_operation_category = "SERVICES".into();
    }
    if !["SERVICES", "BIENS", "MIXTE"].contains(&s.default_operation_category.as_str()) {
        return Err(AppError::Validation("Catégorie d'opération inconnue (SERVICES, BIENS ou MIXTE)".into()));
    }
    // La facture électronique exige le SIREN de l'émetteur.
    if s.einvoicing_enabled && s.siren.is_empty() && s.siret.is_empty() {
        return Err(AppError::Validation("Renseignez le SIREN ou le SIRET avant d'activer la facturation électronique".into()));
    }
    match s.vat_regime.as_str() {
        "FRANCHISE" => {
            if s.default_vat_rate_bp != 0 {
                return err("En franchise en base, le taux de TVA par défaut doit être 0 %");
            }
        }
        "ASSUJETTI" => {
            if !(0..=10_000).contains(&s.default_vat_rate_bp) {
                return err("Taux de TVA invalide");
            }
        }
        _ => return err("Régime de TVA inconnu"),
    }
    if s.invoice_prefix.is_empty()
        || s.invoice_prefix.len() > 10
        || !s.invoice_prefix.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return err("Préfixe de facture : 1 à 10 lettres ou chiffres");
    }
    if !(3..=10).contains(&s.invoice_number_width) {
        return err("Nombre de chiffres du numéro : entre 3 et 10");
    }
    Ok(())
}

// Enregistrement des paramètres (ADMIN) ; le journal garde le détail des champs modifiés.
pub async fn update(
    State(st): State<AppState>,
    user: CurrentUser,
    Json(mut req): Json<CompanySettings>,
) -> AppResult<Json<CompanySettings>> {
    user.require(Permission::ManageSettings)?;
    validate(&mut req)?;
    let mut tx = st.pool.begin().await?;
    let before = sqlx::query_as::<_, CompanySettings>(SELECT).fetch_one(&mut *tx).await?;
    sqlx::query(
        "UPDATE company_settings SET legal_name=?, address_line1=?, address_line2=?, zip=?, city=?, country=?,
            siren=?, siret=?, vat_number=?, nda=?, email=?, phone=?, website=?, bank_name=?, iban=?, bic=?,
            payment_terms=?, default_due_days=?, vat_regime=?, default_vat_rate_bp=?, vat_exemption_mention=?,
            legal_mentions=?, invoice_prefix=?, invoice_number_width=?,
            einvoicing_enabled=?, einvoicing_platform=?, vat_on_debits=?, default_operation_category=?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1",
    )
    .bind(&req.legal_name).bind(&req.address_line1).bind(&req.address_line2).bind(&req.zip)
    .bind(&req.city).bind(&req.country).bind(&req.siren).bind(&req.siret).bind(&req.vat_number).bind(&req.nda)
    .bind(&req.email).bind(&req.phone).bind(&req.website).bind(&req.bank_name).bind(&req.iban)
    .bind(&req.bic).bind(&req.payment_terms).bind(req.default_due_days).bind(&req.vat_regime)
    .bind(req.default_vat_rate_bp).bind(&req.vat_exemption_mention).bind(&req.legal_mentions)
    .bind(&req.invoice_prefix).bind(req.invoice_number_width)
    .bind(req.einvoicing_enabled).bind(&req.einvoicing_platform).bind(req.vat_on_debits).bind(&req.default_operation_category)
    .execute(&mut *tx)
    .await?;
    let after = sqlx::query_as::<_, CompanySettings>(SELECT).fetch_one(&mut *tx).await?;
    let changes = audit::diff(&json!(before), &json!(after));
    audit::record(&mut tx, Some(user.id), "MODIFICATION", "company_settings", Some("1".into()), Some(changes)).await?;
    tx.commit().await?;
    Ok(Json(after))
}

// Taille maximale du logo (2 Mo).
const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;

// Détection du format par les premiers octets (la extension annoncée par le navigateur n'est pas fiable).
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}

// Réception du logo en multipart, écriture sur disque sous un nom fixe, référence en base.
pub async fn upload_logo(
    State(st): State<AppState>,
    user: CurrentUser,
    mut multipart: Multipart,
) -> AppResult<Json<CompanySettings>> {
    user.require(Permission::ManageSettings)?;
    let mut data: Option<Bytes> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::Validation("Envoi du fichier invalide".into()))?
    {
        if field.name() == Some("logo") {
            data = Some(field.bytes().await.map_err(|_| AppError::Validation("Fichier trop volumineux".into()))?);
        }
    }
    let data = data.ok_or_else(|| AppError::Validation("Aucun fichier reçu".into()))?;
    if data.len() > MAX_LOGO_BYTES {
        return Err(AppError::Validation("Le logo ne doit pas dépasser 2 Mo".into()));
    }
    let ext = sniff_image(&data).ok_or_else(|| AppError::Validation("Format accepté : PNG ou JPEG".into()))?;
    let file_name = format!("logo.{ext}");
    tokio::fs::write(st.paths.documents.join(&file_name), &data).await.map_err(AppError::internal)?;

    let mut tx = st.pool.begin().await?;
    sqlx::query("UPDATE company_settings SET logo_file = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = 1")
        .bind(&file_name)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, Some(user.id), "MODIFICATION", "company_settings", Some("1".into()), Some(json!({ "logo_file": file_name }))).await?;
    tx.commit().await?;
    Ok(Json(load(&st.pool).await?))
}

// Renvoie le fichier logo (interface et aperçu).
pub async fn get_logo(State(st): State<AppState>, _user: CurrentUser) -> AppResult<impl IntoResponse> {
    let s = load(&st.pool).await?;
    let file = s.logo_file.ok_or_else(|| AppError::NotFound("Aucun logo".into()))?;
    let bytes = tokio::fs::read(st.paths.documents.join(&file))
        .await
        .map_err(|_| AppError::NotFound("Logo introuvable".into()))?;
    let mime = if file.ends_with(".png") { "image/png" } else { "image/jpeg" };
    Ok(([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache")], bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> CompanySettings {
        CompanySettings {
            legal_name: "Demo Formation".into(), address_line1: String::new(), address_line2: String::new(),
            zip: String::new(), city: String::new(), country: "France".into(), siren: String::new(),
            siret: String::new(), vat_number: String::new(), nda: String::new(), email: String::new(), phone: String::new(),
            website: String::new(), bank_name: String::new(), iban: String::new(), bic: String::new(),
            payment_terms: String::new(), default_due_days: 30, vat_regime: "FRANCHISE".into(),
            default_vat_rate_bp: 0, vat_exemption_mention: String::new(), legal_mentions: String::new(),
            invoice_prefix: "FAC".into(), invoice_number_width: 6, logo_file: None,
            einvoicing_enabled: false, einvoicing_platform: String::new(), vat_on_debits: false, default_operation_category: "SERVICES".into(),
        }
    }

    #[test]
    fn franchise_impose_taux_zero() {
        let mut s = base();
        s.default_vat_rate_bp = 2000;
        assert!(validate(&mut s).is_err());
        s.vat_regime = "ASSUJETTI".into();
        assert!(validate(&mut s).is_ok());
    }

    #[test]
    fn siret_coherent_avec_siren() {
        let mut s = base();
        s.siren = "944 642 917".into();
        s.siret = "94464291700016".into();
        assert!(validate(&mut s).is_ok());
        assert_eq!(s.siren, "944642917");
        s.siren = "732829320".into();
        assert!(validate(&mut s).is_err());
    }

    #[test]
    fn nda_11_chiffres() {
        let mut s = base();
        s.nda = "75 33 12345 33".into();
        assert!(validate(&mut s).is_ok());
        assert_eq!(s.nda, "75331234533");
        s.nda = "7533".into();
        assert!(validate(&mut s).is_err());
    }
}
