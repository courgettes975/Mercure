//! Règles métier des missions de formation.

use serde_json::{json, Value};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use super::model::*;
use crate::{
    domain::{
        money::Money,
        validation::{normalize_spaces, parse_date, parse_hhmm, search_key},
    },
    error::{AppError, AppResult},
    modules::{audit, projects, sequence},
};

// Type d'entité dans le journal d'audit.
pub const ENTITY: &str = "mission";
/// Tarif horaire proposé par défaut : 50,00 EUR (centimes).
pub const DEFAULT_HOURLY_RATE: i64 = 5_000;

// Lecture complète d'une mission avec toutes ses jointures (client, contact, centre, formateur, logiciel).
macro_rules! select_mission {
    ($tail:literal) => {
        concat!(
            "SELECT m.id, m.project_id, m.reference, m.status, p.name AS title, p.client_id,
                TRIM(c.name || ' ' || c.first_name) AS client_name, m.contact_id,
                TRIM(ct.name || ' ' || ct.first_name) AS contact_name, m.center_id, ce.name AS center_name,
                m.trainer_id, u.display_name AS trainer_name,
                m.software_id, s.name AS software_name, p.description, p.start_date, p.end_date, m.os, m.modality,
                m.group_format, m.provide_pc, m.recording, m.planned_minutes, m.days_count, m.schedule_text, m.address,
                m.room, m.visio_link, m.pricing_mode, p.hourly_rate, m.daily_rate, m.flat_fee, p.billable,
                m.special_conditions, m.internal_notes, m.annotations, m.file_created, m.first_call_done, m.invoice_sent,
                m.objectives, m.prerequisites, m.program, m.methods, m.means, m.evaluation_terms, m.evaluation_result,
                m.satisfaction, m.trainer_comment, p.archived_at
             FROM missions m JOIN projects p ON p.id = m.project_id JOIN partners c ON c.id = p.client_id
             LEFT JOIN partners ct ON ct.id = m.contact_id LEFT JOIN partners ce ON ce.id = m.center_id
             LEFT JOIN users u ON u.id = m.trainer_id
             LEFT JOIN software s ON s.id = m.software_id ",
            $tail
        )
    };
}

// Raccourci : erreur de validation.
fn err<T>(m: &str) -> AppResult<T> {
    Err(AppError::Validation(m.into()))
}

// Nettoie la saisie, applique les valeurs par défaut (présentiel, individuel, horaire, 50 EUR/h) et contrôle.
pub fn normalize(i: &mut MissionInput) -> AppResult<()> {
    i.title = normalize_spaces(&i.title);
    for f in [&mut i.schedule_text, &mut i.address, &mut i.room, &mut i.visio_link] {
        *f = f.trim().to_string();
    }
    i.start_date = i.start_date.take().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    i.end_date = i.end_date.take().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    if i.modality.is_empty() {
        i.modality = "PRESENTIEL".into();
    }
    if i.group_format.is_empty() {
        i.group_format = "INDIVIDUEL".into();
    }
    if i.pricing_mode.is_empty() {
        i.pricing_mode = "HORAIRE".into();
    }
    if i.pricing_mode == "HORAIRE" && i.hourly_rate == 0 {
        i.hourly_rate = DEFAULT_HOURLY_RATE;
    }
    if i.title.is_empty() {
        return err("L'intitulé de la formation est obligatoire");
    }
    if i.client_id <= 0 {
        return err("Le client est obligatoire");
    }
    if !["", "PC", "MAC"].contains(&i.os.as_str()) {
        return err("OS inconnu");
    }
    if !["PRESENTIEL", "DISTANCIEL", "HYBRIDE"].contains(&i.modality.as_str()) {
        return err("Modalité inconnue");
    }
    if !["INDIVIDUEL", "GROUPE"].contains(&i.group_format.as_str()) {
        return err("Format inconnu");
    }
    if !["HORAIRE", "JOURNALIER", "FORFAIT"].contains(&i.pricing_mode.as_str()) {
        return err("Mode de tarification inconnu");
    }
    for d in [&i.start_date, &i.end_date].into_iter().flatten() {
        if parse_date(d).is_none() {
            return err("Date invalide (format AAAA-MM-JJ)");
        }
    }
    if let (Some(s), Some(e)) = (&i.start_date, &i.end_date)
        && e < s {
            return err("La date de fin est antérieure à la date de début");
        }
    if [i.planned_minutes, i.days_count, i.hourly_rate, i.daily_rate, i.flat_fee].iter().any(|v| *v < 0) {
        return err("Les durées et tarifs doivent être positifs");
    }
    Ok(())
}

// La mission est portée par un projet : construit les champs du projet à partir de la mission.
fn project_input(i: &MissionInput, status: &str) -> projects::ProjectInput {
    projects::ProjectInput {
        name: i.title.clone(),
        client_id: i.client_id,
        description: i.description.clone(),
        start_date: i.start_date.clone(),
        end_date: i.end_date.clone(),
        status: project_status(status).into(),
        billable: i.billable.unwrap_or(true),
        hourly_rate: i.hourly_rate,
        member_ids: i.trainer_id.into_iter().collect(),
    }
}

// Correspondance statut mission -> statut projet.
fn project_status(mission_status: &str) -> &'static str {
    match mission_status {
        "BROUILLON" => "BROUILLON",
        "ANNULEE" => "ANNULE",
        "REALISEE" | "A_FACTURER" | "FACTUREE" => "TERMINE",
        _ => "EN_COURS",
    }
}

// Vérifie l'existence des références (contact, centre, formateur, logiciel) ; marque le centre dans la liste.
async fn check_refs(conn: &mut SqliteConnection, i: &MissionInput) -> AppResult<()> {
    if let Some(c) = i.contact_id {
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM partners WHERE id = ?").bind(c).fetch_optional(&mut *conn).await?;
        if ok.is_none() {
            return err("Contact principal introuvable");
        }
    }
    if let Some(c) = i.center_id {
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM partners WHERE id = ? AND kind IN ('ENTREPRISE', 'ORGANISME')").bind(c).fetch_optional(&mut *conn).await?;
        if ok.is_none() {
            return err("Centre de formation introuvable (doit être une société ou un organisme des contacts)");
        }
        // Un centre choisi hors liste y est ajouté automatiquement.
        sqlx::query("UPDATE partners SET is_training_center = 1 WHERE id = ? AND is_training_center = 0").bind(c).execute(&mut *conn).await?;
    }
    if let Some(t) = i.trainer_id {
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE id = ?").bind(t).fetch_optional(&mut *conn).await?;
        if ok.is_none() {
            return err("Formateur introuvable");
        }
    }
    if let Some(s) = i.software_id {
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM software WHERE id = ?").bind(s).fetch_optional(&mut *conn).await?;
        if ok.is_none() {
            return err("Logiciel introuvable");
        }
    }
    Ok(())
}

// Écrit les colonnes propres à la mission (le projet est écrit à part).
async fn write_mission(conn: &mut SqliteConnection, id: i64, i: &MissionInput) -> AppResult<()> {
    sqlx::query(
        "UPDATE missions SET contact_id=?, center_id=?, invoice_sent=?, annotations=?, trainer_id=?, software_id=?, os=?, modality=?, group_format=?, provide_pc=?,
            recording=?, planned_minutes=?, days_count=?, schedule_text=?, address=?, room=?, visio_link=?,
            pricing_mode=?, daily_rate=?, flat_fee=?, special_conditions=?, internal_notes=?, file_created=?,
            first_call_done=?, objectives=?, prerequisites=?, program=?, methods=?, means=?, evaluation_terms=?,
            evaluation_result=?, satisfaction=?, trainer_comment=?
         WHERE id = ?",
    )
    .bind(i.contact_id).bind(i.center_id).bind(i.invoice_sent).bind(&i.annotations).bind(i.trainer_id).bind(i.software_id).bind(&i.os).bind(&i.modality).bind(&i.group_format)
    .bind(i.provide_pc).bind(i.recording).bind(i.planned_minutes).bind(i.days_count).bind(&i.schedule_text)
    .bind(&i.address).bind(&i.room).bind(&i.visio_link).bind(&i.pricing_mode).bind(i.daily_rate).bind(i.flat_fee)
    .bind(&i.special_conditions).bind(&i.internal_notes).bind(i.file_created).bind(i.first_call_done)
    .bind(&i.objectives).bind(&i.prerequisites).bind(&i.program).bind(&i.methods).bind(&i.means)
    .bind(&i.evaluation_terms).bind(&i.evaluation_result).bind(&i.satisfaction).bind(&i.trainer_comment).bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

// Lit l'enregistrement brut d'une mission.
pub async fn record(conn: &mut SqliteConnection, id: i64) -> AppResult<MissionRecord> {
    sqlx::query_as::<_, MissionRecord>(select_mission!("WHERE m.id = ?"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Mission introuvable".into()))
}

// Vue journalisée : l'enregistrement sans les identifiants techniques.
fn audit_view(r: &MissionRecord) -> Value {
    let mut v = json!(r);
    if let Value::Object(m) = &mut v {
        for k in ["id", "project_id", "client_id", "contact_id", "trainer_id", "software_id", "archived_at"] {
            m.remove(k);
        }
    }
    v
}

// Fiche complète : sessions, participants (avec présences cumulées), frais, documents, factures et totaux.
pub async fn load(conn: &mut SqliteConnection, id: i64) -> AppResult<MissionDetail> {
    let record = record(conn, id).await?;
    let sessions = sqlx::query_as::<_, Session>(
        "SELECT s.id, s.mission_id, s.date, s.start_time, s.end_time, s.break_minutes, s.duration_minutes, s.trainer_id,
            u.display_name AS trainer_name, s.location, s.status, s.notes,
            (SELECT t.id FROM timesheets t WHERE t.session_id = s.id LIMIT 1) AS timesheet_id,
            EXISTS (SELECT 1 FROM timesheets t WHERE t.session_id = s.id AND t.invoice_line_id IS NOT NULL) AS invoiced
         FROM training_sessions s LEFT JOIN users u ON u.id = s.trainer_id WHERE s.mission_id = ? ORDER BY s.date, s.start_time",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let participants = sqlx::query_as::<_, Participant>(
        "SELECT mp.id, mp.partner_id, p.name, p.first_name, pp.name AS company, p.email,
            CASE WHEN p.mobile <> '' THEN p.mobile ELSE p.phone END AS phone, p.job_title, mp.progress_status,
            mp.evaluation, mp.comment,
            COALESCE((SELECT SUM(s.duration_minutes) FROM session_attendance a JOIN training_sessions s ON s.id = a.session_id
                      WHERE a.participant_id = mp.id AND a.present = 1 AND s.status = 'REALISEE'), 0) AS minutes_done,
            (SELECT COUNT(*) FROM session_attendance a JOIN training_sessions s ON s.id = a.session_id
                      WHERE a.participant_id = mp.id AND a.present = 1 AND s.status = 'REALISEE') AS sessions_present,
            (SELECT COUNT(*) FROM session_attendance a JOIN training_sessions s ON s.id = a.session_id
                      WHERE a.participant_id = mp.id AND a.present = 0 AND s.status = 'REALISEE') AS sessions_absent
         FROM mission_participants mp JOIN partners p ON p.id = mp.partner_id LEFT JOIN partners pp ON pp.id = p.parent_id
         WHERE mp.mission_id = ? ORDER BY p.name COLLATE NOCASE, p.first_name COLLATE NOCASE",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let attendance = sqlx::query_as::<_, Attendance>(
        "SELECT a.session_id, a.participant_id, a.present FROM session_attendance a JOIN training_sessions s ON s.id = a.session_id WHERE s.mission_id = ?",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let expenses = sqlx::query_as::<_, Expense>(
        "SELECT id, kind, label, expense_date, quantity_milli, unit_price, amount, billable, invoice_line_id IS NOT NULL AS invoiced
         FROM mission_expenses WHERE mission_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let documents = sqlx::query_as::<_, Document>(
        "SELECT d.id, d.category, d.file_name, d.mime_type, d.size, u.display_name AS uploaded_by_name, d.created_at
         FROM documents d LEFT JOIN users u ON u.id = d.uploaded_by WHERE d.owner_type = 'MISSION' AND d.owner_id = ? ORDER BY d.id DESC",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let invoices = sqlx::query_as::<_, InvoiceRef>(
        "SELECT id, number, kind, status, invoice_date, total_ttc FROM invoices WHERE project_id = ? ORDER BY id",
    )
    .bind(record.project_id)
    .fetch_all(&mut *conn)
    .await?;

    // Totaux : prévu (selon le mode de tarification), réalisé (temps), facturé HT, reste à facturer.
    let expenses_total: i64 = expenses.iter().map(|e| e.amount).sum();
    let planned = planned_fee(&record.pricing_mode, record.planned_minutes, record.days_count, record.hourly_rate, record.daily_rate, record.flat_fee);
    let (realized_minutes, realized_amount, uninvoiced_ts): (i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(duration_minutes), 0), COALESCE(SUM(amount), 0),
                COALESCE(SUM(CASE WHEN invoice_line_id IS NULL AND billable = 1 THEN amount ELSE 0 END), 0)
         FROM timesheets WHERE project_id = ?",
    )
    .bind(record.project_id)
    .fetch_one(&mut *conn)
    .await?;
    let invoiced_ht: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CASE WHEN kind = 'AVOIR' THEN -total_ht ELSE total_ht END), 0) FROM invoices
         WHERE project_id = ? AND status IN ('VALIDEE', 'PAYEE', 'ANNULEE')",
    )
    .bind(record.project_id)
    .fetch_one(&mut *conn)
    .await?;
    let uninvoiced_exp: i64 = expenses.iter().filter(|e| e.billable && !e.invoiced).map(|e| e.amount).sum();
    let forfait_pending = if record.pricing_mode == "FORFAIT" && !forfait_invoiced(conn, record.project_id).await? { record.flat_fee } else { 0 };
    let totals = Totals {
        planned_fee: planned,
        expenses_total,
        planned_total: planned + expenses_total,
        sessions_minutes: sessions.iter().filter(|s| s.status != "ANNULEE").map(|s| s.duration_minutes).sum(),
        realized_minutes,
        realized_amount: if record.pricing_mode == "FORFAIT" { record.flat_fee } else { realized_amount },
        invoiced_ht,
        uninvoiced_amount: uninvoiced_ts + uninvoiced_exp + forfait_pending,
    };
    Ok(MissionDetail { record, sessions, participants, attendance, expenses, documents, invoices, totals })
}

/// Le forfait a-t-il déjà été facturé (facture non annulée) ?
pub async fn forfait_invoiced(conn: &mut SqliteConnection, project_id: i64) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invoice_lines l JOIN invoices i ON i.id = l.invoice_id
         WHERE i.project_id = ? AND l.source_type = 'FORFAIT' AND i.kind = 'FACTURE' AND i.status <> 'ANNULEE'",
    )
    .bind(project_id)
    .fetch_one(conn)
    .await?;
    Ok(n > 0)
}

// Création : projet FORMATION (référence PRJ-) puis mission (référence MF-AAAA-NNNN), dans une même transaction.
pub async fn create(pool: &SqlitePool, user_id: i64, mut i: MissionInput) -> AppResult<MissionDetail> {
    normalize(&mut i)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_refs(&mut tx, &i).await?;
    let project_id = projects::insert(&mut tx, user_id, "FORMATION", &project_input(&i, "BROUILLON")).await?;
    let year = chrono::Local::now().format("%Y").to_string();
    let n = sequence::next_value(&mut tx, &format!("MF-{year}")).await?;
    let reference = sequence::format_reference("MF", year.parse().unwrap_or(2026), n, 4);
    let id: i64 = sqlx::query_scalar("INSERT INTO missions (project_id, reference) VALUES (?, ?) RETURNING id")
        .bind(project_id)
        .bind(&reference)
        .fetch_one(&mut *tx)
        .await?;
    write_mission(&mut tx, id, &i).await?;
    let r = record(&mut tx, id).await?;
    audit::record(&mut tx, Some(user_id), "CREATION", ENTITY, Some(id.to_string()), Some(audit_view(&r))).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

// Modification ; les tarifs sont verrouillés dès qu'une facture non annulée existe.
pub async fn update(pool: &SqlitePool, user_id: i64, id: i64, mut i: MissionInput) -> AppResult<MissionDetail> {
    normalize(&mut i)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = record(&mut tx, id).await?;
    check_refs(&mut tx, &i).await?;
    let has_invoices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE project_id = ? AND status <> 'ANNULEE'")
        .bind(before.project_id)
        .fetch_one(&mut *tx)
        .await?;
    if has_invoices > 0
        && (before.pricing_mode != i.pricing_mode || before.hourly_rate != i.hourly_rate || before.daily_rate != i.daily_rate || before.flat_fee != i.flat_fee)
    {
        return err("Tarifs verrouillés : une facture existe déjà pour cette mission. Annulez-la (avoir) avant de modifier les tarifs.");
    }
    projects::update_in_tx(&mut tx, user_id, before.project_id, &project_input(&i, &before.status)).await?;
    write_mission(&mut tx, id, &i).await?;
    // Les temps non facturés suivent le nouveau tarif.
    refresh_uninvoiced_rates(&mut tx, id).await?;
    let after = record(&mut tx, id).await?;
    let changes = audit::diff(&audit_view(&before), &audit_view(&after));
    if changes.as_object().is_some_and(|m| !m.is_empty()) {
        audit::record(&mut tx, Some(user_id), "MODIFICATION", ENTITY, Some(id.to_string()), Some(changes)).await?;
    }
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

// Recalcule montant et facturabilité des temps de session non facturés après un changement de tarif.
async fn refresh_uninvoiced_rates(conn: &mut SqliteConnection, mission_id: i64) -> AppResult<()> {
    let r = record(conn, mission_id).await?;
    let rate = effective_hourly_rate(&r.pricing_mode, r.hourly_rate, r.daily_rate);
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT t.id, t.duration_minutes FROM timesheets t JOIN training_sessions s ON s.id = t.session_id
         WHERE s.mission_id = ? AND t.invoice_line_id IS NULL",
    )
    .bind(mission_id)
    .fetch_all(&mut *conn)
    .await?;
    for (tid, minutes) in rows {
        sqlx::query("UPDATE timesheets SET hourly_rate = ?, amount = ?, billable = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
            .bind(rate)
            .bind(Money::for_minutes(Money(rate), minutes).0)
            .bind(r.billable && r.pricing_mode != "FORFAIT")
            .bind(tid)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

// Changement de statut manuel avec garde-fous liés à la facturation.
pub async fn set_status(pool: &SqlitePool, user_id: i64, id: i64, status: &str) -> AppResult<MissionDetail> {
    if !STATUSES.contains(&status) {
        return err("Statut inconnu");
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let r = record(&mut tx, id).await?;
    if r.status == status {
        let d = load(&mut tx, id).await?;
        return Ok(d);
    }
    let validated: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE project_id = ? AND kind = 'FACTURE' AND status IN ('VALIDEE', 'PAYEE')")
        .bind(r.project_id)
        .fetch_one(&mut *tx)
        .await?;
    if status == "ANNULEE" && validated > 0 {
        return err("Mission facturée : établissez d'abord un avoir avant de l'annuler");
    }
    if r.status == "FACTUREE" && validated > 0 && !["FACTUREE", "A_FACTURER"].contains(&status) {
        return err("Mission facturée : son statut ne peut plus revenir en arrière");
    }
    sqlx::query("UPDATE missions SET status = ? WHERE id = ?").bind(status).bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE projects SET status = ? WHERE id = ?").bind(project_status(status)).bind(r.project_id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "CHANGEMENT_STATUT", ENTITY, Some(id.to_string()), Some(json!({ "status": { "old": r.status, "new": status } }))).await?;
    let d = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(d)
}

// Suppression physique uniquement si aucun temps ni facture (sinon statut Annulée).
pub async fn delete(pool: &SqlitePool, user_id: i64, id: i64) -> AppResult<()> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let r = record(&mut tx, id).await?;
    let used: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM invoices WHERE project_id = ?1) + (SELECT COUNT(*) FROM timesheets WHERE project_id = ?1)",
    )
    .bind(r.project_id)
    .fetch_one(&mut *tx)
    .await?;
    if used > 0 {
        return err("Impossible de supprimer : la mission a des temps ou des factures. Passez-la au statut « Annulée ».");
    }
    let docs: Vec<String> = sqlx::query_scalar("SELECT storage_name FROM documents WHERE owner_type = 'MISSION' AND owner_id = ?")
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM documents WHERE owner_type = 'MISSION' AND owner_id = ?").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM missions WHERE id = ?").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM projects WHERE id = ?").bind(r.project_id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "SUPPRESSION", ENTITY, Some(id.to_string()), Some(audit_view(&r))).await?;
    tx.commit().await?;
    let _ = docs; // fichiers supprimés par l'appelant (il connaît le dossier)
    Ok(())
}

// ---------- Sessions ----------

// Durée = fin - début - pause, en minutes, après contrôle des formats.
fn session_duration(i: &SessionInput) -> AppResult<i64> {
    if parse_date(&i.date).is_none() {
        return err("Date de session invalide");
    }
    let (Some(s), Some(e)) = (parse_hhmm(&i.start_time), parse_hhmm(&i.end_time)) else {
        return err("Heures invalides (format HH:MM)");
    };
    if e <= s {
        return err("L'heure de fin doit être après l'heure de début");
    }
    let d = e - s - i.break_minutes;
    if i.break_minutes < 0 || d <= 0 {
        return err("La pause est plus longue que la session");
    }
    Ok(d)
}

// Mission, statut et "déjà facturée ?" d'une session.
async fn session_owner(conn: &mut SqliteConnection, session_id: i64) -> AppResult<(i64, String, bool)> {
    let row: Option<(i64, String, bool)> = sqlx::query_as(
        "SELECT s.mission_id, s.status, EXISTS (SELECT 1 FROM timesheets t WHERE t.session_id = s.id AND t.invoice_line_id IS NOT NULL)
         FROM training_sessions s WHERE s.id = ?",
    )
    .bind(session_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.ok_or_else(|| AppError::NotFound("Session introuvable".into()))
}

// Ajout d'une séance ; la mission passe en PLANIFIEE si elle était à planifier.
pub async fn add_session(pool: &SqlitePool, user_id: i64, mission_id: i64, mut i: SessionInput) -> AppResult<MissionDetail> {
    let duration = session_duration(&i)?;
    i.location = i.location.trim().to_string();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let r = record(&mut tx, mission_id).await?;
    if r.status == "ANNULEE" {
        return err("Mission annulée");
    }
    let sid: i64 = sqlx::query_scalar(
        "INSERT INTO training_sessions (mission_id, date, start_time, end_time, break_minutes, duration_minutes, trainer_id, location, notes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(mission_id).bind(&i.date).bind(&i.start_time).bind(&i.end_time).bind(i.break_minutes).bind(duration)
    .bind(i.trainer_id.or(r.trainer_id)).bind(&i.location).bind(i.notes.trim())
    .fetch_one(&mut *tx)
    .await?;
    audit::record(&mut tx, Some(user_id), "AJOUT_SESSION", ENTITY, Some(mission_id.to_string()),
        Some(json!({ "session": sid, "date": i.date, "horaires": format!("{}-{}", i.start_time, i.end_time) }))).await?;
    if ["BROUILLON", "A_PLANIFIER"].contains(&r.status.as_str()) {
        sqlx::query("UPDATE missions SET status = 'PLANIFIEE' WHERE id = ?").bind(mission_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE projects SET status = 'EN_COURS' WHERE id = ?").bind(r.project_id).execute(&mut *tx).await?;
    }
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// Modification d'une séance ; si elle est réalisée, le temps généré est resynchronisé.
pub async fn update_session(pool: &SqlitePool, user_id: i64, session_id: i64, mut i: SessionInput) -> AppResult<MissionDetail> {
    let duration = session_duration(&i)?;
    i.location = i.location.trim().to_string();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (mission_id, status, invoiced) = session_owner(&mut tx, session_id).await?;
    if invoiced {
        return err("Session facturée : elle ne peut plus être modifiée");
    }
    let r = record(&mut tx, mission_id).await?;
    sqlx::query(
        "UPDATE training_sessions SET date=?, start_time=?, end_time=?, break_minutes=?, duration_minutes=?, trainer_id=?, location=?, notes=? WHERE id = ?",
    )
    .bind(&i.date).bind(&i.start_time).bind(&i.end_time).bind(i.break_minutes).bind(duration)
    .bind(i.trainer_id.or(r.trainer_id)).bind(&i.location).bind(i.notes.trim()).bind(session_id)
    .execute(&mut *tx)
    .await?;
    if status == "REALISEE" {
        sync_session_timesheet(&mut tx, session_id).await?;
    }
    audit::record(&mut tx, Some(user_id), "MODIFICATION_SESSION", ENTITY, Some(mission_id.to_string()),
        Some(json!({ "session": session_id, "date": i.date, "horaires": format!("{}-{}", i.start_time, i.end_time) }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// Suppression d'une séance non facturée (et de son temps éventuel).
pub async fn delete_session(pool: &SqlitePool, user_id: i64, session_id: i64) -> AppResult<MissionDetail> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (mission_id, _, invoiced) = session_owner(&mut tx, session_id).await?;
    if invoiced {
        return err("Session facturée : elle ne peut pas être supprimée");
    }
    sqlx::query("DELETE FROM timesheets WHERE session_id = ?").bind(session_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM training_sessions WHERE id = ?").bind(session_id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "SUPPRESSION_SESSION", ENTITY, Some(mission_id.to_string()), Some(json!({ "session": session_id }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

/// Crée ou met à jour la ligne de temps d'une session réalisée (une par formateur, jamais deux).
async fn sync_session_timesheet(conn: &mut SqliteConnection, session_id: i64) -> AppResult<()> {
    let (mission_id, date, duration, trainer_id): (i64, String, i64, Option<i64>) =
        sqlx::query_as("SELECT mission_id, date, duration_minutes, trainer_id FROM training_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_one(&mut *conn)
            .await?;
    let r = record(conn, mission_id).await?;
    let Some(user_id) = trainer_id.or(r.trainer_id) else {
        return err("Indiquez le formateur de la session (ou de la mission) avant de la marquer réalisée");
    };
    let rate = effective_hourly_rate(&r.pricing_mode, r.hourly_rate, r.daily_rate);
    let amount = Money::for_minutes(Money(rate), duration).0;
    let billable = r.billable && r.pricing_mode != "FORFAIT";
    let existing: Option<(i64, Option<i64>)> = sqlx::query_as("SELECT id, invoice_line_id FROM timesheets WHERE session_id = ?")
        .bind(session_id)
        .fetch_optional(&mut *conn)
        .await?;
    let description = format!("{} - session du {}", r.title, date);
    match existing {
        Some((_, Some(_))) => return err("Session facturée : le temps ne peut plus être modifié"),
        Some((tid, None)) => {
            sqlx::query(
                "UPDATE timesheets SET date=?, user_id=?, project_id=?, description=?, duration_minutes=?, billable=?, hourly_rate=?,
                    amount=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
            )
            .bind(&date).bind(user_id).bind(r.project_id).bind(&description).bind(duration).bind(billable).bind(rate).bind(amount).bind(tid)
            .execute(&mut *conn)
            .await?;
        }
        None => {
            sqlx::query(
                "INSERT INTO timesheets (date, user_id, project_id, session_id, description, duration_minutes, billable, hourly_rate, amount, status)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'VALIDE')",
            )
            .bind(&date).bind(user_id).bind(r.project_id).bind(session_id).bind(&description).bind(duration).bind(billable).bind(rate).bind(amount)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize, Default)]
#[serde(default)]
// Présences lors de la réalisation : on liste les absents, les autres sont présents.
pub struct RealizeInput {
    /// Participants absents (les autres sont présents).
    pub absent_participant_ids: Vec<i64>,
}

// Séance réalisée : présences enregistrées, temps du formateur créé, mission passée EN_COURS si besoin.
pub async fn realize_session(pool: &SqlitePool, user_id: i64, session_id: i64, input: RealizeInput) -> AppResult<MissionDetail> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (mission_id, _, invoiced) = session_owner(&mut tx, session_id).await?;
    if invoiced {
        return err("Session facturée : la présence ne peut plus être modifiée");
    }
    sqlx::query("UPDATE training_sessions SET status = 'REALISEE' WHERE id = ?").bind(session_id).execute(&mut *tx).await?;
    let participants: Vec<i64> = sqlx::query_scalar("SELECT id FROM mission_participants WHERE mission_id = ?")
        .bind(mission_id)
        .fetch_all(&mut *tx)
        .await?;
    for pid in &input.absent_participant_ids {
        if !participants.contains(pid) {
            return err("Participant inconnu pour cette mission");
        }
    }
    for pid in participants {
        sqlx::query(
            "INSERT INTO session_attendance (session_id, participant_id, present) VALUES (?, ?, ?)
             ON CONFLICT(session_id, participant_id) DO UPDATE SET present = excluded.present",
        )
        .bind(session_id)
        .bind(pid)
        .bind(!input.absent_participant_ids.contains(&pid))
        .execute(&mut *tx)
        .await?;
    }
    sync_session_timesheet(&mut tx, session_id).await?;
    let r = record(&mut tx, mission_id).await?;
    if ["BROUILLON", "A_PLANIFIER", "PLANIFIEE", "CONFIRMEE"].contains(&r.status.as_str()) {
        sqlx::query("UPDATE missions SET status = 'EN_COURS' WHERE id = ?").bind(mission_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE projects SET status = 'EN_COURS' WHERE id = ?").bind(r.project_id).execute(&mut *tx).await?;
    }
    audit::record(&mut tx, Some(user_id), "SESSION_REALISEE", ENTITY, Some(mission_id.to_string()),
        Some(json!({ "session": session_id, "absents": input.absent_participant_ids }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// Retour à PREVUE ou annulation : le temps et les présences générés sont supprimés.
pub async fn set_session_status(pool: &SqlitePool, user_id: i64, session_id: i64, status: &str) -> AppResult<MissionDetail> {
    if !["PREVUE", "ANNULEE"].contains(&status) {
        return err("Statut de session inconnu");
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (mission_id, _, invoiced) = session_owner(&mut tx, session_id).await?;
    if invoiced {
        return err("Session facturée : son statut ne peut plus changer");
    }
    sqlx::query("UPDATE training_sessions SET status = ? WHERE id = ?").bind(status).bind(session_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM timesheets WHERE session_id = ?").bind(session_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM session_attendance WHERE session_id = ?").bind(session_id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "STATUT_SESSION", ENTITY, Some(mission_id.to_string()), Some(json!({ "session": session_id, "status": status }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// ---------- Participants ----------

// Inscrit un contact (particulier) ; le marque stagiaire ; unicité par mission.
pub async fn add_participant(pool: &SqlitePool, user_id: i64, mission_id: i64, partner_id: i64) -> AppResult<MissionDetail> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    record(&mut tx, mission_id).await?;
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM partners WHERE id = ?").bind(partner_id).fetch_optional(&mut *tx).await?;
    match kind.as_deref() {
        None => return err("Contact introuvable"),
        Some(k) if k != "PARTICULIER" => return err("Un participant doit être une personne (particulier)"),
        _ => {}
    }
    sqlx::query("INSERT INTO mission_participants (mission_id, partner_id) VALUES (?, ?)")
        .bind(mission_id)
        .bind(partner_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| match AppError::from(e) {
            AppError::Conflict(_) => AppError::Conflict("Ce stagiaire participe déjà à la mission".into()),
            o => o,
        })?;
    sqlx::query("UPDATE partners SET is_stagiaire = 1 WHERE id = ?").bind(partner_id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "AJOUT_PARTICIPANT", ENTITY, Some(mission_id.to_string()), Some(json!({ "partner": partner_id }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

#[derive(Debug, serde::Deserialize, Default)]
#[serde(default)]
// Suivi pédagogique d'un participant.
pub struct ParticipantInput {
    pub progress_status: String,
    pub evaluation: String,
    pub comment: String,
}

// Mise à jour du suivi (progression, évaluation, commentaire).
pub async fn update_participant(pool: &SqlitePool, user_id: i64, pid: i64, i: ParticipantInput) -> AppResult<MissionDetail> {
    if !["EN_COURS", "TERMINE", "ABANDON"].contains(&i.progress_status.as_str()) {
        return err("Statut du participant inconnu");
    }
    let mut tx = pool.begin().await?;
    let mission_id: i64 = sqlx::query_scalar("SELECT mission_id FROM mission_participants WHERE id = ?").bind(pid).fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::NotFound("Participant introuvable".into()))?;
    sqlx::query("UPDATE mission_participants SET progress_status = ?, evaluation = ?, comment = ? WHERE id = ?")
        .bind(&i.progress_status).bind(i.evaluation.trim()).bind(i.comment.trim()).bind(pid)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, Some(user_id), "MODIFICATION_PARTICIPANT", ENTITY, Some(mission_id.to_string()),
        Some(json!({ "participant": pid, "progress_status": i.progress_status }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// Désinscription (présences associées supprimées).
pub async fn remove_participant(pool: &SqlitePool, user_id: i64, pid: i64) -> AppResult<MissionDetail> {
    let mut tx = pool.begin().await?;
    let mission_id: i64 = sqlx::query_scalar("SELECT mission_id FROM mission_participants WHERE id = ?").bind(pid).fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::NotFound("Participant introuvable".into()))?;
    let attended: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_attendance WHERE participant_id = ?").bind(pid).fetch_one(&mut *tx).await?;
    if attended > 0 {
        return err("Ce participant a des présences enregistrées : passez son statut à « Abandon » plutôt que de le retirer");
    }
    sqlx::query("DELETE FROM mission_participants WHERE id = ?").bind(pid).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "RETRAIT_PARTICIPANT", ENTITY, Some(mission_id.to_string()), Some(json!({ "participant": pid }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// ---------- Frais ----------

// Contrôle un frais et renvoie son montant = quantité (millièmes) x prix unitaire.
fn normalize_expense(i: &mut ExpenseInput) -> AppResult<i64> {
    i.label = normalize_spaces(&i.label);
    i.expense_date = i.expense_date.take().filter(|d| !d.trim().is_empty());
    if !EXPENSE_KINDS.contains(&i.kind.as_str()) {
        return err("Type de frais inconnu");
    }
    if let Some(d) = &i.expense_date
        && parse_date(d).is_none() {
            return err("Date de frais invalide");
        }
    if i.quantity_milli == 0 {
        i.quantity_milli = 1000;
    }
    if i.quantity_milli < 0 || i.unit_price < 0 {
        return err("Quantité et prix doivent être positifs");
    }
    Ok(Money(i.unit_price).times_milli_qty(i.quantity_milli).0)
}

// Ajout d'un frais (refacturable par défaut).
pub async fn add_expense(pool: &SqlitePool, user_id: i64, mission_id: i64, mut i: ExpenseInput) -> AppResult<MissionDetail> {
    let amount = normalize_expense(&mut i)?;
    let mut tx = pool.begin().await?;
    record(&mut tx, mission_id).await?;
    sqlx::query(
        "INSERT INTO mission_expenses (mission_id, kind, label, expense_date, quantity_milli, unit_price, amount, billable) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(mission_id).bind(&i.kind).bind(&i.label).bind(&i.expense_date).bind(i.quantity_milli).bind(i.unit_price).bind(amount)
    .bind(i.billable.unwrap_or(true))
    .execute(&mut *tx)
    .await?;
    audit::record(&mut tx, Some(user_id), "AJOUT_FRAIS", ENTITY, Some(mission_id.to_string()), Some(json!({ "kind": i.kind, "amount": amount }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// Mission d'un frais ; refus si le frais est déjà facturé.
async fn expense_owner(conn: &mut SqliteConnection, id: i64) -> AppResult<i64> {
    let row: Option<(i64, Option<i64>)> = sqlx::query_as("SELECT mission_id, invoice_line_id FROM mission_expenses WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    match row {
        None => Err(AppError::NotFound("Frais introuvable".into())),
        Some((_, Some(_))) => err("Frais facturé : il ne peut plus être modifié"),
        Some((m, None)) => Ok(m),
    }
}

// Modification d'un frais non facturé.
pub async fn update_expense(pool: &SqlitePool, user_id: i64, id: i64, mut i: ExpenseInput) -> AppResult<MissionDetail> {
    let amount = normalize_expense(&mut i)?;
    let mut tx = pool.begin().await?;
    let mission_id = expense_owner(&mut tx, id).await?;
    sqlx::query("UPDATE mission_expenses SET kind=?, label=?, expense_date=?, quantity_milli=?, unit_price=?, amount=?, billable=? WHERE id = ?")
        .bind(&i.kind).bind(&i.label).bind(&i.expense_date).bind(i.quantity_milli).bind(i.unit_price).bind(amount)
        .bind(i.billable.unwrap_or(true)).bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, Some(user_id), "MODIFICATION_FRAIS", ENTITY, Some(mission_id.to_string()), Some(json!({ "expense": id, "amount": amount }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// Suppression d'un frais non facturé.
pub async fn delete_expense(pool: &SqlitePool, user_id: i64, id: i64) -> AppResult<MissionDetail> {
    let mut tx = pool.begin().await?;
    let mission_id = expense_owner(&mut tx, id).await?;
    sqlx::query("DELETE FROM mission_expenses WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit::record(&mut tx, Some(user_id), "SUPPRESSION_FRAIS", ENTITY, Some(mission_id.to_string()), Some(json!({ "expense": id }))).await?;
    let d = load(&mut tx, mission_id).await?;
    tx.commit().await?;
    Ok(d)
}

// ---------- Liste et tableau de bord ----------

// Requête de la liste : une ligne par mission avec premières/dernières sessions, temps, frais et reste à facturer.
const LIST_SQL: &str = "SELECT m.id, m.project_id, m.reference, m.status, p.name AS title, p.client_id,
    TRIM(c.name || ' ' || c.first_name) AS client_name, m.center_id, ce.name AS center_name, m.invoice_sent,
    u.display_name AS trainer_name, sw.name AS software_name,
    m.modality, m.group_format, p.start_date, p.end_date,
    (SELECT MIN(s.date) FROM training_sessions s WHERE s.mission_id = m.id AND s.status <> 'ANNULEE') AS first_session,
    (SELECT MAX(s.date) FROM training_sessions s WHERE s.mission_id = m.id AND s.status <> 'ANNULEE') AS last_session,
    m.planned_minutes,
    COALESCE((SELECT SUM(t.duration_minutes) FROM timesheets t WHERE t.project_id = p.id), 0) AS realized_minutes,
    (SELECT COUNT(*) FROM mission_participants mp WHERE mp.mission_id = m.id) AS participants,
    m.pricing_mode, p.hourly_rate, m.daily_rate, m.flat_fee, m.days_count,
    COALESCE((SELECT SUM(e.amount) FROM mission_expenses e WHERE e.mission_id = m.id), 0) AS expenses_total,
    COALESCE((SELECT SUM(t.amount) FROM timesheets t WHERE t.project_id = p.id AND t.billable = 1 AND t.invoice_line_id IS NULL), 0)
      + COALESCE((SELECT SUM(e.amount) FROM mission_expenses e WHERE e.mission_id = m.id AND e.billable = 1 AND e.invoice_line_id IS NULL), 0)
      + CASE WHEN m.pricing_mode = 'FORFAIT' AND NOT EXISTS (SELECT 1 FROM invoice_lines l JOIN invoices i ON i.id = l.invoice_id
            WHERE i.project_id = p.id AND l.source_type = 'FORFAIT' AND i.kind = 'FACTURE' AND i.status <> 'ANNULEE') THEN m.flat_fee ELSE 0 END
      AS uninvoiced_amount
    FROM missions m JOIN projects p ON p.id = m.project_id JOIN partners c ON c.id = p.client_id
    LEFT JOIN partners ce ON ce.id = m.center_id
    LEFT JOIN users u ON u.id = m.trainer_id LEFT JOIN software sw ON sw.id = m.software_id WHERE 1 = 1";

// Filtres de la liste en SQL paramétré (recherche, statut, raccourcis, client, centre, formateur, logiciel, dates).
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &MissionFilters) {
    match f.archived.as_deref() {
        Some("archives") => qb.push(" AND p.archived_at IS NOT NULL"),
        Some("tous") => qb.push(""),
        _ => qb.push(" AND p.archived_at IS NULL"),
    };
    if let Some(q) = f.q.as_deref() {
        for w in search_key(q).split(' ').filter(|w| !w.is_empty()) {
            let like = format!("%{}%", w.replace(['%', '_'], ""));
            qb.push(" AND (lower(p.name) LIKE ").push_bind(like.clone())
                .push(" OR lower(m.reference) LIKE ").push_bind(like.clone())
                .push(" OR c.search_text LIKE ").push_bind(like.clone())
                .push(" OR EXISTS (SELECT 1 FROM partners ce2 WHERE ce2.id = m.center_id AND ce2.search_text LIKE ").push_bind(like.clone()).push(")")
                .push(" OR EXISTS (SELECT 1 FROM mission_participants mp JOIN partners pa ON pa.id = mp.partner_id WHERE mp.mission_id = m.id AND pa.search_text LIKE ")
                .push_bind(like).push("))");
        }
    }
    if let Some(s) = f.status.as_deref().filter(|s| STATUSES.contains(s)) {
        qb.push(" AND m.status = ").push_bind(s.to_string());
    }
    match f.bucket.as_deref() {
        Some("semaine") => {
            let (mon, sun) = week_bounds();
            qb.push(" AND EXISTS (SELECT 1 FROM training_sessions s WHERE s.mission_id = m.id AND s.status <> 'ANNULEE' AND s.date BETWEEN ")
                .push_bind(mon).push(" AND ").push_bind(sun).push(")");
        }
        Some("a_planifier") => { qb.push(" AND m.status IN ('BROUILLON', 'A_PLANIFIER')"); }
        Some("confirmees") => { qb.push(" AND m.status IN ('PLANIFIEE', 'CONFIRMEE')"); }
        Some("en_cours") => { qb.push(" AND m.status = 'EN_COURS'"); }
        Some("realisees") => { qb.push(" AND m.status = 'REALISEE'"); }
        Some("a_facturer") => { qb.push(" AND m.status = 'A_FACTURER'"); }
        Some("facturees") => { qb.push(" AND m.status = 'FACTUREE'"); }
        _ => {}
    }
    if let Some(c) = f.client_id {
        qb.push(" AND p.client_id = ").push_bind(c);
    }
    if let Some(c) = f.center_id {
        qb.push(" AND m.center_id = ").push_bind(c);
    }
    if let Some(t) = f.trainer_id {
        qb.push(" AND m.trainer_id = ").push_bind(t);
    }
    if let Some(s) = f.software_id {
        qb.push(" AND m.software_id = ").push_bind(s);
    }
    if let Some(from) = f.from.as_deref().filter(|d| parse_date(d).is_some()) {
        qb.push(" AND COALESCE(p.end_date, (SELECT MAX(s.date) FROM training_sessions s WHERE s.mission_id = m.id), p.start_date, '9999') >= ").push_bind(from.to_string());
    }
    if let Some(to) = f.to.as_deref().filter(|d| parse_date(d).is_some()) {
        qb.push(" AND COALESCE(p.start_date, (SELECT MIN(s.date) FROM training_sessions s WHERE s.mission_id = m.id), p.end_date, '0000') <= ").push_bind(to.to_string());
    }
}

// Lundi et dimanche de la semaine courante (AAAA-MM-JJ).
pub fn week_bounds() -> (String, String) {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    let mon = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    let sun = mon + chrono::Duration::days(6);
    (mon.format("%Y-%m-%d").to_string(), sun.format("%Y-%m-%d").to_string())
}

// Liste paginée, triée par date de première session décroissante.
pub async fn list(pool: &SqlitePool, f: &MissionFilters) -> AppResult<(Vec<MissionListRow>, i64)> {
    let per_page = f.per_page.unwrap_or(80).clamp(1, 1000);
    let page = f.page.unwrap_or(1).max(1);
    let mut count = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) FROM missions m JOIN projects p ON p.id = m.project_id JOIN partners c ON c.id = p.client_id WHERE 1 = 1",
    );
    push_filters(&mut count, f);
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;
    let mut qb = QueryBuilder::<Sqlite>::new(LIST_SQL);
    push_filters(&mut qb, f);
    qb.push(" ORDER BY COALESCE(first_session, p.start_date, '9999') DESC, m.id DESC LIMIT ").push_bind(per_page)
        .push(" OFFSET ").push_bind((page - 1) * per_page);
    Ok((qb.build_query_as::<MissionListRow>().fetch_all(pool).await?, total))
}

#[derive(Debug, serde::Serialize, Default)]
// Indicateurs de l'écran Missions : compteurs par étape et montants prévus / réalisés / à facturer.
pub struct Dashboard {
    pub buckets: Value,
    pub planned_revenue: i64,
    pub realized_revenue: i64,
    pub planned_minutes: i64,
    pub realized_minutes: i64,
    pub trainees: i64,
    pub to_invoice_count: i64,
    pub to_invoice_amount: i64,
}

// Agrège la liste complète (filtres appliqués, raccourci ignoré) pour calculer les indicateurs.
pub async fn dashboard(pool: &SqlitePool, f: &MissionFilters) -> AppResult<Dashboard> {
    // Lecture de toutes les pages avec les mêmes filtres.
    let mut all = f.clone();
    all.bucket = None;
    all.per_page = Some(1000);
    let mut rows = Vec::new();
    let mut page = 1;
    loop {
        all.page = Some(page);
        let (chunk, total) = list(pool, &all).await?;
        let n = chunk.len();
        rows.extend(chunk);
        if rows.len() as i64 >= total || n == 0 {
            break;
        }
        page += 1;
    }
    let live: Vec<&MissionListRow> = rows.iter().filter(|r| r.status != "ANNULEE").collect();
    let count = |st: &[&str]| live.iter().filter(|r| st.contains(&r.status.as_str())).count();
    let (mon, sun) = week_bounds();
    let mut week = QueryBuilder::<Sqlite>::new("SELECT COUNT(DISTINCT s.mission_id) FROM training_sessions s WHERE s.status <> 'ANNULEE' AND s.date BETWEEN ");
    week.push_bind(mon).push(" AND ").push_bind(sun);
    let this_week: i64 = week.build_query_scalar().fetch_one(pool).await?;
    let ids: Vec<i64> = live.iter().map(|r| r.id).collect();
    let trainees: i64 = if ids.is_empty() {
        0
    } else {
        let mut q = QueryBuilder::<Sqlite>::new("SELECT COUNT(DISTINCT partner_id) FROM mission_participants WHERE mission_id IN (");
        let mut sep = q.separated(", ");
        for id in &ids {
            sep.push_bind(id);
        }
        q.push(")");
        q.build_query_scalar().fetch_one(pool).await?
    };
    let planned_revenue: i64 = live
        .iter()
        .map(|r| planned_fee(&r.pricing_mode, r.planned_minutes, r.days_count, r.hourly_rate, r.daily_rate, r.flat_fee) + r.expenses_total)
        .sum();
    let ids_csv_in = |col: &str| format!("{col} IN ({})", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","));
    let realized_revenue: i64 = if ids.is_empty() {
        0
    } else {
        // CA réalisé = temps réalisés valorisés + frais + forfaits des missions réalisées.
        let sql = format!(
            "SELECT COALESCE((SELECT SUM(t.amount) FROM timesheets t JOIN missions m ON m.project_id = t.project_id WHERE {}), 0)
              + COALESCE((SELECT SUM(e.amount) FROM mission_expenses e WHERE {}), 0)
              + COALESCE((SELECT SUM(m.flat_fee) FROM missions m WHERE m.pricing_mode = 'FORFAIT' AND m.status IN ('REALISEE', 'A_FACTURER', 'FACTUREE') AND {}), 0)",
            ids_csv_in("m.id"), ids_csv_in("e.mission_id"), ids_csv_in("m.id")
        );
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql)).fetch_one(pool).await?
    };
    let to_invoice: Vec<&&MissionListRow> = live.iter().filter(|r| r.uninvoiced_amount > 0 || r.status == "A_FACTURER").collect();
    Ok(Dashboard {
        buckets: json!({
            "a_planifier": count(&["BROUILLON", "A_PLANIFIER"]),
            "confirmees": count(&["PLANIFIEE", "CONFIRMEE"]),
            "semaine": this_week,
            "en_cours": count(&["EN_COURS"]),
            "realisees": count(&["REALISEE"]),
            "a_facturer": count(&["A_FACTURER"]),
            "facturees": count(&["FACTUREE"]),
        }),
        planned_revenue,
        realized_revenue,
        planned_minutes: live.iter().map(|r| r.planned_minutes).sum(),
        realized_minutes: live.iter().map(|r| r.realized_minutes).sum(),
        trainees,
        to_invoice_count: to_invoice.len() as i64,
        to_invoice_amount: to_invoice.iter().map(|r| r.uninvoiced_amount).sum(),
    })
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
// Séance pour la vue calendrier.
pub struct CalendarSession {
    pub id: i64,
    pub mission_id: i64,
    pub reference: String,
    pub title: String,
    pub client_name: String,
    pub date: String,
    pub start_time: String,
    pub end_time: String,
    pub duration_minutes: i64,
    pub status: String,
    pub trainer_name: Option<String>,
}

// Séances d'une période, toutes missions confondues.
pub async fn calendar(pool: &SqlitePool, from: &str, to: &str) -> AppResult<Vec<CalendarSession>> {
    if parse_date(from).is_none() || parse_date(to).is_none() {
        return err("Période invalide");
    }
    Ok(sqlx::query_as::<_, CalendarSession>(
        "SELECT s.id, s.mission_id, m.reference, p.name AS title, TRIM(c.name || ' ' || c.first_name) AS client_name, s.date,
            s.start_time, s.end_time, s.duration_minutes, s.status, u.display_name AS trainer_name
         FROM training_sessions s JOIN missions m ON m.id = s.mission_id JOIN projects p ON p.id = m.project_id
         JOIN partners c ON c.id = p.client_id LEFT JOIN users u ON u.id = s.trainer_id
         WHERE s.date BETWEEN ? AND ? ORDER BY s.date, s.start_time",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?)
}
