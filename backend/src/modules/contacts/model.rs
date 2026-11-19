//! Types du module Contacts.

use serde::{Deserialize, Serialize};

// Types de contact : personne physique, société cliente, organisme (centre de formation, OPCO...).
pub const KINDS: [&str; 3] = ["PARTICULIER", "ENTREPRISE", "ORGANISME"];

// Société ou organisme (par opposition à un particulier).
pub fn is_company_kind(kind: &str) -> bool {
    kind == "ENTREPRISE" || kind == "ORGANISME"
}

/// "MARTIN Jean" pour une personne, raison sociale sinon.
pub fn display_name(kind: &str, name: &str, first_name: &str) -> String {
    if kind == "PARTICULIER" && !first_name.is_empty() {
        format!("{name} {first_name}")
    } else {
        name.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq)]
// Étiquette libre (ex. "Import Odoo", "Doublon possible").
pub struct Tag {
    pub id: i64,
    pub name: String,
}

/// Données saisies (création et modification).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ContactInput {
    pub kind: String,
    // Société de rattachement d'une personne (un particulier seulement).
    pub parent_id: Option<i64>,
    pub name: String,
    pub first_name: String,
    // Rôles cumulables : client facturé, prospect, stagiaire.
    pub is_client: bool,
    pub is_prospect: bool,
    pub is_stagiaire: bool,
    pub job_title: String,
    pub email: String,
    pub phone: String,
    pub mobile: String,
    pub street: String,
    pub street2: String,
    pub zip: String,
    pub city: String,
    pub country: String,
    pub website: String,
    pub notes: String,
    pub siren: String,
    pub siret: String,
    pub vat_number: String,
    pub duns: String,
    // Étiquettes à associer (remplace la liste existante en modification).
    pub tag_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Contact tel qu'enregistré, avec le nom de la société parente résolu.
pub struct ContactRecord {
    pub id: i64,
    pub kind: String,
    pub parent_id: Option<i64>,
    pub parent_name: Option<String>,
    pub name: String,
    pub first_name: String,
    pub is_client: bool,
    pub is_prospect: bool,
    pub is_stagiaire: bool,
    pub job_title: String,
    pub email: String,
    pub phone: String,
    pub mobile: String,
    pub street: String,
    pub street2: String,
    pub zip: String,
    pub city: String,
    pub country: String,
    pub website: String,
    pub notes: String,
    // Numéro client CLI-NNNNN attribué à la première facture ou au marquage "client".
    pub customer_number: Option<String>,
    pub siren: String,
    pub siret: String,
    pub vat_number: String,
    pub duns: String,
    pub archived_at: Option<String>,
    // Jeu de démonstration : supprimable d'un bloc.
    pub is_demo: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
// Fiche complète : enregistrement + nom d'affichage + étiquettes + nombre de personnes rattachées.
pub struct ContactDetail {
    #[serde(flatten)]
    pub record: ContactRecord,
    pub display_name: String,
    pub tags: Vec<Tag>,
    pub children_count: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Colonnes réduites pour la liste (performance).
pub struct ContactListRow {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub first_name: String,
    pub parent_id: Option<i64>,
    pub parent_name: Option<String>,
    pub email: String,
    pub phone: String,
    pub mobile: String,
    pub city: String,
    pub country: String,
    pub is_client: bool,
    pub is_prospect: bool,
    pub is_stagiaire: bool,
    pub customer_number: Option<String>,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
// Ligne de liste avec nom d'affichage et étiquettes.
pub struct ContactListItem {
    #[serde(flatten)]
    pub row: ContactListRow,
    pub display_name: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
// Filtres de la liste (paramètres d'URL).
pub struct ContactFilters {
    pub q: Option<String>,
    pub kind: Option<String>,
    /// client | prospect | stagiaire
    pub role: Option<String>,
    pub tag_id: Option<i64>,
    pub parent_id: Option<i64>,
    /// actifs (défaut) | archives | tous
    pub archived: Option<String>,
    // Sociétés et organismes uniquement (sélecteurs "Client", "Centre").
    pub companies_only: Option<bool>,
    /// name | city | country | email | created (préfixe "-" = décroissant)
    pub sort: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Note libre horodatée.
pub struct Note {
    pub id: i64,
    pub author_name: Option<String>,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Entrée du journal d'audit concernant le contact.
pub struct HistoryEntry {
    pub id: i64,
    pub user_name: Option<String>,
    pub action: String,
    pub changes: Option<String>,
    pub created_at: String,
}
