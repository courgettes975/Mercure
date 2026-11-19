//! Types du module Missions de formation.

use serde::{Deserialize, Serialize};

// Cycle de vie d'une mission, du brouillon à la facturation.
pub const STATUSES: [&str; 9] = [
    "BROUILLON", "A_PLANIFIER", "PLANIFIEE", "CONFIRMEE", "EN_COURS", "REALISEE", "A_FACTURER", "FACTUREE", "ANNULEE",
];
// Natures de frais.
pub const EXPENSE_KINDS: [&str; 5] = ["DEPLACEMENT", "KILOMETRIQUE", "HEBERGEMENT", "RESTAURATION", "AUTRE"];
// Catégories de documents joints (convention, émargement, attestation...).
pub const DOC_CATEGORIES: [&str; 12] = [
    "CONVENTION", "CONVOCATION", "PROGRAMME", "DEVIS", "BON_COMMANDE", "EMARGEMENT", "ATTESTATION", "CERTIFICAT",
    "EVALUATION", "FACTURE", "CLIENT", "AUTRE",
];
/// Hypothèse : une journée de formation = 7 heures (tarif journalier -> équivalent horaire).
pub const MINUTES_PER_DAY: i64 = 7 * 60;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
// Saisie complète d'une mission (création et modification).
pub struct MissionInput {
    pub title: String,
    pub client_id: i64,
    pub contact_id: Option<i64>,
    // Centre de formation donneur d'ordre (facturé en fin de mois), distinct du client.
    pub center_id: Option<i64>,
    pub trainer_id: Option<i64>,
    pub software_id: Option<i64>,
    pub description: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    // "", "PC" ou "MAC".
    pub os: String,
    // PRESENTIEL / DISTANCIEL / HYBRIDE et INDIVIDUEL / GROUPE.
    pub modality: String,
    pub group_format: String,
    pub provide_pc: bool,
    pub recording: bool,
    pub planned_minutes: i64,
    pub days_count: i64,
    pub schedule_text: String,
    pub address: String,
    pub room: String,
    pub visio_link: String,
    // HORAIRE (tarif horaire), JOURNALIER (tarif / jour de 7 h) ou FORFAIT.
    pub pricing_mode: String,
    pub hourly_rate: i64,
    pub daily_rate: i64,
    pub flat_fee: i64,
    pub billable: Option<bool>,
    pub special_conditions: String,
    pub internal_notes: String,
    // Zone libre de la fiche.
    pub annotations: String,
    pub file_created: bool,
    pub first_call_done: bool,
    // Suivi du dossier : dossier créé, premier appel, facture envoyée.
    pub invoice_sent: bool,
    // Volet pédagogique (programme, prérequis, évaluation...).
    pub objectives: String,
    pub prerequisites: String,
    pub program: String,
    pub methods: String,
    pub means: String,
    pub evaluation_terms: String,
    pub evaluation_result: String,
    pub satisfaction: String,
    pub trainer_comment: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Mission telle que lue en base, avec les noms résolus (client, contact, centre, formateur, logiciel).
pub struct MissionRecord {
    pub id: i64,
    pub project_id: i64,
    pub reference: String,
    pub status: String,
    pub title: String,
    pub client_id: i64,
    pub client_name: String,
    pub contact_id: Option<i64>,
    pub contact_name: Option<String>,
    pub center_id: Option<i64>,
    pub center_name: Option<String>,
    pub trainer_id: Option<i64>,
    pub trainer_name: Option<String>,
    pub software_id: Option<i64>,
    pub software_name: Option<String>,
    pub description: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub os: String,
    pub modality: String,
    pub group_format: String,
    pub provide_pc: bool,
    pub recording: bool,
    pub planned_minutes: i64,
    pub days_count: i64,
    pub schedule_text: String,
    pub address: String,
    pub room: String,
    pub visio_link: String,
    pub pricing_mode: String,
    pub hourly_rate: i64,
    pub daily_rate: i64,
    pub flat_fee: i64,
    pub billable: bool,
    pub special_conditions: String,
    pub internal_notes: String,
    pub annotations: String,
    pub file_created: bool,
    pub first_call_done: bool,
    pub invoice_sent: bool,
    pub objectives: String,
    pub prerequisites: String,
    pub program: String,
    pub methods: String,
    pub means: String,
    pub evaluation_terms: String,
    pub evaluation_result: String,
    pub satisfaction: String,
    pub trainer_comment: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
// Séance de formation ; `timesheet_id` relie au temps généré quand elle est réalisée.
pub struct Session {
    pub id: i64,
    pub mission_id: i64,
    pub date: String,
    pub start_time: String,
    pub end_time: String,
    pub break_minutes: i64,
    pub duration_minutes: i64,
    pub trainer_id: Option<i64>,
    pub trainer_name: Option<String>,
    pub location: String,
    pub status: String,
    pub notes: String,
    pub timesheet_id: Option<i64>,
    pub invoiced: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
// Saisie d'une séance : la durée est calculée (fin - début - pause).
pub struct SessionInput {
    pub date: String,
    pub start_time: String,
    pub end_time: String,
    pub break_minutes: i64,
    pub trainer_id: Option<i64>,
    pub location: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Stagiaire inscrit, avec son suivi pédagogique et ses présences cumulées.
pub struct Participant {
    pub id: i64,
    pub partner_id: i64,
    pub name: String,
    pub first_name: String,
    pub company: Option<String>,
    pub email: String,
    pub phone: String,
    pub job_title: String,
    pub progress_status: String,
    pub evaluation: String,
    pub comment: String,
    pub minutes_done: i64,
    pub sessions_present: i64,
    pub sessions_absent: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Présence d'un participant à une séance.
pub struct Attendance {
    pub session_id: i64,
    pub participant_id: i64,
    pub present: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Frais refacturable ou non ; `invoiced` = déjà repris sur une facture.
pub struct Expense {
    pub id: i64,
    pub kind: String,
    pub label: String,
    pub expense_date: Option<String>,
    pub quantity_milli: i64,
    pub unit_price: i64,
    pub amount: i64,
    pub billable: bool,
    pub invoiced: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
// Saisie d'un frais : quantité en millièmes x prix unitaire en centimes.
pub struct ExpenseInput {
    pub kind: String,
    pub label: String,
    pub expense_date: Option<String>,
    pub quantity_milli: i64,
    pub unit_price: i64,
    pub billable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Pièce jointe (métadonnées ; le fichier est sur le disque).
pub struct Document {
    pub id: i64,
    pub category: String,
    pub file_name: String,
    pub mime_type: String,
    pub size: i64,
    pub uploaded_by_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Facture liée à la mission (aperçu dans la fiche).
pub struct InvoiceRef {
    pub id: i64,
    pub number: Option<String>,
    pub kind: String,
    pub status: String,
    pub invoice_date: String,
    pub total_ttc: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
// Totaux calculés de la fiche : prévu, réalisé, facturé, reste à facturer.
pub struct Totals {
    /// Prestation prévue (heures, jours ou forfait), hors frais.
    pub planned_fee: i64,
    pub expenses_total: i64,
    pub planned_total: i64,
    pub sessions_minutes: i64,
    pub realized_minutes: i64,
    pub realized_amount: i64,
    pub invoiced_ht: i64,
    pub uninvoiced_amount: i64,
}

#[derive(Debug, Serialize)]
// Fiche complète renvoyée à l'interface.
pub struct MissionDetail {
    #[serde(flatten)]
    pub record: MissionRecord,
    pub sessions: Vec<Session>,
    pub participants: Vec<Participant>,
    pub attendance: Vec<Attendance>,
    pub expenses: Vec<Expense>,
    pub documents: Vec<Document>,
    pub invoices: Vec<InvoiceRef>,
    pub totals: Totals,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
// Ligne de la liste des missions, avec quelques agrégats (sessions, temps, reste à facturer).
pub struct MissionListRow {
    pub id: i64,
    pub project_id: i64,
    pub reference: String,
    pub status: String,
    pub title: String,
    pub client_id: i64,
    pub client_name: String,
    pub center_id: Option<i64>,
    pub center_name: Option<String>,
    pub invoice_sent: bool,
    pub trainer_name: Option<String>,
    pub software_name: Option<String>,
    pub modality: String,
    pub group_format: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub first_session: Option<String>,
    pub last_session: Option<String>,
    pub planned_minutes: i64,
    pub realized_minutes: i64,
    pub participants: i64,
    pub pricing_mode: String,
    pub hourly_rate: i64,
    pub daily_rate: i64,
    pub flat_fee: i64,
    pub days_count: i64,
    pub expenses_total: i64,
    pub uninvoiced_amount: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
// Filtres de la liste (paramètres d'URL) ; `bucket` = raccourcis (semaine, à facturer...).
pub struct MissionFilters {
    pub q: Option<String>,
    pub status: Option<String>,
    pub bucket: Option<String>,
    pub client_id: Option<i64>,
    pub center_id: Option<i64>,
    pub trainer_id: Option<i64>,
    pub software_id: Option<i64>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub archived: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

/// Prestation prévue hors frais.
pub fn planned_fee(pricing_mode: &str, planned_minutes: i64, days: i64, hourly: i64, daily: i64, flat: i64) -> i64 {
    match pricing_mode {
        "JOURNALIER" => daily * days,
        "FORFAIT" => flat,
        _ => crate::domain::money::Money::for_minutes(crate::domain::money::Money(hourly), planned_minutes).0,
    }
}

/// Taux horaire appliqué aux temps générés par les sessions.
pub fn effective_hourly_rate(pricing_mode: &str, hourly: i64, daily: i64) -> i64 {
    match pricing_mode {
        "JOURNALIER" => crate::domain::money::div_round(daily as i128 * 60, MINUTES_PER_DAY as i128) as i64,
        "FORFAIT" => 0,
        _ => hourly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn montants_prevus() {
        assert_eq!(planned_fee("HORAIRE", 14 * 60, 2, 8000, 0, 0), 112_000);
        assert_eq!(planned_fee("JOURNALIER", 0, 2, 0, 50_000, 0), 100_000);
        assert_eq!(planned_fee("FORFAIT", 0, 0, 0, 0, 90_000), 90_000);
        assert_eq!(effective_hourly_rate("JOURNALIER", 0, 56_000), 8_000);
        assert_eq!(effective_hourly_rate("FORFAIT", 8000, 0), 0);
    }
}
