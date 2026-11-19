//! Tests d'intégration : projets (lot 3) et missions de formation (lot 3b).

mod common;

use axum::http::StatusCode;
use common::{app, TestApp};
use serde_json::{json, Value};

// POST attendu en succès.
async fn post(t: &TestApp, c: &str, uri: &str, body: Value) -> Value {
    let (s, b, _) = t.call("POST", uri, Some(c), Some(body)).await;
    assert_eq!(s, StatusCode::OK, "{uri} {b}");
    b
}

// Admin + client + stagiaire de base.
async fn setup(t: &TestApp) -> (String, i64, i64, i64) {
    let admin = t.setup_admin().await;
    let me = t.call("GET", "/api/auth/me", Some(&admin), None).await.1["id"].as_i64().unwrap();
    let client = post(t, &admin, "/api/contacts", json!({ "kind": "ENTREPRISE", "name": "Entreprise ABC (DEMO)", "is_client": true })).await;
    let trainee = post(t, &admin, "/api/contacts", json!({ "kind": "PARTICULIER", "name": "MARTIN", "first_name": "Jean", "parent_id": client["id"] })).await;
    (admin, me, client["id"].as_i64().unwrap(), trainee["id"].as_i64().unwrap())
}

// Corps minimal d'une mission.
fn mission_body(client: i64, trainer: i64) -> Value {
    json!({
        "title": "Formation AutoCAD", "client_id": client, "trainer_id": trainer, "software_id": 1,
        "modality": "PRESENTIEL", "group_format": "GROUPE", "os": "PC", "pricing_mode": "HORAIRE",
        "hourly_rate": 8000, "planned_minutes": 840, "days_count": 2
    })
}

#[tokio::test]
// Projet simple : référence PRJ-, tâches, doublon de tâche, dates incohérentes, recherche, suppression.
async fn projet_et_taches() {
    let t = app().await;
    let (admin, me, client, _) = setup(&t).await;
    let p = post(&t, &admin, "/api/projects", json!({ "name": "Formation Microsoft 365", "client_id": client, "hourly_rate": 8000, "billable": true, "member_ids": [me] })).await;
    assert!(p["reference"].as_str().unwrap().starts_with("PRJ-"));
    assert_eq!(p["members"].as_array().unwrap().len(), 1);
    let task = post(&t, &admin, &format!("/api/projects/{}/tasks", p["id"]), json!({ "name": "Formation Teams" })).await;
    assert_eq!(task["name"], "Formation Teams");
    let (s, _, _) = t.call("POST", &format!("/api/projects/{}/tasks", p["id"]), Some(&admin), Some(json!({ "name": "Formation Teams" }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _, _) = t.call("POST", "/api/projects", Some(&admin), Some(json!({ "name": "X", "client_id": client, "start_date": "2026-12-01", "end_date": "2026-11-01" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let list = t.call("GET", "/api/projects?q=microsoft", Some(&admin), None).await.1;
    assert_eq!(list["total"], 1);
    let (s, _, _) = t.call("DELETE", &format!("/api/projects/{}", p["id"]), Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
// Mission : sessions, réalisation -> un seul temps par session, statuts automatiques.
async fn mission_sessions_et_temps_sans_doublon() {
    let t = app().await;
    let (admin, me, client, trainee) = setup(&t).await;
    let m = post(&t, &admin, "/api/missions", mission_body(client, me)).await;
    let year = chrono::Local::now().format("%Y").to_string();
    assert_eq!(m["reference"], format!("MF-{year}-0001"));
    assert_eq!(m["status"], "BROUILLON");
    assert_eq!(m["totals"]["planned_fee"], 112_000);
    let id = m["id"].as_i64().unwrap();

    let m = post(&t, &admin, &format!("/api/missions/{id}/participants"), json!({ "partner_id": trainee })).await;
    assert_eq!(m["participants"].as_array().unwrap().len(), 1);
    let (s, _, _) = t.call("POST", &format!("/api/missions/{id}/participants"), Some(&admin), Some(json!({ "partner_id": trainee }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _, _) = t.call("POST", &format!("/api/missions/{id}/participants"), Some(&admin), Some(json!({ "partner_id": client }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Deux sessions de 7 h (pause d'une heure).
    for d in ["2027-01-12", "2027-01-13"] {
        post(&t, &admin, &format!("/api/missions/{id}/sessions"), json!({ "date": d, "start_time": "09:00", "end_time": "17:00", "break_minutes": 60 })).await;
    }
    let m = t.call("GET", &format!("/api/missions/{id}"), Some(&admin), None).await.1;
    assert_eq!(m["status"], "PLANIFIEE");
    assert_eq!(m["totals"]["sessions_minutes"], 840);
    let s1 = m["sessions"][0]["id"].as_i64().unwrap();
    let s2 = m["sessions"][1]["id"].as_i64().unwrap();
    let p1 = m["participants"][0]["id"].as_i64().unwrap();

    let (s, _, _) = t.call("POST", &format!("/api/missions/{id}/sessions"), Some(&admin), Some(json!({ "date": "2027-01-14", "start_time": "17:00", "end_time": "09:00" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Session réalisée -> 1 ligne de temps ; deuxième validation -> toujours 1 ligne.
    post(&t, &admin, &format!("/api/sessions/{s1}/realize"), json!({})).await;
    let m = post(&t, &admin, &format!("/api/sessions/{s1}/realize"), json!({ "absent_participant_ids": [p1] })).await;
    assert_eq!(m["status"], "EN_COURS");
    assert_eq!(m["totals"]["realized_minutes"], 420);
    assert_eq!(m["totals"]["realized_amount"], 56_000);
    assert_eq!(m["participants"][0]["sessions_absent"], 1);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timesheets").fetch_one(&t.state.pool).await.unwrap();
    assert_eq!(n, 1);

    post(&t, &admin, &format!("/api/sessions/{s2}/realize"), json!({})).await;
    let m = t.call("GET", &format!("/api/missions/{id}"), Some(&admin), None).await.1;
    assert_eq!(m["participants"][0]["minutes_done"], 420);
    assert_eq!(m["totals"]["uninvoiced_amount"], 112_000);

    // Modifier une session réalisée met à jour son temps (pas de nouvelle ligne).
    let (s, m, _) = t.call("PUT", &format!("/api/sessions/{s2}"), Some(&admin), Some(json!({ "date": "2027-01-13", "start_time": "09:00", "end_time": "12:00" }))).await;
    assert_eq!(s, StatusCode::OK, "{m}");
    assert_eq!(m["totals"]["realized_minutes"], 600);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timesheets").fetch_one(&t.state.pool).await.unwrap();
    assert_eq!(n, 2);

    // Changement de tarif : les temps non facturés suivent.
    let mut body = mission_body(client, me);
    body["hourly_rate"] = json!(9000);
    let (s, m, _) = t.call("PUT", &format!("/api/missions/{id}"), Some(&admin), Some(body)).await;
    assert_eq!(s, StatusCode::OK, "{m}");
    assert_eq!(m["totals"]["realized_amount"], 90_000);

    // Annuler une session supprime son temps.
    let m = post(&t, &admin, &format!("/api/sessions/{s2}/status"), json!({ "status": "ANNULEE" })).await;
    assert_eq!(m["totals"]["realized_minutes"], 420);

    // Un participant avec présences ne peut pas être retiré.
    let (s, _, _) = t.call("DELETE", &format!("/api/participants/{p1}"), Some(&admin), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
// Impossible de réaliser une session sans formateur connu.
async fn session_sans_formateur_refusee() {
    let t = app().await;
    let (admin, _, client, _) = setup(&t).await;
    let mut body = mission_body(client, 0);
    body["trainer_id"] = Value::Null;
    let m = post(&t, &admin, "/api/missions", body).await;
    let id = m["id"].as_i64().unwrap();
    let m = post(&t, &admin, &format!("/api/missions/{id}/sessions"), json!({ "date": "2027-01-12", "start_time": "09:00", "end_time": "12:00" })).await;
    let s1 = m["sessions"][0]["id"].as_i64().unwrap();
    let (s, b, _) = t.call("POST", &format!("/api/sessions/{s1}/realize"), Some(&admin), Some(json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("formateur"));
}

#[tokio::test]
// Frais : montant = quantité x PU ; totaux de la fiche.
async fn frais_et_totaux() {
    let t = app().await;
    let (admin, me, client, _) = setup(&t).await;
    let id = post(&t, &admin, "/api/missions", mission_body(client, me)).await["id"].as_i64().unwrap();
    post(&t, &admin, &format!("/api/missions/{id}/expenses"), json!({ "kind": "DEPLACEMENT", "label": "Train", "unit_price": 12000 })).await;
    let m = post(&t, &admin, &format!("/api/missions/{id}/expenses"), json!({ "kind": "KILOMETRIQUE", "label": "Trajet", "quantity_milli": 100_000, "unit_price": 60 })).await;
    assert_eq!(m["expenses"][1]["amount"], 6_000);
    assert_eq!(m["totals"]["expenses_total"], 18_000);
    assert_eq!(m["totals"]["planned_total"], 130_000);
    let (s, _, _) = t.call("POST", &format!("/api/missions/{id}/expenses"), Some(&admin), Some(json!({ "kind": "AUTRE", "unit_price": -5 }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
// Pièces jointes : envoi, téléchargement, suppression, nom de fichier assaini.
async fn documents() {
    let t = app().await;
    let (admin, me, client, _) = setup(&t).await;
    let id = post(&t, &admin, "/api/missions", mission_body(client, me)).await["id"].as_i64().unwrap();
    let boundary = "----b";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\nCONVENTION\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../convention.pdf\"\r\n\r\n%PDF-1.4 test\r\n--{boundary}--\r\n"
    );
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/missions/{id}/documents"))
        .header("cookie", &admin)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(axum::body::Body::from(body))
        .unwrap();
    let res = tower::ServiceExt::oneshot(t.router.clone(), req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let m = t.call("GET", &format!("/api/missions/{id}"), Some(&admin), None).await.1;
    let doc = &m["documents"][0];
    assert_eq!(doc["file_name"], "convention.pdf");
    assert_eq!(doc["category"], "CONVENTION");
    let (s, _, _) = t.call("GET", &format!("/api/documents/{}", doc["id"]), Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = t.call("DELETE", &format!("/api/documents/{}", doc["id"]), Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
// Compteurs du tableau de bord et transitions de statut.
async fn tableau_de_bord_et_statuts() {
    let t = app().await;
    let (admin, me, client, trainee) = setup(&t).await;
    let id = post(&t, &admin, "/api/missions", mission_body(client, me)).await["id"].as_i64().unwrap();
    post(&t, &admin, &format!("/api/missions/{id}/participants"), json!({ "partner_id": trainee })).await;
    let d = t.call("GET", "/api/missions/dashboard", Some(&admin), None).await.1;
    assert_eq!(d["buckets"]["a_planifier"], 1);
    assert_eq!(d["planned_revenue"], 112_000);
    assert_eq!(d["planned_minutes"], 840);
    assert_eq!(d["trainees"], 1);
    post(&t, &admin, &format!("/api/missions/{id}/status"), json!({ "status": "A_FACTURER" })).await;
    let d = t.call("GET", "/api/missions/dashboard", Some(&admin), None).await.1;
    assert_eq!(d["buckets"]["a_facturer"], 1);
    let l = t.call("GET", "/api/missions?bucket=a_facturer", Some(&admin), None).await.1;
    assert_eq!(l["total"], 1);
    let l = t.call("GET", "/api/missions?q=martin", Some(&admin), None).await.1;
    assert_eq!(l["total"], 1, "recherche par stagiaire");
    let (s, _, _) = t.call("POST", &format!("/api/missions/{id}/status"), Some(&admin), Some(json!({ "status": "XXX" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Suppression possible tant qu'il n'y a ni temps ni facture.
    let (s, _, _) = t.call("DELETE", &format!("/api/missions/{id}"), Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
// Un UTILISATEUR ne peut pas créer de mission.
async fn droits_missions() {
    let t = app().await;
    let (admin, me, client, _) = setup(&t).await;
    let (_, user) = t.create_user(&admin, "user@demo.fr", "UTILISATEUR").await;
    assert_eq!(t.call("POST", "/api/missions", Some(&user), Some(mission_body(client, me))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("GET", "/api/missions", Some(&user), None).await.0, StatusCode::OK);
    assert_eq!(t.call("POST", "/api/projects", Some(&user), Some(json!({ "name": "X", "client_id": client }))).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
// Centre de formation, case facture envoyée, annotations, tarif 50 EUR par défaut, filtre par centre.
async fn centre_annotations_et_tarif_par_defaut() {
    let t = app().await;
    let (admin, me, client, trainee) = setup(&t).await;
    let centre = post(&t, &admin, "/api/contacts", json!({ "kind": "ORGANISME", "name": "SENZA (DEMO)", "is_client": true })).await;
    // Tarif horaire omis -> 50,00 EUR par défaut.
    let mut body = mission_body(client, me);
    body["hourly_rate"] = json!(0);
    body["center_id"] = centre["id"].clone();
    let m = post(&t, &admin, "/api/missions", body).await;
    assert_eq!(m["hourly_rate"], 5000);
    assert_eq!(m["center_name"], "SENZA (DEMO)");
    assert_eq!(m["invoice_sent"], false);
    // Un particulier ne peut pas être un centre.
    let mut bad = mission_body(client, me);
    bad["center_id"] = json!(trainee);
    let (s, _, _) = t.call("POST", "/api/missions", Some(&admin), Some(bad)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Mise à jour : facture envoyée + annotations.
    let mut upd = m.clone();
    upd["invoice_sent"] = json!(true);
    upd["annotations"] = json!("Convention signée le 3/11");
    let (s, m2, _) = t.call("PUT", &format!("/api/missions/{}", m["id"]), Some(&admin), Some(upd)).await;
    assert_eq!(s, StatusCode::OK, "{m2}");
    assert_eq!(m2["invoice_sent"], true);
    assert_eq!(m2["annotations"], "Convention signée le 3/11");
    // Filtre et recherche par centre.
    let list = t.call("GET", &format!("/api/missions?center_id={}", centre["id"]), Some(&admin), None).await.1;
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["center_name"], "SENZA (DEMO)");
    let list = t.call("GET", "/api/missions?q=senza", Some(&admin), None).await.1;
    assert_eq!(list["total"], 1);
}

#[tokio::test]
// Liste de référence des centres : ajout, refus d'un particulier, retrait, droits.
async fn liste_des_centres_de_formation() {
    let t = app().await;
    let (admin, me, client, trainee) = setup(&t).await;
    // Création d'un nouvel organisme directement depuis la liste.
    let c1 = post(&t, &admin, "/api/training-centers", json!({ "name": "BT Formation" })).await;
    assert_eq!(c1["kind"], "ORGANISME");
    // Ajout d'un contact existant ; un particulier est refusé ; doublon refusé.
    let c2 = post(&t, &admin, "/api/training-centers", json!({ "partner_id": client })).await;
    assert_eq!(c2["id"], client);
    let (s, _, _) = t.call("POST", "/api/training-centers", Some(&admin), Some(json!({ "partner_id": trainee }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = t.call("POST", "/api/training-centers", Some(&admin), Some(json!({ "partner_id": client }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let list = t.call("GET", "/api/training-centers", Some(&admin), None).await.1;
    assert_eq!(list.as_array().unwrap().len(), 2);
    // Une mission liée au centre ; retrait de la liste : la mission garde son centre.
    let mut body = mission_body(client, me);
    body["center_id"] = c1["id"].clone();
    let m = post(&t, &admin, "/api/missions", body).await;
    let (s, _, _) = t.call("DELETE", &format!("/api/training-centers/{}", c1["id"]), Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK);
    let list = t.call("GET", "/api/training-centers", Some(&admin), None).await.1;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let m2 = t.call("GET", &format!("/api/missions/{}", m["id"]), Some(&admin), None).await.1;
    assert_eq!(m2["center_name"], "BT Formation");
    // Droits : un utilisateur simple lit la liste mais ne la modifie pas.
    let (_, user) = t.create_user(&admin, "u@demo.fr", "UTILISATEUR").await;
    assert_eq!(t.call("GET", "/api/training-centers", Some(&user), None).await.0, StatusCode::OK);
    assert_eq!(t.call("POST", "/api/training-centers", Some(&user), Some(json!({ "name": "X" }))).await.0, StatusCode::FORBIDDEN);
}
