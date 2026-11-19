//! Règles métier du module Contacts.

use serde_json::{json, Value};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use super::model::*;
use crate::{
    domain::validation::{is_valid_email, is_valid_siren, is_valid_siret, normalize_spaces, search_key},
    error::{AppError, AppResult},
    modules::{audit, sequence},
};

// Type d'entité dans le journal d'audit.
pub const ENTITY: &str = "contact";

// Lecture d'un contact complet avec le nom de sa société parente.
macro_rules! select_record {
    ($tail:literal) => {
        concat!(
            "SELECT p.id, p.kind, p.parent_id, pp.name AS parent_name, p.name, p.first_name, p.is_client,
                p.is_prospect, p.is_stagiaire, p.job_title, p.email, p.phone, p.mobile, p.street, p.street2,
                p.zip, p.city, p.country, p.website, p.notes, p.customer_number, p.siren, p.siret, p.vat_number,
                p.duns, p.archived_at, p.is_demo, p.created_at, p.updated_at
             FROM partners p LEFT JOIN partners pp ON pp.id = p.parent_id ",
            $tail
        )
    };
}

// Espaces normalisés.
fn trim(s: &mut String) {
    *s = normalize_spaces(s);
}

/// Nettoie et contrôle la saisie. La base reste la référence pour l'unicité et l'intégrité.
pub fn normalize_and_validate(i: &mut ContactInput) -> AppResult<()> {
    for f in [
        &mut i.name, &mut i.first_name, &mut i.job_title, &mut i.email, &mut i.phone, &mut i.mobile,
        &mut i.street, &mut i.street2, &mut i.zip, &mut i.city, &mut i.country, &mut i.website,
    ] {
        trim(f);
    }
    i.notes = i.notes.trim().to_string();
    // Identifiants normalisés : email en minuscules, SIREN/SIRET/TVA/DUNS sans espaces.
    i.email = i.email.to_lowercase();
    i.siren = i.siren.replace(' ', "");
    i.siret = i.siret.replace(' ', "");
    i.vat_number = i.vat_number.replace(' ', "").to_uppercase();
    i.duns = i.duns.replace([' ', '-'], "");
    i.tag_ids.sort_unstable();
    i.tag_ids.dedup();
    if i.country.is_empty() {
        i.country = "France".into();
    }

    let err = |m: &str| Err(AppError::Validation(m.into()));
    if !KINDS.contains(&i.kind.as_str()) {
        return err("Type de contact inconnu");
    }
    if i.name.is_empty() {
        return err(if i.kind == "PARTICULIER" { "Le nom est obligatoire" } else { "La raison sociale est obligatoire" });
    }
    // Une société n'a pas de prénom et ne se rattache pas à une autre société (un seul niveau).
    if is_company_kind(&i.kind) {
        i.first_name.clear();
        if i.parent_id.is_some() {
            return err("Une entreprise ou un organisme ne peut pas être rattaché à une autre société");
        }
    }
    if !i.email.is_empty() && !is_valid_email(&i.email) {
        return err("Adresse email invalide");
    }
    if !i.siren.is_empty() && !is_valid_siren(&i.siren) {
        return err("SIREN invalide (9 chiffres, clé de contrôle)");
    }
    if !i.siret.is_empty() && !is_valid_siret(&i.siret) {
        return err("SIRET invalide (14 chiffres, clé de contrôle)");
    }
    if !i.siren.is_empty() && !i.siret.is_empty() && !i.siret.starts_with(&i.siren) {
        return err("Le SIRET doit commencer par le SIREN");
    }
    if !i.duns.is_empty() && !(i.duns.len() == 9 && i.duns.chars().all(|c| c.is_ascii_digit())) {
        return err("DUNS invalide (9 chiffres)");
    }
    if i.is_client && i.is_prospect {
        return err("Un contact ne peut pas être à la fois client et prospect");
    }
    Ok(())
}

// Contrôles qui nécessitent la base : société parente valide, pas de boucle, étiquettes existantes,
// une société avec des personnes rattachées ne peut pas devenir un particulier.
async fn check_relations(conn: &mut SqliteConnection, id: Option<i64>, i: &ContactInput) -> AppResult<()> {
    if let Some(pid) = i.parent_id {
        if Some(pid) == id {
            return Err(AppError::Validation("Un contact ne peut pas être rattaché à lui-même".into()));
        }
        let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM partners WHERE id = ?")
            .bind(pid)
            .fetch_optional(&mut *conn)
            .await?;
        match kind {
            None => return Err(AppError::Validation("Société de rattachement introuvable".into())),
            Some(k) if !is_company_kind(&k) => {
                return Err(AppError::Validation("Le rattachement doit être une entreprise ou un organisme".into()))
            }
            _ => {}
        }
    }
    if let Some(id) = id
        && !is_company_kind(&i.kind) {
            let children: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partners WHERE parent_id = ?")
                .bind(id)
                .fetch_one(&mut *conn)
                .await?;
            if children > 0 {
                return Err(AppError::Validation(
                    "Des contacts sont rattachés à cette société : elle ne peut pas devenir un particulier".into(),
                ));
            }
        }
    for tag in &i.tag_ids {
        let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM tags WHERE id = ?").bind(tag).fetch_optional(&mut *conn).await?;
        if exists.is_none() {
            return Err(AppError::Validation("Étiquette introuvable".into()));
        }
    }
    Ok(())
}

/// Recalcule la clé de recherche d'un contact (et de ses contacts rattachés si c'est une société).
async fn refresh_search(conn: &mut SqliteConnection, id: i64) -> AppResult<()> {
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM partners WHERE id = ?1 OR parent_id = ?1")
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
    for pid in ids {
        let r = sqlx::query_as::<_, ContactRecord>(select_record!("WHERE p.id = ?")).bind(pid).fetch_one(&mut *conn).await?;
        let digits = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
        let key = search_key(&format!(
            "{} {} {} {} {} {} {} {} {} {} {}",
            r.name, r.first_name, r.parent_name.as_deref().unwrap_or(""), r.email, r.city,
            r.customer_number.as_deref().unwrap_or(""), r.siren, r.siret, digits(&r.phone), digits(&r.mobile), r.job_title
        ));
        sqlx::query("UPDATE partners SET search_text = ? WHERE id = ?").bind(key).bind(pid).execute(&mut *conn).await?;
    }
    Ok(())
}

// Attribue CLI-NNNNN à la première fois qu'un contact devient client (jamais réattribué).
async fn assign_customer_number(conn: &mut SqliteConnection, id: i64) -> AppResult<()> {
    let current: Option<String> = sqlx::query_scalar("SELECT customer_number FROM partners WHERE id = ? AND is_client = 1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .flatten();
    let is_client: bool = sqlx::query_scalar("SELECT is_client FROM partners WHERE id = ?").bind(id).fetch_one(&mut *conn).await?;
    if is_client && current.is_none() {
        let n = sequence::next_value(conn, "CLI").await?;
        sqlx::query("UPDATE partners SET customer_number = ? WHERE id = ?")
            .bind(format!("CLI-{n:05}"))
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

// Remplace l'ensemble des étiquettes du contact.
async fn set_tags(conn: &mut SqliteConnection, id: i64, tags: &[i64]) -> AppResult<()> {
    sqlx::query("DELETE FROM partner_tags WHERE partner_id = ?").bind(id).execute(&mut *conn).await?;
    for t in tags {
        sqlx::query("INSERT INTO partner_tags (partner_id, tag_id) VALUES (?, ?)").bind(id).bind(t).execute(&mut *conn).await?;
    }
    Ok(())
}

// Étiquettes d'un contact, triées par nom.
pub async fn tags_of(conn: &mut SqliteConnection, id: i64) -> AppResult<Vec<Tag>> {
    Ok(sqlx::query_as::<_, Tag>(
        "SELECT t.id, t.name FROM tags t JOIN partner_tags pt ON pt.tag_id = t.id WHERE pt.partner_id = ? ORDER BY t.name COLLATE NOCASE",
    )
    .bind(id)
    .fetch_all(conn)
    .await?)
}

// Charge la fiche complète (enregistrement, nom d'affichage, étiquettes, nombre de rattachés).
pub async fn load_detail(conn: &mut SqliteConnection, id: i64) -> AppResult<ContactDetail> {
    let record = sqlx::query_as::<_, ContactRecord>(select_record!("WHERE p.id = ?"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Contact introuvable".into()))?;
    let tags = tags_of(conn, id).await?;
    let children_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partners WHERE parent_id = ? AND archived_at IS NULL")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let display_name = display_name(&record.kind, &record.name, &record.first_name);
    Ok(ContactDetail { record, display_name, tags, children_count })
}

/// Vue figée d'un contact pour le journal.
// Vue lisible pour le journal (noms plutôt que ids).
fn audit_view(d: &ContactDetail) -> Value {
    let mut v = json!(d.record);
    if let Value::Object(m) = &mut v {
        for k in ["id", "created_at", "updated_at", "parent_name", "is_demo"] {
            m.remove(k);
        }
        m.insert("tags".into(), json!(d.tags.iter().map(|t| t.name.clone()).collect::<Vec<_>>()));
    }
    v
}

// Écrit tous les champs modifiables puis recalcule la clé de recherche.
async fn write_fields(conn: &mut SqliteConnection, id: i64, i: &ContactInput, user_id: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE partners SET kind=?, parent_id=?, name=?, first_name=?, is_client=?, is_prospect=?, is_stagiaire=?,
            job_title=?, email=?, phone=?, mobile=?, street=?, street2=?, zip=?, city=?, country=?, website=?, notes=?,
            siren=?, siret=?, vat_number=?, duns=?, updated_by=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(&i.kind).bind(i.parent_id).bind(&i.name).bind(&i.first_name).bind(i.is_client).bind(i.is_prospect)
    .bind(i.is_stagiaire).bind(&i.job_title).bind(&i.email).bind(&i.phone).bind(&i.mobile).bind(&i.street)
    .bind(&i.street2).bind(&i.zip).bind(&i.city).bind(&i.country).bind(&i.website).bind(&i.notes).bind(&i.siren)
    .bind(&i.siret).bind(&i.vat_number).bind(&i.duns).bind(user_id).bind(id)
    .execute(&mut *conn)
    .await?;
    set_tags(conn, id, &i.tag_ids).await?;
    assign_customer_number(conn, id).await?;
    refresh_search(conn, id).await?;
    Ok(())
}

/// Création dans une transaction existante (saisie déjà normalisée et validée).
// Création dans une transaction ouverte par l'appelant (utilisée aussi par l'import et la démo).
pub async fn insert_in_tx(conn: &mut SqliteConnection, user_id: i64, i: &ContactInput) -> AppResult<i64> {
    check_relations(conn, None, i).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO partners (kind, name, created_by, updated_by) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(&i.kind)
    .bind(&i.name)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await?;
    write_fields(conn, id, i, user_id).await?;
    let d = load_detail(conn, id).await?;
    audit::record(conn, Some(user_id), "CREATION", ENTITY, Some(id.to_string()), Some(audit_view(&d))).await?;
    Ok(id)
}

// Création complète : validation, transaction, numéro client éventuel, étiquettes, audit.
pub async fn create(pool: &SqlitePool, user_id: i64, mut i: ContactInput) -> AppResult<ContactDetail> {
    normalize_and_validate(&mut i)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let id = insert_in_tx(&mut tx, user_id, &i).await?;
    let d = load_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

// Modification : seules les différences sont journalisées.
pub async fn update(pool: &SqlitePool, user_id: i64, id: i64, mut i: ContactInput) -> AppResult<ContactDetail> {
    normalize_and_validate(&mut i)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = load_detail(&mut tx, id).await?;
    check_relations(&mut tx, Some(id), &i).await?;
    write_fields(&mut tx, id, &i, user_id).await?;
    let after = load_detail(&mut tx, id).await?;
    let changes = audit::diff(&audit_view(&before), &audit_view(&after));
    if changes.as_object().is_some_and(|m| !m.is_empty()) {
        audit::record(&mut tx, Some(user_id), "MODIFICATION", ENTITY, Some(id.to_string()), Some(changes)).await?;
    }
    tx.commit().await?;
    Ok(after)
}

// Archivage / désarchivage (le contact reste référençable par l'historique).
pub async fn set_archived(pool: &SqlitePool, user_id: i64, id: i64, archived: bool) -> AppResult<ContactDetail> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = load_detail(&mut tx, id).await?;
    if before.record.archived_at.is_some() == archived {
        return Ok(before);
    }
    sqlx::query(
        "UPDATE partners SET archived_at = CASE WHEN ? THEN strftime('%Y-%m-%dT%H:%M:%SZ', 'now') ELSE NULL END,
            updated_by = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
    )
    .bind(archived)
    .bind(user_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let action = if archived { "ARCHIVAGE" } else { "DESARCHIVAGE" };
    audit::record(&mut tx, Some(user_id), action, ENTITY, Some(id.to_string()), None).await?;
    let after = load_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(after)
}

// Suppression physique, refusée si des personnes y sont rattachées ou si des données l'utilisent.
pub async fn delete(pool: &SqlitePool, user_id: i64, id: i64) -> AppResult<()> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let d = load_detail(&mut tx, id).await?;
    let children: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partners WHERE parent_id = ?").bind(id).fetch_one(&mut *tx).await?;
    if children > 0 {
        return Err(AppError::Conflict(format!(
            "Impossible de supprimer « {} » : {children} contact(s) y sont rattachés. Archivez-le plutôt.",
            d.display_name
        )));
    }
    sqlx::query("DELETE FROM partners WHERE id = ?").bind(id).execute(&mut *tx).await.map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict(format!(
            "Impossible de supprimer « {} » : il est utilisé ailleurs (missions, temps ou factures). Archivez-le plutôt.",
            d.display_name
        )),
        other => other,
    })?;
    audit::record(&mut tx, Some(user_id), "SUPPRESSION", ENTITY, Some(id.to_string()), Some(audit_view(&d))).await?;
    tx.commit().await?;
    Ok(())
}

// Copie d'un contact sous le nom "... (copie)", notes exclues.
pub async fn duplicate(pool: &SqlitePool, user_id: i64, id: i64) -> AppResult<ContactDetail> {
    let src = {
        let mut conn = pool.acquire().await?;
        load_detail(&mut conn, id).await?
    };
    let r = src.record;
    let input = ContactInput {
        kind: r.kind,
        parent_id: r.parent_id,
        name: format!("{} (copie)", r.name),
        first_name: r.first_name,
        is_client: r.is_client,
        is_prospect: r.is_prospect,
        is_stagiaire: r.is_stagiaire,
        job_title: r.job_title,
        email: r.email,
        phone: r.phone,
        mobile: r.mobile,
        street: r.street,
        street2: r.street2,
        zip: r.zip,
        city: r.city,
        country: r.country,
        website: r.website,
        notes: String::new(),
        siren: r.siren,
        siret: r.siret,
        vat_number: r.vat_number,
        duns: r.duns,
        tag_ids: src.tags.iter().map(|t| t.id).collect(),
    };
    create(pool, user_id, input).await
}

// Traduit les filtres de liste en SQL paramétré.
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &ContactFilters) {
    match f.archived.as_deref() {
        Some("archives") => qb.push(" AND p.archived_at IS NOT NULL"),
        Some("tous") => qb.push(""),
        _ => qb.push(" AND p.archived_at IS NULL"),
    };
    if let Some(q) = f.q.as_deref() {
        for word in search_key(q).split(' ').filter(|w| !w.is_empty()) {
            let escaped = word.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            qb.push(" AND p.search_text LIKE ").push_bind(format!("%{escaped}%")).push(" ESCAPE '\\'");
        }
    }
    if let Some(k) = f.kind.as_deref().filter(|k| KINDS.contains(k)) {
        qb.push(" AND p.kind = ").push_bind(k);
    }
    if f.companies_only == Some(true) {
        qb.push(" AND p.kind IN ('ENTREPRISE', 'ORGANISME')");
    }
    match f.role.as_deref() {
        Some("client") => qb.push(" AND p.is_client = 1"),
        Some("prospect") => qb.push(" AND p.is_prospect = 1"),
        Some("stagiaire") => qb.push(" AND p.is_stagiaire = 1"),
        _ => qb.push(""),
    };
    if let Some(t) = f.tag_id {
        qb.push(" AND EXISTS (SELECT 1 FROM partner_tags pt WHERE pt.partner_id = p.id AND pt.tag_id = ").push_bind(t).push(")");
    }
    if let Some(pid) = f.parent_id {
        qb.push(" AND p.parent_id = ").push_bind(pid);
    }
}

// Clause ORDER BY selon le tri demandé (préfixe "-" = décroissant).
fn order_by(sort: Option<&str>) -> &'static str {
    match sort.unwrap_or("name") {
        "-name" => " ORDER BY COALESCE(pp.name, p.name) COLLATE NOCASE DESC, p.parent_id IS NOT NULL, p.name COLLATE NOCASE DESC, p.first_name COLLATE NOCASE DESC",
        "city" => " ORDER BY p.city COLLATE NOCASE, p.name COLLATE NOCASE",
        "-city" => " ORDER BY p.city COLLATE NOCASE DESC, p.name COLLATE NOCASE",
        "country" => " ORDER BY p.country COLLATE NOCASE, p.name COLLATE NOCASE",
        "-country" => " ORDER BY p.country COLLATE NOCASE DESC, p.name COLLATE NOCASE",
        "email" => " ORDER BY p.email COLLATE NOCASE, p.name COLLATE NOCASE",
        "-email" => " ORDER BY p.email COLLATE NOCASE DESC, p.name COLLATE NOCASE",
        "created" => " ORDER BY p.created_at, p.id",
        "-created" => " ORDER BY p.created_at DESC, p.id DESC",
        // Comme Odoo : tri sur le nom affiché ("SOCIÉTÉ, Personne"), la société avant ses interlocuteurs.
        _ => " ORDER BY COALESCE(pp.name, p.name) COLLATE NOCASE, p.parent_id IS NOT NULL, p.name COLLATE NOCASE, p.first_name COLLATE NOCASE",
    }
}

// Colonnes de la liste (jointure sur la société parente pour l'affichage "SOCIÉTÉ, Personne").
const LIST_COLUMNS: &str = "SELECT p.id, p.kind, p.name, p.first_name, p.parent_id, pp.name AS parent_name, p.email, p.phone,
    p.mobile, p.city, p.country, p.is_client, p.is_prospect, p.is_stagiaire, p.customer_number, p.archived_at
    FROM partners p LEFT JOIN partners pp ON pp.id = p.parent_id WHERE 1 = 1";

// Liste paginée : total, page triée, puis étiquettes et nom d'affichage par ligne.
pub async fn list(pool: &SqlitePool, f: &ContactFilters, limit: i64, offset: i64) -> AppResult<(Vec<ContactListItem>, i64)> {
    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM partners p WHERE 1 = 1");
    push_filters(&mut count, f);
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;

    let mut qb = QueryBuilder::<Sqlite>::new(LIST_COLUMNS);
    push_filters(&mut qb, f);
    qb.push(order_by(f.sort.as_deref()));
    qb.push(" LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
    let rows: Vec<ContactListRow> = qb.build_query_as().fetch_all(pool).await?;

    let mut conn = pool.acquire().await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let tags = tags_of(&mut conn, row.id).await?;
        let base = display_name(&row.kind, &row.name, &row.first_name);
        let display_name = match &row.parent_name {
            Some(p) => format!("{p}, {base}"),
            None => base,
        };
        items.push(ContactListItem { row, display_name, tags });
    }
    Ok((items, total))
}

// Encadre une cellule CSV de guillemets et double les guillemets internes.
fn csv_cell(s: &str) -> String {
    // Protection contre l'injection de formules dans Excel.
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", s.replace('"', "\"\""))
}

// Export CSV (BOM UTF-8 + séparateur ; pour une ouverture directe dans Excel).
pub async fn export_csv(pool: &SqlitePool, f: &ContactFilters) -> AppResult<String> {
    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT p.id, p.kind, p.parent_id, pp.name AS parent_name, p.name, p.first_name, p.is_client, p.is_prospect,
            p.is_stagiaire, p.job_title, p.email, p.phone, p.mobile, p.street, p.street2, p.zip, p.city, p.country,
            p.website, p.notes, p.customer_number, p.siren, p.siret, p.vat_number, p.duns, p.archived_at, p.is_demo,
            p.created_at, p.updated_at
         FROM partners p LEFT JOIN partners pp ON pp.id = p.parent_id WHERE 1 = 1",
    );
    push_filters(&mut qb, f);
    qb.push(order_by(f.sort.as_deref()));
    qb.push(" LIMIT 50000");
    let rows: Vec<ContactRecord> = qb.build_query_as().fetch_all(pool).await?;
    let mut out = String::from("\u{feff}");
    out.push_str("N° client;Type;Nom;Prénom;Société;Fonction;Email;Téléphone;Mobile;Adresse;Complément;Code postal;Ville;Pays;Site web;SIREN;SIRET;TVA;Client;Prospect;Stagiaire;Archivé\r\n");
    let yn = |b: bool| if b { "oui" } else { "non" };
    for r in rows {
        let cells = [
            r.customer_number.clone().unwrap_or_default(), r.kind.clone(), r.name.clone(), r.first_name.clone(),
            r.parent_name.clone().unwrap_or_default(), r.job_title.clone(), r.email.clone(), r.phone.clone(),
            r.mobile.clone(), r.street.clone(), r.street2.clone(), r.zip.clone(), r.city.clone(), r.country.clone(),
            r.website.clone(), r.siren.clone(), r.siret.clone(), r.vat_number.clone(), yn(r.is_client).into(),
            yn(r.is_prospect).into(), yn(r.is_stagiaire).into(), yn(r.archived_at.is_some()).into(),
        ];
        out.push_str(&cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
    }
    Ok(out)
}

// Ajoute une note libre à un contact.
pub async fn add_note(pool: &SqlitePool, user_id: i64, id: i64, body: &str) -> AppResult<Note> {
    let body = body.trim();
    if body.is_empty() {
        return Err(AppError::Validation("La note est vide".into()));
    }
    let mut tx = pool.begin().await?;
    load_detail(&mut tx, id).await?;
    let note_id: i64 = sqlx::query_scalar("INSERT INTO partner_notes (partner_id, author_id, body) VALUES (?, ?, ?) RETURNING id")
        .bind(id)
        .bind(user_id)
        .bind(body)
        .fetch_one(&mut *tx)
        .await?;
    let note = sqlx::query_as::<_, Note>(
        "SELECT n.id, u.display_name AS author_name, n.body, n.created_at FROM partner_notes n LEFT JOIN users u ON u.id = n.author_id WHERE n.id = ?",
    )
    .bind(note_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(note)
}

// Notes et historique d'audit du contact (les plus récents d'abord).
pub async fn activity(pool: &SqlitePool, id: i64) -> AppResult<(Vec<Note>, Vec<HistoryEntry>)> {
    let notes = sqlx::query_as::<_, Note>(
        "SELECT n.id, u.display_name AS author_name, n.body, n.created_at FROM partner_notes n
         LEFT JOIN users u ON u.id = n.author_id WHERE n.partner_id = ? ORDER BY n.id DESC",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    let history = sqlx::query_as::<_, HistoryEntry>(
        "SELECT a.id, u.display_name AS user_name, a.action, a.changes, a.created_at FROM audit_log a
         LEFT JOIN users u ON u.id = a.user_id WHERE a.entity_type = ? AND a.entity_id = ? ORDER BY a.id DESC LIMIT 200",
    )
    .bind(ENTITY)
    .bind(id.to_string())
    .fetch_all(pool)
    .await?;
    Ok((notes, history))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person() -> ContactInput {
        ContactInput { kind: "PARTICULIER".into(), name: "  MARTIN ".into(), first_name: "Jean".into(), ..Default::default() }
    }

    #[test]
    fn nettoyage() {
        let mut i = person();
        i.email = " Jean.Martin@Demo.FR ".into();
        i.siret = "944 642 917 00016".into();
        normalize_and_validate(&mut i).unwrap();
        assert_eq!(i.name, "MARTIN");
        assert_eq!(i.email, "jean.martin@demo.fr");
        assert_eq!(i.siret, "94464291700016");
        assert_eq!(i.country, "France");
    }

    #[test]
    fn controles() {
        let mut i = person();
        i.name = " ".into();
        assert!(normalize_and_validate(&mut i).is_err());
        let mut i = person();
        i.email = "faux".into();
        assert!(normalize_and_validate(&mut i).is_err());
        let mut i = person();
        i.is_client = true;
        i.is_prospect = true;
        assert!(normalize_and_validate(&mut i).is_err());
        let mut i = ContactInput { kind: "ENTREPRISE".into(), name: "SENZA".into(), parent_id: Some(1), ..Default::default() };
        assert!(normalize_and_validate(&mut i).is_err());
        let mut i = ContactInput { kind: "AUTRE".into(), name: "X".into(), ..Default::default() };
        assert!(normalize_and_validate(&mut i).is_err());
    }

    #[test]
    fn societe_sans_prenom() {
        let mut i = ContactInput { kind: "ENTREPRISE".into(), name: "SENZA".into(), first_name: "x".into(), ..Default::default() };
        normalize_and_validate(&mut i).unwrap();
        assert_eq!(i.first_name, "");
    }

    #[test]
    fn csv_protege_les_formules() {
        assert_eq!(csv_cell("=1+1"), "\"'=1+1\"");
        assert_eq!(csv_cell("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn noms_affiches() {
        assert_eq!(display_name("PARTICULIER", "MARTIN", "Jean"), "MARTIN Jean");
        assert_eq!(display_name("ENTREPRISE", "SENZA", ""), "SENZA");
    }
}
