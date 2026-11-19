//! Import de contacts depuis un export Odoo (XLSX ou CSV).
//!
//! Colonnes reconnues (en-têtes Odoo, insensibles à la casse et aux accents) :
//! "Nom d'affichage" / "Nom", "E-mail", "Téléphone", "Mobile", "Pays", "Rue", "Rue 2", "Code postal", "Ville",
//! "Site Web", "Poste" / "Fonction", "N° TVA" / "TVA", "SIRET", "SIREN", "Étiquettes", "Est une société",
//! "Notes", "Stats". Les autres colonnes sont ignorées.
//!
//! Règles (hypothèses documentées dans docs/screens/CNT-IMPORT.md) :
//! - "SOCIÉTÉ, Personne" : la personne est rattachée à la société (créée si absente).
//! - Les noms utilisés comme société de rattachement, ou contenant un marqueur (SAS, ASSOCIATION...), sont des
//!   sociétés ; ORGANISME si le nom évoque la formation, ENTREPRISE sinon.
//! - Nom de personne : mots en MAJUSCULES -> nom, autres mots -> prénom (sinon tout dans le nom).
//! - Téléphone : "0625459190 / 0381635632" -> deux numéros ; numéro à 9 chiffres -> zéro initial rétabli ;
//!   06 / 07 / +33 6 / +33 7 -> mobile.
//! - Stats Odoo (factures, commandes) -> note ; au moins une facture ou commande -> client.
//! - Personne rattachée à une société cliente, avec un email hors domaine de la société -> stagiaire.
//! - Étiquette "Import Odoo" sur tout ; "Doublon possible" (même email ou même nom) ; "À compléter" (nom = email) ;
//!   "Fiche formation Odoo" ("Nom - intitulé de formation").

use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use calamine::{open_workbook_auto_from_rs, Reader};
use serde::Serialize;
use serde_json::json;
use sqlx::SqlitePool;

use super::model::ContactInput;
use super::service;
use crate::{
    domain::validation::{is_valid_email, normalize_spaces, search_key},
    error::{AppError, AppResult},
    modules::audit,
};

// Étiquettes posées automatiquement par l'import.
pub const TAG_IMPORT: &str = "Import Odoo";
pub const TAG_DUPLICATE: &str = "Doublon possible";
pub const TAG_INCOMPLETE: &str = "À compléter";
pub const TAG_TRAINING: &str = "Fiche formation Odoo";

// Une ligne du fichier : en-tête normalisé (minuscules, sans accents) -> valeur.
type Row = HashMap<String, String>;

/// Lit le fichier et renvoie les lignes sous forme "en-tête normalisé -> valeur".
pub fn read_rows(bytes: &[u8], file_name: &str) -> AppResult<Vec<Row>> {
    let lower = file_name.to_lowercase();
    let table: Vec<Vec<String>> = if lower.ends_with(".csv") {
        let text = String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_string();
        let first = text.lines().next().unwrap_or("");
        let delim = if first.matches(';').count() > first.matches(',').count() { b';' } else { b',' };
        let mut rdr = csv::ReaderBuilder::new().delimiter(delim).has_headers(false).flexible(true).from_reader(text.as_bytes());
        rdr.records()
            .map(|r| r.map(|r| r.iter().map(|c| c.to_string()).collect()))
            .collect::<Result<_, _>>()
            .map_err(|_| AppError::Validation("Fichier CSV illisible".into()))?
    } else if lower.ends_with(".xlsx") || lower.ends_with(".xls") || lower.ends_with(".ods") {
        let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
            .map_err(|_| AppError::Validation("Classeur illisible (XLSX, XLS ou ODS attendu)".into()))?;
        let range = wb
            .worksheet_range_at(0)
            .ok_or_else(|| AppError::Validation("Le classeur ne contient aucune feuille".into()))?
            .map_err(|_| AppError::Validation("Feuille illisible".into()))?;
        range.rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect()
    } else {
        return Err(AppError::Validation("Format accepté : .xlsx, .xls, .ods ou .csv".into()));
    };
    let mut it = table.into_iter();
    let headers: Vec<String> = it
        .next()
        .ok_or_else(|| AppError::Validation("Fichier vide".into()))?
        .iter()
        .map(|h| search_key(h))
        .collect();
    let rows: Vec<Row> = it
        .filter(|r| r.iter().any(|c| !c.trim().is_empty()))
        .map(|r| headers.iter().cloned().zip(r).collect())
        .collect();
    if !headers.iter().any(|h| matches!(h.as_str(), "nom d'affichage" | "nom" | "name" | "display name")) {
        return Err(AppError::Validation("Colonne « Nom d'affichage » ou « Nom » introuvable".into()));
    }
    Ok(rows)
}

// Première colonne trouvée parmi plusieurs noms possibles (en-têtes Odoo FR/EN).
fn get<'a>(r: &'a Row, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|k| r.get(*k).map(|v| v.trim()).filter(|v| !v.is_empty())).unwrap_or("")
}

// Valeur nettoyée des espaces superflus.
fn clean(s: &str) -> String {
    normalize_spaces(&s.replace('\t', " "))
}

// Mots qui signent un organisme de formation...
const ORG_WORDS: [&str; 10] = ["formation", "forma", "association", "campus", "ecole", "universite", "learn", "educ", "academ", "institut"];
// ... ou une société (formes juridiques et mots courants).
const COMPANY_WORDS: [&str; 16] = [
    "sas", "sasu", "sarl", "sàrl", "eurl", "sa", "sci", "association", "agence", "services", "campus", "groupe",
    "architecture", "societe", "cabinet", "studio",
];

// Heuristique : le nom contient un marqueur de société.
fn looks_like_company(name: &str) -> bool {
    let key = search_key(name);
    key.split(|c: char| !c.is_alphanumeric()).any(|w| COMPANY_WORDS.contains(&w))
        || ORG_WORDS.iter().any(|w| key.contains(w) && *w != "forma")
}

// ORGANISME si le nom évoque la formation, ENTREPRISE sinon.
fn company_kind(name: &str) -> &'static str {
    let key = search_key(name);
    if ORG_WORDS.iter().any(|w| key.contains(w)) { "ORGANISME" } else { "ENTREPRISE" }
}

// Mot entièrement en majuscules (au moins 2 lettres) : convention Odoo pour le nom de famille.
fn is_upper_word(w: &str) -> bool {
    w.chars().any(|c| c.is_alphabetic()) && w.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
}

/// "LOPEZ Éléonore" -> ("LOPEZ", "Éléonore") ; "Jean-Jacques OBLED" -> ("OBLED", "Jean-Jacques").
// "MARTIN Jean-Pierre" -> ("MARTIN", "Jean-Pierre") ; sans majuscules, tout va dans le nom.
pub fn split_person(full: &str) -> (String, String) {
    let words: Vec<&str> = full.split(' ').filter(|w| !w.is_empty()).collect();
    let upper: Vec<&str> = words.iter().copied().filter(|w| is_upper_word(w) && w.chars().filter(|c| c.is_alphabetic()).count() > 1).collect();
    let other: Vec<&str> = words.iter().copied().filter(|w| !upper.contains(w)).collect();
    if upper.is_empty() || other.is_empty() {
        (full.to_string(), String::new())
    } else {
        (upper.join(" "), other.join(" "))
    }
}

// Extrait les adresses valides d'une cellule pouvant en contenir plusieurs.
fn emails_in(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '/' | '<' | '>'))
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| is_valid_email(w))
        .collect()
}

// Garde chiffres et +, rétablit le 0 initial d'un numéro à 9 chiffres.
fn normalize_phone(p: &str) -> String {
    let p = p.trim();
    let digits: String = p.chars().filter(|c| c.is_ascii_digit()).collect();
    if p.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ' ') && digits.len() == 9 && !digits.starts_with('0') {
        // Zéro initial perdu par Excel.
        return format!("0{digits}");
    }
    p.to_string()
}

// 06 / 07 / +33 6 / +33 7.
fn is_mobile(p: &str) -> bool {
    let d: String = p.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    d.starts_with("06") || d.starts_with("07") || d.starts_with("+336") || d.starts_with("+337")
}

/// Résumé des statistiques Odoo ("[{label, value}]").
// Transforme la colonne "Stats" d'Odoo en note lisible ; vrai si au moins une facture ou commande (= client).
fn stats_note(raw: &str) -> (String, bool) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (String::new(), false);
    };
    let mut parts = Vec::new();
    let mut business = false;
    for it in items {
        let label = it["label"].as_str().unwrap_or("").to_string();
        let n = it["value"].as_i64().unwrap_or(0);
        if n == 0 {
            continue;
        }
        let l = search_key(&label);
        if l.contains("facture") || l.contains("commande") || l.contains("abonnement") {
            business = true;
        }
        parts.push(format!("{n} {}", label.to_lowercase()));
    }
    (if parts.is_empty() { String::new() } else { format!("Odoo : {}", parts.join(", ")) }, business)
}

// Partie significative du domaine email ("contact@bt-formation.fr" -> "btformation") pour reconnaître le personnel.
fn domain_root(email: &str) -> String {
    let d = email.split('@').nth(1).unwrap_or("");
    let root = d.rsplit_once('.').map(|(a, _)| a).unwrap_or(d);
    root.chars().filter(|c| c.is_alphanumeric()).collect()
}

#[derive(Debug, Clone, Serialize)]
// Un contact prêt à être créé, avec ses étiquettes, remarques et son lien de rattachement.
pub struct PlannedContact {
    pub import_ref: String,
    pub source: String,
    pub input: ContactInput,
    pub parent_ref: Option<String>,
    pub parent_name: Option<String>,
    pub tags: Vec<String>,
    pub remarks: Vec<String>,
    pub already_imported: bool,
}

#[derive(Debug, Serialize)]
// Aperçu renvoyé avant exécution : compteurs + détail.
pub struct ImportPlan {
    pub items: Vec<PlannedContact>,
    pub companies: usize,
    pub persons: usize,
    pub duplicates: usize,
    pub already_imported: usize,
}

/// Construit le plan d'import (aucune écriture).
// Cœur de l'import : transforme les lignes brutes en contacts planifiés selon les règles du module.
pub fn plan(rows: &[Row]) -> Vec<PlannedContact> {
    // Ligne pré-découpée : "SOCIÉTÉ, Personne" -> parent + own.
    struct Raw {
        display: String,
        parent: Option<String>,
        own: String,
        row: Row,
    }
    let raws: Vec<Raw> = rows
        .iter()
        .filter_map(|r| {
            let display = clean(get(r, &["nom d'affichage", "display name", "nom", "name"]));
            if display.is_empty() {
                return None;
            }
            let (parent, own) = match display.split_once(", ") {
                Some((p, o)) if !p.trim().is_empty() && !o.trim().is_empty() => (Some(p.trim().to_string()), o.trim().to_string()),
                _ => (None, display.clone()),
            };
            Some(Raw { display, parent, own, row: r.clone() })
        })
        .collect();

    // Noms utilisés comme société de rattachement : ce sont forcément des sociétés.
    let parent_names: HashSet<String> = raws.iter().filter_map(|r| r.parent.clone()).collect();
    let mut out: Vec<PlannedContact> = Vec::new();
    let mut client_companies: HashSet<String> = HashSet::new();

    for r in &raws {
        let row = &r.row;
        let mut tags = vec![TAG_IMPORT.to_string()];
        let mut remarks = Vec::new();
        let mut notes = Vec::new();

        let mut own = r.own.clone();
        // "ARLIGUY Hanna - Utilisation d'un logiciel ... formation ..." : fiche formation Odoo.
        if let Some((who, what)) = own.split_once(" - ") {
            let w = search_key(what);
            if w.contains("formation") || w.contains("utilisation") || w.contains("logiciel") {
                notes.push(format!("Formation (Odoo) : {what}"));
                tags.push(TAG_TRAINING.into());
                remarks.push("Fiche formation Odoo : à transformer en mission".into());
                own = who.trim().to_string();
            }
        }

        // Société si Odoo le dit, si le nom sert de rattachement, ou si le nom ressemble à une société.
        let explicit_company = matches!(search_key(get(row, &["est une societe", "is a company"])).as_str(), "vrai" | "true" | "oui" | "1");
        let own_is_email = is_valid_email(&own.to_lowercase());
        let is_company = r.parent.is_none()
            && (explicit_company || parent_names.contains(&r.display) || (!own_is_email && looks_like_company(&own)));

        let raw_email = get(row, &["e-mail", "email", "courriel"]);
        let emails = emails_in(raw_email);
        if !raw_email.is_empty() && emails.is_empty() {
            notes.push(format!("Email non reconnu à l'import : {raw_email}"));
            remarks.push("Email illisible".into());
        }
        if emails.len() > 1 {
            notes.push(format!("Autres emails : {}", emails[1..].join(", ")));
        }

        // Téléphones : plusieurs numéros séparés par / ; le premier mobile devient "mobile", le reste "téléphone" ou note.
        let mut phones: Vec<String> = get(row, &["telephone", "phone"])
            .split('/')
            .map(normalize_phone)
            .filter(|p| !p.is_empty())
            .collect();
        let mut mobile = normalize_phone(get(row, &["mobile", "portable"]));
        let mut phone = String::new();
        for p in phones.drain(..) {
            if mobile.is_empty() && is_mobile(&p) {
                mobile = p;
            } else if phone.is_empty() {
                phone = p;
            } else {
                notes.push(format!("Autre téléphone : {p}"));
            }
        }

        let (stats, business) = stats_note(get(row, &["stats"]));
        if !stats.is_empty() {
            notes.push(stats);
        }
        let extra_notes = get(row, &["notes", "note"]);
        if !extra_notes.is_empty() {
            notes.push(extra_notes.to_string());
        }

        let name_is_email = is_valid_email(&own.to_lowercase());
        if name_is_email {
            tags.push(TAG_INCOMPLETE.into());
            remarks.push("Nom = adresse email : à compléter".into());
        }

        // Type et découpage nom/prénom selon le cas.
        let (kind, name, first_name) = if is_company {
            (company_kind(&own).to_string(), own.clone(), String::new())
        } else if name_is_email {
            ("PARTICULIER".to_string(), own.to_lowercase(), String::new())
        } else {
            let (n, f) = split_person(&own);
            ("PARTICULIER".to_string(), n, f)
        };
        if is_company && business {
            client_companies.insert(r.display.clone());
        }

        // Construction du contact ; les colonnes d'adresse et d'identifiants sont reprises telles quelles.
        let mut input = ContactInput {
            kind,
            name,
            first_name,
            is_client: business,
            email: emails.first().cloned().unwrap_or_default(),
            phone,
            mobile,
            street: clean(get(row, &["rue", "street", "adresse"])),
            street2: clean(get(row, &["rue 2", "street2", "complement"])),
            zip: clean(get(row, &["code postal", "zip"])),
            city: clean(get(row, &["ville", "city"])),
            country: clean(get(row, &["pays", "country"])),
            website: clean(get(row, &["site web", "website"])),
            job_title: clean(get(row, &["poste", "fonction", "job position"])),
            vat_number: clean(get(row, &["n° tva", "tva", "numero de tva", "vat"])),
            siret: clean(get(row, &["siret"])),
            siren: clean(get(row, &["siren"])),
            notes: notes.join("\n"),
            ..Default::default()
        };
        for t in get(row, &["etiquettes", "tags"]).split(',').map(clean).filter(|t| !t.is_empty()) {
            tags.push(t);
        }
        // Les champs invalides ne bloquent pas l'import : ils passent en note.
        // Un SIRET/SIREN invalide ne bloque pas l'import : la valeur passe en note.
        let mut probe = input.clone();
        if let Err(AppError::Validation(msg)) = service::normalize_and_validate(&mut probe) {
            for (field, label) in [("siret", "SIRET"), ("siren", "SIREN")] {
                if msg.contains(label) {
                    let v = if field == "siret" { std::mem::take(&mut input.siret) } else { std::mem::take(&mut input.siren) };
                    input.notes = format!("{}\n{label} importé non valide : {v}", input.notes).trim().to_string();
                    remarks.push(format!("{label} invalide (mis en note)"));
                }
            }
        }

        out.push(PlannedContact {
            import_ref: format!("odoo:{}", r.display),
            source: r.display.clone(),
            input,
            parent_ref: r.parent.as_ref().map(|p| format!("odoo:{p}")),
            parent_name: r.parent.clone(),
            tags,
            remarks,
            already_imported: false,
        });
    }

    // Sociétés de rattachement absentes du fichier : créées.
    // Sociétés citées comme rattachement mais absentes du fichier : créées vides.
    let existing: HashSet<String> = out.iter().map(|p| p.source.clone()).collect();
    for p in parent_names.iter().filter(|p| !existing.contains(*p)) {
        out.push(PlannedContact {
            import_ref: format!("odoo:{p}"),
            source: p.clone(),
            input: ContactInput { kind: company_kind(p).into(), name: p.clone(), ..Default::default() },
            parent_ref: None,
            parent_name: None,
            tags: vec![TAG_IMPORT.into()],
            remarks: vec!["Société créée (absente du fichier)".into()],
            already_imported: false,
        });
    }

    // Stagiaires : personnes rattachées à une société cliente, email hors domaine de la société.
    // Détection des stagiaires et des interlocuteurs selon le domaine de l'email.
    for p in out.iter_mut() {
        if let Some(parent) = &p.parent_name {
            let slug: String = search_key(parent).chars().filter(|c| c.is_alphanumeric()).collect();
            let root = domain_root(&p.input.email);
            let staff = !root.is_empty() && root.len() >= 4 && (slug.contains(&root) || root.contains(&slug));
            let same_as_parent = search_key(&p.source) == search_key(&format!("{parent}, {parent}"));
            // Sans email, il faut au moins un NOM en majuscules pour reconnaître une personne.
            let person_like = !p.input.email.is_empty() || p.input.name.split(' ').any(is_upper_word);
            if same_as_parent {
                p.tags.push(TAG_DUPLICATE.into());
                p.remarks.push("Contact portant le nom de sa société".into());
            } else if client_companies.contains(parent) && !staff && person_like && p.input.kind == "PARTICULIER" {
                p.input.is_stagiaire = true;
            }
            if staff {
                p.remarks.push("Interlocuteur de la société (même domaine email)".into());
            }
        }
    }

    // Doublons possibles : même email, ou même nom de personne (ordre des mots indifférent).
    // Doublons : même email, ou même nom de personne quel que soit l'ordre des mots.
    let person_key = |p: &PlannedContact| {
        let mut w: Vec<String> = search_key(&format!("{} {}", p.input.name, p.input.first_name)).split(' ').map(String::from).collect();
        w.sort();
        w.join(" ")
    };
    let mut by_email: HashMap<String, usize> = HashMap::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for p in &out {
        if !p.input.email.is_empty() {
            *by_email.entry(p.input.email.clone()).or_default() += 1;
        }
        *by_name.entry(format!("{}|{}", p.input.kind, person_key(p))).or_default() += 1;
    }
    for p in out.iter_mut() {
        let e = !p.input.email.is_empty() && by_email[&p.input.email] > 1;
        let n = by_name[&format!("{}|{}", p.input.kind, person_key(p))] > 1;
        if (e || n) && !p.tags.iter().any(|t| t == TAG_DUPLICATE) {
            p.tags.push(TAG_DUPLICATE.into());
            p.remarks.push(if e { "Même email qu'un autre contact".into() } else { "Même nom qu'un autre contact".into() });
        }
    }

    // Sociétés d'abord pour que les rattachements existent.
    // Ordre d'insertion : sociétés d'abord pour que les rattachements existent.
    out.sort_by_key(|p| (p.parent_ref.is_some(), search_key(&p.source)));
    out
}

// Marque les contacts dont la référence d'import (odoo:<nom>) est déjà en base.
pub async fn mark_existing(pool: &SqlitePool, items: &mut [PlannedContact]) -> AppResult<()> {
    let refs: HashSet<String> = sqlx::query_scalar::<_, String>("SELECT import_ref FROM partners WHERE import_ref IS NOT NULL")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
    for it in items.iter_mut() {
        it.already_imported = refs.contains(&it.import_ref);
    }
    Ok(())
}

// Compteurs de l'aperçu.
pub fn summarize(items: Vec<PlannedContact>) -> ImportPlan {
    ImportPlan {
        companies: items.iter().filter(|p| p.input.kind != "PARTICULIER" && !p.already_imported).count(),
        persons: items.iter().filter(|p| p.input.kind == "PARTICULIER" && !p.already_imported).count(),
        duplicates: items.iter().filter(|p| p.tags.iter().any(|t| t == TAG_DUPLICATE)).count(),
        already_imported: items.iter().filter(|p| p.already_imported).count(),
        items,
    }
}

#[derive(Debug, Serialize)]
// Bilan de l'exécution.
pub struct ImportResult {
    pub created: usize,
    pub skipped: usize,
}

/// Exécute l'import en une seule transaction : tout ou rien.
pub async fn execute(pool: &SqlitePool, user_id: i64, items: Vec<PlannedContact>) -> AppResult<ImportResult> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    // Cache des étiquettes créées ou retrouvées (nom -> id).
    let mut tag_ids: HashMap<String, i64> = HashMap::new();
    // Références déjà importées : évite les doublons si on relance le même fichier.
    let mut ids: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>("SELECT import_ref, id FROM partners WHERE import_ref IS NOT NULL")
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
    let (mut created, mut skipped) = (0, 0);
    for mut it in items {
        if ids.contains_key(&it.import_ref) {
            skipped += 1;
            continue;
        }
        for t in &it.tags {
            if !tag_ids.contains_key(t) {
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO tags (name) VALUES (?) ON CONFLICT(name) DO UPDATE SET name = name RETURNING id",
                )
                .bind(t)
                .fetch_one(&mut *tx)
                .await?;
                tag_ids.insert(t.clone(), id);
            }
            it.input.tag_ids.push(tag_ids[t]);
        }
        it.input.parent_id = it.parent_ref.as_ref().and_then(|r| ids.get(r).copied());
        service::normalize_and_validate(&mut it.input)
            .map_err(|e| AppError::Validation(format!("« {} » : {e}", it.source)))?;
        let id = service::insert_in_tx(&mut tx, user_id, &it.input).await?;
        sqlx::query("UPDATE partners SET import_ref = ? WHERE id = ?").bind(&it.import_ref).bind(id).execute(&mut *tx).await?;
        ids.insert(it.import_ref.clone(), id);
        created += 1;
    }
    audit::record(&mut tx, Some(user_id), "IMPORT", service::ENTITY, None, Some(json!({ "created": created, "skipped": skipped, "source": "Odoo" }))).await?;
    tx.commit().await?;
    Ok(ImportResult { created, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, &str)]) -> Row {
        pairs.iter().map(|(k, v)| (search_key(k), v.to_string())).collect()
    }

    #[test]
    fn decoupage_nom_prenom() {
        assert_eq!(split_person("LOPEZ ARMELLE ANGELIQUE ELEONORE"), ("LOPEZ ARMELLE ANGELIQUE ELEONORE".into(), "".into()));
        assert_eq!(split_person("Jean-Jacques OBLED"), ("OBLED".into(), "Jean-Jacques".into()));
        assert_eq!(split_person("CADOR Catherine"), ("CADOR".into(), "Catherine".into()));
        assert_eq!(split_person("noelle jaunier"), ("noelle jaunier".into(), "".into()));
    }

    #[test]
    fn telephones() {
        assert_eq!(normalize_phone("174642710"), "0174642710");
        assert_eq!(normalize_phone("+33 6 11 65 30 34"), "+33 6 11 65 30 34");
        assert!(is_mobile("+33 6 11 65 30 34"));
        assert!(!is_mobile("+33 5 56 90 00 10"));
    }

    #[test]
    fn plan_odoo() {
        let rows = vec![
            row(&[("Nom d'affichage", "BT-FORMATION"), ("Téléphone", "+33 3 88 38 81 26"), ("Pays", "France"),
                  ("Stats", r#"[{"iconClass": "fa-pencil-square-o", "value": 24, "label": "Factures / Factures fournisseurs / Mandats"}]"#)]),
            row(&[("Nom d'affichage", "BT-FORMATION, LAMBIN Mickaël"), ("E-mail", "meli_mon@hotmail.fr"), ("Téléphone", "+33 6 88 24 56 41")]),
            row(&[("Nom d'affichage", "BT-FORMATION, barbara GIBALDI"), ("E-mail", "bgibaldi@bt-formation.fr")]),
            row(&[("Nom d'affichage", "BT-FORMATION, Jean-Jacques OBLED"), ("E-mail", "JJObled@peugeot-saveurs.com"), ("Téléphone", "0625459190 / 0381635632")]),
            row(&[("Nom d'affichage", "OBLED Jean-Jacques"), ("E-mail", "jjobled@gmail.com")]),
            row(&[("Nom d'affichage", "SENZA, MOULIN Jean-Luc"), ("E-mail", ":  jeanlucmoulin63@gmail.com")]),
            row(&[("Nom d'affichage", "alix@one-learn.fr"), ("E-mail", "alix@one-learn.fr")]),
            row(&[("Nom d'affichage", "ARLIGUY Hanna - Utilisation d'un logiciel de CAO 3D (Revit) : formation individuelle 8h")]),
            row(&[("Nom d'affichage", "ASSOCIATION ARTILECT")]),
            row(&[("Nom d'affichage", "paul@nouvelleecole.fr"), ("E-mail", "paul@nouvelleecole.fr")]),
            row(&[("Nom d'affichage", "SENZA, SENZA")]),
            row(&[("Nom d'affichage", "SENZA, antoine")]),
            row(&[("Nom d'affichage", "SENZA, MARIN Matias")]),
            row(&[("Nom d'affichage", "SENZA"), ("Stats", r#"[{"value": 2, "label": "Commandes clients"}]"#)]),
        ];
        let p = plan(&rows);
        let find = |s: &str| p.iter().find(|x| x.source == s).unwrap();

        let bt = find("BT-FORMATION");
        assert_eq!(bt.input.kind, "ORGANISME");
        assert!(bt.input.is_client);
        assert!(bt.input.notes.contains("24 factures"));

        let lambin = find("BT-FORMATION, LAMBIN Mickaël");
        assert_eq!((lambin.input.name.as_str(), lambin.input.first_name.as_str()), ("LAMBIN", "Mickaël"));
        assert_eq!(lambin.input.mobile, "+33 6 88 24 56 41");
        assert!(lambin.input.is_stagiaire);
        assert_eq!(lambin.parent_ref.as_deref(), Some("odoo:BT-FORMATION"));

        let gibaldi = find("BT-FORMATION, barbara GIBALDI");
        assert!(!gibaldi.input.is_stagiaire);

        let obled = find("BT-FORMATION, Jean-Jacques OBLED");
        assert_eq!(obled.input.mobile, "0625459190");
        assert_eq!(obled.input.phone, "0381635632");
        assert!(obled.tags.contains(&TAG_DUPLICATE.to_string()));
        assert!(find("OBLED Jean-Jacques").tags.contains(&TAG_DUPLICATE.to_string()));

        assert_eq!(find("SENZA, MOULIN Jean-Luc").input.email, "jeanlucmoulin63@gmail.com");
        assert_eq!(find("paul@nouvelleecole.fr").input.kind, "PARTICULIER");
        assert!(!find("SENZA, SENZA").input.is_stagiaire);
        assert!(find("SENZA, SENZA").tags.contains(&TAG_DUPLICATE.to_string()));
        assert!(!find("SENZA, antoine").input.is_stagiaire);
        assert!(find("SENZA, MARIN Matias").input.is_stagiaire);

        assert!(find("alix@one-learn.fr").tags.contains(&TAG_INCOMPLETE.to_string()));
        let arliguy = find("ARLIGUY Hanna - Utilisation d'un logiciel de CAO 3D (Revit) : formation individuelle 8h");
        assert_eq!(arliguy.input.name, "ARLIGUY");
        assert!(arliguy.tags.contains(&TAG_TRAINING.to_string()));

        assert_eq!(find("ASSOCIATION ARTILECT").input.kind, "ORGANISME");
        // Les sociétés passent avant leurs interlocuteurs.
        let pos = |s: &str| p.iter().position(|x| x.source == s).unwrap();
        assert!(pos("BT-FORMATION") < pos("BT-FORMATION, LAMBIN Mickaël"));
    }
}
