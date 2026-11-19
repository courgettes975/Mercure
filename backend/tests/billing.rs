//! Tests d'intégration : feuilles de temps (lot 4), facturation (lots 5-6), PDF (lot 7).

mod common;

use axum::http::StatusCode;
use common::{app, TestApp};
use serde_json::{json, Value};

// Appel attendu en succès (le corps est renvoyé).
async fn ok(t: &TestApp, c: &str, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (s, b, _) = t.call(method, uri, Some(c), body).await;
    assert_eq!(s, StatusCode::OK, "{method} {uri} {b}");
    b
}

// Contexte commun : admin, client, projet facturable.
struct Ctx {
    admin: String,
    me: i64,
    client: i64,
    project: i64,
}

// Paramètres entreprise + client + projet à 80 EUR/h.
async fn setup(t: &TestApp) -> Ctx {
    let admin = t.setup_admin().await;
    let me = t.call("GET", "/api/auth/me", Some(&admin), None).await.1["id"].as_i64().unwrap();
    let mut s = t.call("GET", "/api/settings/company", Some(&admin), None).await.1;
    s["legal_name"] = json!("RL Concept (DEMO)");
    s["vat_exemption_mention"] = json!("TVA non applicable, art. 293 B du CGI");
    ok(t, &admin, "PUT", "/api/settings/company", Some(s)).await;
    let client = ok(t, &admin, "POST", "/api/contacts", Some(json!({ "kind": "ENTREPRISE", "name": "Demo Industrie SAS", "is_client": true, "street": "1 rue de la Démo", "zip": "33000", "city": "Bordeaux" }))).await["id"].as_i64().unwrap();
    let project = ok(t, &admin, "POST", "/api/projects", Some(json!({ "name": "Formation Microsoft 365", "client_id": client, "hourly_rate": 8000, "billable": true }))).await["id"].as_i64().unwrap();
    Ctx { admin, me, client, project }
}

// Ajoute une ligne de temps.
async fn add_time(t: &TestApp, c: &str, project: i64, date: &str, minutes: i64) -> Value {
    ok(t, c, "POST", "/api/timesheets", Some(json!({ "date": date, "project_id": project, "description": "Formation Teams", "duration_minutes": minutes }))).await
}

#[tokio::test]
// Saisie, contrôles (durée), visibilité limitée d'un utilisateur, verrous.
async fn saisie_des_temps_et_droits() {
    let t = app().await;
    let c = setup(&t).await;
    let ts = add_time(&t, &c.admin, c.project, "2026-11-21", 420).await;
    assert_eq!(ts["amount"], 56_000, "7 h x 80 € = 560 €");
    assert_eq!(ts["billable"], true);

    let (s, _, _) = t.call("POST", "/api/timesheets", Some(&c.admin), Some(json!({ "date": "2026-11-21", "project_id": c.project, "duration_minutes": 0 }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (uid, user) = t.create_user(&c.admin, "user@demo.fr", "UTILISATEUR").await;
    // Un utilisateur ne voit que ses temps.
    assert_eq!(ok(&t, &user, "GET", "/api/timesheets", None).await["items"].as_array().unwrap().len(), 0);
    let mine = add_time(&t, &user, c.project, "2026-11-22", 60).await;
    assert_eq!(mine["user_id"], uid);
    let all = ok(&t, &c.admin, "GET", "/api/timesheets", None).await;
    assert_eq!(all["totals"]["minutes"], 480);
    assert_eq!(all["totals"]["billable_amount"], 64_000);
    // Il ne peut ni modifier ni supprimer le temps d'un autre, ni saisir pour un autre.
    assert_eq!(t.call("DELETE", &format!("/api/timesheets/{}", ts["id"]), Some(&user), None).await.0, StatusCode::FORBIDDEN);
    let (s, _, _) = t.call("POST", "/api/timesheets", Some(&user), Some(json!({ "date": "2026-11-22", "project_id": c.project, "duration_minutes": 60, "user_id": c.me }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Il ne voit pas les factures.
    assert_eq!(t.call("GET", "/api/invoices", Some(&user), None).await.0, StatusCode::FORBIDDEN);

    // Duplication et grille.
    let dup = ok(&t, &c.admin, "POST", &format!("/api/timesheets/{}/duplicate", ts["id"]), Some(json!({ "date": "2026-11-23" }))).await;
    assert_eq!(dup["date"], "2026-11-23");
    ok(&t, &c.admin, "PUT", "/api/timesheets/cell", Some(json!({ "date": "2026-11-24", "project_id": c.project, "minutes": 150 }))).await;
    ok(&t, &c.admin, "PUT", "/api/timesheets/cell", Some(json!({ "date": "2026-11-24", "project_id": c.project, "minutes": 90 }))).await;
    let day = ok(&t, &c.admin, "GET", &format!("/api/timesheets?from=2026-11-24&to=2026-11-24&user_id={}", c.me), None).await;
    assert_eq!(day["totals"]["minutes"], 90);
    ok(&t, &c.admin, "PUT", "/api/timesheets/cell", Some(json!({ "date": "2026-11-24", "project_id": c.project, "minutes": 0 }))).await;
    let day = ok(&t, &c.admin, "GET", "/api/timesheets?from=2026-11-24&to=2026-11-24", None).await;
    assert_eq!(day["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
// Facture depuis les temps ; un temps facturé ne peut plus l'être ni être modifié.
async fn facturation_des_temps_sans_double_facturation() {
    let t = app().await;
    let c = setup(&t).await;
    add_time(&t, &c.admin, c.project, "2026-11-20", 420).await;
    let ts2 = add_time(&t, &c.admin, c.project, "2026-11-21", 180).await;

    let b = ok(&t, &c.admin, "GET", &format!("/api/billing/projects/{}", c.project), None).await;
    assert_eq!(b["total"], 80_000, "10 h x 80 € = 800 €");

    let inv = ok(&t, &c.admin, "POST", "/api/billing/from-timesheets", Some(json!({ "project_id": c.project, "invoice_date": "2026-11-25" }))).await;
    assert_eq!(inv["status"], "BROUILLON");
    assert!(inv["number"].is_null());
    assert_eq!(inv["total_ht"], 80_000);
    assert_eq!(inv["total_ttc"], 80_000);
    assert_eq!(inv["lines"][0]["quantity_milli"], 10_000);
    assert_eq!(inv["due_date"], "2026-12-25");

    // Deuxième tentative : plus rien à facturer.
    let (s, b, _) = t.call("POST", "/api/billing/from-timesheets", Some(&c.admin), Some(json!({ "project_id": c.project }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    // Un temps facturé est verrouillé.
    let (s, _, _) = t.call("PUT", &format!("/api/timesheets/{}", ts2["id"]), Some(&c.admin), Some(json!({ "date": "2026-11-21", "project_id": c.project, "duration_minutes": 60 }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Sélection explicite d'un temps déjà facturé : refusée.
    let (s, _, _) = t.call("POST", "/api/billing/from-timesheets", Some(&c.admin), Some(json!({ "project_id": c.project, "timesheet_ids": [ts2["id"]] }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Supprimer le brouillon libère les temps.
    ok(&t, &c.admin, "DELETE", &format!("/api/invoices/{}", inv["id"]), None).await;
    let b = ok(&t, &c.admin, "GET", &format!("/api/billing/projects/{}", c.project), None).await;
    assert_eq!(b["total"], 80_000);
}

#[tokio::test]
// Validation (numéro, figement), paiements partiels, avoir et libération des temps.
async fn validation_numerotation_paiements_avoir() {
    let t = app().await;
    let c = setup(&t).await;
    let year = 2026;
    add_time(&t, &c.admin, c.project, "2026-11-20", 420).await;
    let inv = ok(&t, &c.admin, "POST", "/api/billing/from-timesheets", Some(json!({ "project_id": c.project, "invoice_date": "2026-11-25" }))).await;
    let id = inv["id"].as_i64().unwrap();
    let v = ok(&t, &c.admin, "POST", &format!("/api/invoices/{id}/validate"), None).await;
    assert_eq!(v["number"], format!("FAC-{year}-000001"));
    assert_eq!(v["status"], "VALIDEE");
    assert_eq!(v["billing_name"], "Demo Industrie SAS");
    assert!(v["billing_address"].as_str().unwrap().contains("33000 Bordeaux"));
    assert_eq!(v["vat_mention"], "TVA non applicable, art. 293 B du CGI");
    assert_eq!(v["amount_due"], 56_000);

    // Facture validée : non modifiable, non supprimable, même directement en base.
    let (s, _, _) = t.call("PUT", &format!("/api/invoices/{id}"), Some(&c.admin), Some(json!({ "partner_id": c.client, "lines": [] }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(t.call("DELETE", &format!("/api/invoices/{id}"), Some(&c.admin), None).await.0, StatusCode::BAD_REQUEST);
    assert!(sqlx::query("UPDATE invoices SET total_ttc = 1 WHERE id = ?").bind(id).execute(&t.state.pool).await.is_err());
    assert!(sqlx::query("DELETE FROM invoice_lines WHERE invoice_id = ?").bind(id).execute(&t.state.pool).await.is_err());

    // Chronologie : un brouillon daté avant la dernière facture validée ne peut pas être validé.
    let early = ok(&t, &c.admin, "POST", "/api/invoices", Some(json!({ "partner_id": c.client, "invoice_date": "2026-11-01",
        "lines": [{ "description": "Conseil", "quantity_milli": 1000, "unit_price": 10000 }] }))).await;
    let (s, b, _) = t.call("POST", &format!("/api/invoices/{}/validate", early["id"]), Some(&c.admin), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("chronologique"));

    // Paiements partiels puis solde.
    let (s, _, _) = t.call("POST", &format!("/api/invoices/{id}/payments"), Some(&c.admin), Some(json!({ "amount": 60_000 }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "trop-perçu refusé");
    let p = ok(&t, &c.admin, "POST", &format!("/api/invoices/{id}/payments"), Some(json!({ "amount": 20_000, "payment_date": "2026-11-30" }))).await;
    assert_eq!(p["status"], "VALIDEE");
    assert_eq!(p["amount_due"], 36_000);
    let pay_id = p["payments"][0]["id"].as_i64().unwrap();
    let p = ok(&t, &c.admin, "POST", &format!("/api/invoices/{id}/payments"), Some(json!({ "amount": 36_000 }))).await;
    assert_eq!(p["status"], "PAYEE");
    assert_eq!(p["amount_due"], 0);

    // Une facture payée ne s'annule pas sans retirer les paiements.
    let (s, _, _) = t.call("POST", &format!("/api/invoices/{id}/cancel"), Some(&c.admin), Some(json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    ok(&t, &c.admin, "DELETE", &format!("/api/payments/{pay_id}"), None).await;
    let other = p["payments"].as_array().unwrap().iter().map(|x| x["id"].as_i64().unwrap()).find(|i| *i != pay_id).unwrap();
    let p = ok(&t, &c.admin, "DELETE", &format!("/api/payments/{other}"), None).await;
    assert_eq!(p["status"], "VALIDEE");

    // Avoir : numéro propre, facture annulée, temps de nouveau facturables.
    let av = ok(&t, &c.admin, "POST", &format!("/api/invoices/{id}/cancel"), Some(json!({ "reason": "Erreur de tarif" }))).await;
    assert_eq!(av["kind"], "AVOIR");
    assert_eq!(av["number"], format!("AV-{}-000001", chrono::Local::now().format("%Y")));
    assert_eq!(av["total_ttc"], 56_000);
    let orig = ok(&t, &c.admin, "GET", &format!("/api/invoices/{id}"), None).await;
    assert_eq!(orig["status"], "ANNULEE");
    assert_eq!(orig["credit_note_id"], av["id"]);
    let b = ok(&t, &c.admin, "GET", &format!("/api/billing/projects/{}", c.project), None).await;
    assert_eq!(b["total"], 56_000);

    // Nouvelle facture : numéro suivant, sans trou.
    let inv2 = ok(&t, &c.admin, "POST", "/api/billing/from-timesheets", Some(json!({ "project_id": c.project, "invoice_date": "2026-11-26" }))).await;
    let v2 = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", inv2["id"]), None).await;
    assert_eq!(v2["number"], format!("FAC-{year}-000002"));

    let list = ok(&t, &c.admin, "GET", "/api/invoices?q=demo", None).await;
    assert_eq!(list["total"], 4);
}

#[tokio::test]
// Validations concurrentes -> numéros uniques et consécutifs.
async fn numerotation_concurrente_sans_doublon() {
    let t = app().await;
    let c = setup(&t).await;
    let mut ids = Vec::new();
    for _ in 0..15 {
        let inv = ok(&t, &c.admin, "POST", "/api/invoices", Some(json!({ "partner_id": c.client, "invoice_date": "2026-11-25",
            "lines": [{ "description": "Formation", "quantity_milli": 7000, "unit_price": 8000 }] }))).await;
        ids.push(inv["id"].as_i64().unwrap());
    }
    let mut handles = Vec::new();
    for id in ids {
        let router = t.router.clone();
        let cookie = c.admin.clone();
        handles.push(tokio::spawn(async move {
            let req = axum::http::Request::builder().method("POST").uri(format!("/api/invoices/{id}/validate"))
                .header("cookie", cookie).body(axum::body::Body::empty()).unwrap();
            let res = tower::ServiceExt::oneshot(router, req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let mut numbers: Vec<String> = sqlx::query_scalar("SELECT number FROM invoices").fetch_all(&t.state.pool).await.unwrap();
    numbers.sort();
    let expected: Vec<String> = (1..=15).map(|n| format!("FAC-2026-{n:06}")).collect();
    assert_eq!(numbers, expected);
}

#[tokio::test]
// Scénario complet : mission -> sessions -> frais -> facture -> statut FACTUREE.
async fn mission_autocad_jusqu_a_la_facture() {
    let t = app().await;
    let c = setup(&t).await;
    let m = ok(&t, &c.admin, "POST", "/api/missions", Some(json!({ "title": "Formation AutoCAD", "client_id": c.client, "trainer_id": c.me,
        "pricing_mode": "HORAIRE", "hourly_rate": 8000, "planned_minutes": 840 }))).await;
    let id = m["id"].as_i64().unwrap();
    for d in ["2027-01-12", "2027-01-13"] {
        ok(&t, &c.admin, "POST", &format!("/api/missions/{id}/sessions"), Some(json!({ "date": d, "start_time": "09:00", "end_time": "17:00", "break_minutes": 60 }))).await;
    }
    ok(&t, &c.admin, "POST", &format!("/api/missions/{id}/expenses"), Some(json!({ "kind": "DEPLACEMENT", "unit_price": 12000 }))).await;
    let m = ok(&t, &c.admin, "POST", &format!("/api/missions/{id}/expenses"), Some(json!({ "kind": "HEBERGEMENT", "unit_price": 9500 }))).await;
    for s in m["sessions"].as_array().unwrap() {
        ok(&t, &c.admin, "POST", &format!("/api/sessions/{}/realize", s["id"]), Some(json!({}))).await;
    }
    let project = m["project_id"].as_i64().unwrap();
    let inv = ok(&t, &c.admin, "POST", "/api/billing/from-timesheets", Some(json!({ "project_id": project, "invoice_date": "2027-01-14" }))).await;
    assert_eq!(inv["total_ttc"], 133_500, "14 h x 80 € + 120 € + 95 € = 1 335 €");
    assert_eq!(inv["lines"].as_array().unwrap().len(), 3);
    assert!(inv["lines"][0]["description"].as_str().unwrap().contains("du 12/01/2027 au 13/01/2027"));

    // Session facturée : plus modifiable.
    let s1 = m["sessions"][0]["id"].as_i64().unwrap();
    let (s, _, _) = t.call("PUT", &format!("/api/sessions/{s1}"), Some(&c.admin), Some(json!({ "date": "2027-01-12", "start_time": "09:00", "end_time": "12:00" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", inv["id"]), None).await;
    let m = ok(&t, &c.admin, "GET", &format!("/api/missions/{id}"), None).await;
    assert_eq!(m["status"], "FACTUREE");
    assert_eq!(m["totals"]["uninvoiced_amount"], 0);
    assert_eq!(m["totals"]["invoiced_ht"], 133_500);

    // Tableau de bord.
    let d = ok(&t, &c.admin, "GET", "/api/dashboard", None).await;
    assert_eq!(d["pending_invoices"], 1);
    assert_eq!(d["pending_amount"], 133_500);
    assert_eq!(d["uninvoiced_amount"], 0);
    assert_eq!(d["revenue_by_month"].as_array().unwrap().len(), 12);

    // PDF.
    let req = axum::http::Request::builder().uri(format!("/api/invoices/{}/pdf", inv["id"])).header("cookie", &c.admin).body(axum::body::Body::empty()).unwrap();
    let res = tower::ServiceExt::oneshot(t.router.clone(), req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = http_body_util::BodyExt::collect(res.into_body()).await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));
}

#[tokio::test]
// Mission au forfait et régime assujetti : TVA calculée côté serveur, forfait facturé une seule fois.
async fn forfait_et_tva() {
    let t = app().await;
    let c = setup(&t).await;
    // Passage en régime assujetti (20 %).
    let mut s = ok(&t, &c.admin, "GET", "/api/settings/company", None).await;
    s["vat_regime"] = json!("ASSUJETTI");
    s["default_vat_rate_bp"] = json!(2000);
    ok(&t, &c.admin, "PUT", "/api/settings/company", Some(s)).await;

    let m = ok(&t, &c.admin, "POST", "/api/missions", Some(json!({ "title": "Formation Revit", "client_id": c.client, "trainer_id": c.me,
        "pricing_mode": "FORFAIT", "flat_fee": 90_000 }))).await;
    let project = m["project_id"].as_i64().unwrap();
    let inv = ok(&t, &c.admin, "POST", "/api/billing/from-timesheets", Some(json!({ "project_id": project, "invoice_date": "2026-11-25" }))).await;
    assert_eq!(inv["total_ht"], 90_000);
    assert_eq!(inv["total_vat"], 18_000);
    assert_eq!(inv["total_ttc"], 108_000);
    // Le forfait n'est facturé qu'une fois.
    let (st, _, _) = t.call("POST", "/api/billing/from-timesheets", Some(&c.admin), Some(json!({ "project_id": project }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Facture manuelle : le navigateur n'impose pas les totaux.
    let manual = ok(&t, &c.admin, "POST", "/api/invoices", Some(json!({ "partner_id": c.client, "invoice_date": "2026-11-25",
        "total_ttc": 1, "lines": [{ "description": "Support", "quantity_milli": 1500, "unit_price": 3333, "vat_rate_bp": 550, "total_ht": 999999 }] }))).await;
    assert_eq!(manual["total_ht"], 5_000);
    assert_eq!(manual["total_vat"], 275);
    assert_eq!(manual["total_ttc"], 5_275);
    let (st, _, _) = t.call("POST", "/api/invoices", Some(&c.admin), Some(json!({ "partner_id": c.client, "lines": [{ "description": "X", "quantity_milli": 0, "unit_price": 100 }] }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _, _) = t.call("POST", "/api/invoices", Some(&c.admin), Some(json!({ "partner_id": c.client, "lines": [{ "description": "X", "quantity_milli": 1000, "unit_price": -100 }] }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
// Une facture intermédiaire ne passe pas la mission en FACTUREE s'il reste des sessions prévues.
async fn mission_partiellement_realisee_reste_en_cours() {
    let t = app().await;
    let c = setup(&t).await;
    let m = ok(&t, &c.admin, "POST", "/api/missions", Some(json!({ "title": "Formation Revit", "client_id": c.client, "trainer_id": c.me, "hourly_rate": 5000 }))).await;
    let id = m["id"].as_i64().unwrap();
    for d in ["2026-11-20", "2026-11-27"] {
        ok(&t, &c.admin, "POST", &format!("/api/missions/{id}/sessions"), Some(json!({ "date": d, "start_time": "09:00", "end_time": "12:30" }))).await;
    }
    let m = ok(&t, &c.admin, "GET", &format!("/api/missions/{id}"), None).await;
    ok(&t, &c.admin, "POST", &format!("/api/sessions/{}/realize", m["sessions"][0]["id"]), Some(json!({}))).await;
    let inv = ok(&t, &c.admin, "POST", "/api/billing/from-timesheets", Some(json!({ "project_id": m["project_id"], "invoice_date": "2026-11-21" }))).await;
    assert_eq!(inv["total_ttc"], 17_500);
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", inv["id"]), None).await;
    let m = ok(&t, &c.admin, "GET", &format!("/api/missions/{id}"), None).await;
    assert_eq!(m["status"], "EN_COURS", "une session reste prévue : la mission n'est pas encore facturée");
}

#[tokio::test]
// Récapitulatif mensuel par centre et création de la facture mensuelle.
async fn facturation_mensuelle_par_centre() {
    let t = app().await;
    let c = setup(&t).await;
    let centre = ok(&t, &c.admin, "POST", "/api/training-centers", Some(json!({ "name": "SENZA" }))).await["id"].as_i64().unwrap();
    let client2 = ok(&t, &c.admin, "POST", "/api/contacts", Some(json!({ "kind": "ENTREPRISE", "name": "Atelier Bois", "is_client": true }))).await["id"].as_i64().unwrap();
    // Deux missions pour le même centre (clients différents, tarifs différents) + un projet direct.
    let m1 = ok(&t, &c.admin, "POST", "/api/missions", Some(json!({ "title": "AutoCAD initiation", "client_id": c.client, "center_id": centre, "trainer_id": c.me, "hourly_rate": 5000, "pricing_mode": "HORAIRE" }))).await;
    let m2 = ok(&t, &c.admin, "POST", "/api/missions", Some(json!({ "title": "Revit perfectionnement", "client_id": client2, "center_id": centre, "trainer_id": c.me, "hourly_rate": 6000, "pricing_mode": "HORAIRE" }))).await;
    let (p1, p2) = (m1["project_id"].as_i64().unwrap(), m2["project_id"].as_i64().unwrap());
    add_time(&t, &c.admin, p1, "2026-11-03", 180).await; // 3 h x 50
    add_time(&t, &c.admin, p1, "2026-10-28", 60).await; // arriéré 1 h x 50
    add_time(&t, &c.admin, p2, "2026-11-12", 420).await; // 7 h x 60
    add_time(&t, &c.admin, p2, "2026-12-01", 60).await; // mois suivant : exclu
    add_time(&t, &c.admin, c.project, "2026-11-05", 120).await; // client direct 2 h x 80
    let frais = ok(&t, &c.admin, "POST", &format!("/api/missions/{}/expenses", m2["id"]), Some(json!({ "kind": "DEPLACEMENT", "label": "Train", "quantity_milli": 1000, "unit_price": 4500, "billable": true }))).await;
    assert!(frais["id"].is_number());

    let r = ok(&t, &c.admin, "GET", "/api/billing/monthly?month=2026-11", None).await;
    assert_eq!(r["month_label"], "novembre 2026");
    let groups = r["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let g = groups.iter().find(|g| g["partner_id"] == centre).unwrap();
    assert_eq!(g["is_center"], true);
    assert_eq!(g["minutes"], 660);
    assert_eq!(g["minutes_before"], 60);
    assert_eq!(g["amount"], 4 * 5000 + 7 * 6000);
    assert_eq!(g["expenses_total"], 4500);
    assert_eq!(g["missions"].as_array().unwrap().len(), 2);
    let direct = groups.iter().find(|g| g["partner_id"] == c.client).unwrap();
    assert_eq!(direct["is_center"], false);
    assert_eq!(direct["amount"], 2 * 8000);

    let (s, _, _) = t.call("GET", "/api/billing/monthly?month=2026-13", Some(&c.admin), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Facture du centre : 2 lignes de temps + 1 frais, adressée au centre.
    let inv = ok(&t, &c.admin, "POST", "/api/billing/monthly", Some(json!({ "partner_id": centre, "month": "2026-11", "invoice_date": "2026-11-30" }))).await;
    assert_eq!(inv["partner_id"], centre);
    let lines = inv["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(inv["total_ht"], 4 * 5000 + 7 * 6000 + 4500);
    assert!(lines[0]["description"].as_str().unwrap().contains("Demo Industrie SAS"));
    // Plus rien pour ce centre ; le temps de décembre reste à facturer le mois suivant.
    let r = ok(&t, &c.admin, "GET", "/api/billing/monthly?month=2026-11", None).await;
    assert!(r["groups"].as_array().unwrap().iter().all(|g| g["partner_id"] != centre));
    let r = ok(&t, &c.admin, "GET", "/api/billing/monthly?month=2026-12", None).await;
    let g = r["groups"].as_array().unwrap().iter().find(|g| g["partner_id"] == centre).unwrap();
    assert_eq!(g["minutes"], 60);
    let (s, _, _) = t.call("POST", "/api/billing/monthly", Some(&c.admin), Some(json!({ "partner_id": centre, "month": "2026-11" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
// Statistiques : agrégats par centre, filtres, période, exports CSV.
async fn statistiques_par_centre_et_export() {
    let t = app().await;
    let c = setup(&t).await;
    let centre = ok(&t, &c.admin, "POST", "/api/training-centers", Some(json!({ "name": "ONE LEARN" }))).await["id"].as_i64().unwrap();
    let m = ok(&t, &c.admin, "POST", "/api/missions", Some(json!({ "title": "Fusion 360", "client_id": c.client, "center_id": centre, "trainer_id": c.me, "software_id": 1, "hourly_rate": 5000, "modality": "DISTANCIEL", "os": "MAC" }))).await;
    let stag = ok(&t, &c.admin, "POST", "/api/contacts", Some(json!({ "kind": "PARTICULIER", "name": "DURAND", "first_name": "Léa", "parent_id": c.client }))).await["id"].as_i64().unwrap();
    ok(&t, &c.admin, "POST", &format!("/api/missions/{}/participants", m["id"]), Some(json!({ "partner_id": stag }))).await;
    let s = ok(&t, &c.admin, "POST", &format!("/api/missions/{}/sessions", m["id"]), Some(json!({ "date": "2026-03-10", "start_time": "09:00", "end_time": "12:00", "trainer_id": c.me }))).await;
    ok(&t, &c.admin, "POST", &format!("/api/sessions/{}/realize", s["id"]), Some(json!({}))).await;
    add_time(&t, &c.admin, c.project, "2026-03-11", 120).await; // client direct 2 h x 80
    add_time(&t, &c.admin, c.project, "2025-12-15", 60).await; // hors période
    // Facture mensuelle du centre, validée et payée.
    let inv = ok(&t, &c.admin, "POST", "/api/billing/monthly", Some(json!({ "partner_id": centre, "month": "2026-03", "invoice_date": "2026-03-31" }))).await;
    let inv = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", inv["id"]), None).await;
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/payments", inv["id"]), Some(json!({ "payment_date": "2026-04-15", "amount": inv["total_ttc"], "method": "VIREMENT" }))).await;

    let r = ok(&t, &c.admin, "GET", "/api/stats?from=2026-01-01&to=2026-12-31&group_by=centre", None).await;
    let rows = r["rows"].as_array().unwrap();
    let g = rows.iter().find(|x| x["key"].as_str() == Some(centre.to_string().as_str())).unwrap();
    assert_eq!(g["label"], "ONE LEARN");
    assert_eq!(g["minutes"], 180);
    assert_eq!(g["missions"], 1);
    assert_eq!(g["participants"], 1);
    assert_eq!(g["sessions"], 1);
    assert_eq!(g["invoiced_ht"], 15000);
    assert_eq!(g["paid"], 15000);
    let direct = rows.iter().find(|x| x["label"] == "Sans centre (client direct)").unwrap();
    assert_eq!(direct["minutes"], 120);
    assert_eq!(r["total"]["minutes"], 300);
    // Filtre OS + regroupement par logiciel ; période par défaut = année en cours (ici 2026).
    let r = ok(&t, &c.admin, "GET", "/api/stats?from=2026-01-01&to=2026-12-31&group_by=logiciel&os=MAC", None).await;
    assert_eq!(r["rows"].as_array().unwrap().len(), 1);
    assert_eq!(r["total"]["minutes"], 180);
    let (st, _, _) = t.call("GET", "/api/stats?from=2026-12-01&to=2026-01-01", Some(&c.admin), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // Exports CSV.
    for ds in ["synthese", "timesheets", "missions", "invoices", "payments"] {
        let (st, body, _) = t.call("GET", &format!("/api/stats/export.csv?from=2026-01-01&to=2026-12-31&dataset={ds}"), Some(&c.admin), None).await;
        assert_eq!(st, StatusCode::OK, "{ds} {body}");
        let txt = body.as_str().unwrap_or("");
        assert!(txt.starts_with('\u{feff}'), "{ds}: BOM attendu");
        assert!(txt.lines().count() >= 2, "{ds}: au moins une ligne de données");
    }
}

#[tokio::test]
// Produits : liste modifiable, pré-remplissage côté facture (product_id conservé), TVA optionnelle en franchise.
async fn produits_et_tva_optionnelle() {
    let t = app().await;
    let c = setup(&t).await;
    // Produits par défaut, puis ajout / modification / doublon.
    let list = ok(&t, &c.admin, "GET", "/api/products", None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["unit_price"], 5000);
    let p = ok(&t, &c.admin, "POST", "/api/products", Some(json!({ "name": "Support de cours", "unit": "u", "unit_price": 2500, "vat_rate_bp": 2000 }))).await;
    let (s, _, _) = t.call("POST", "/api/products", Some(&c.admin), Some(json!({ "name": "support de cours" }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let p2 = ok(&t, &c.admin, "PATCH", &format!("/api/products/{}", p["id"]), Some(json!({ "unit_price": 3000, "active": false }))).await;
    assert_eq!(p2["unit_price"], 3000);
    assert_eq!(p2["active"], false);
    let (s, _, _) = t.call("PATCH", &format!("/api/products/{}", p["id"]), Some(&c.admin), Some(json!({ "vat_rate_bp": 12000 }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Facture en franchise avec une ligne SANS TVA et une ligne AVEC TVA explicite (produit à 20 %).
    let inv = ok(&t, &c.admin, "POST", "/api/invoices", Some(json!({ "partner_id": c.client, "lines": [
        { "product_id": 1, "description": "Formation - heure", "quantity_milli": 2000, "unit": "h", "unit_price": 5000 },
        { "product_id": p["id"], "description": "Support de cours", "quantity_milli": 1000, "unit": "u", "unit_price": 3000, "vat_rate_bp": 2000 }
    ] }))).await;
    assert_eq!(inv["lines"][0]["product_id"], 1);
    assert_eq!(inv["lines"][0]["vat_rate_bp"], 0);
    assert_eq!(inv["lines"][1]["total_vat"], 600);
    assert_eq!(inv["total_ht"], 13000);
    assert_eq!(inv["total_ttc"], 13600);
    // Validée : la mention d'exonération n'est pas imprimée puisque la facture porte de la TVA.
    let v = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", inv["id"]), None).await;
    assert_eq!(v["vat_mention"], "");
    // Facture sans TVA : la mention reste.
    let inv2 = ok(&t, &c.admin, "POST", "/api/invoices", Some(json!({ "partner_id": c.client, "lines": [{ "description": "Heure", "quantity_milli": 1000, "unit_price": 5000 }] }))).await;
    let v2 = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", inv2["id"]), None).await;
    assert_eq!(v2["vat_mention"], "TVA non applicable, art. 293 B du CGI");
    // Droits : lecture pour tous, modification réservée.
    let (_, user) = t.create_user(&c.admin, "u@demo.fr", "UTILISATEUR").await;
    assert_eq!(t.call("GET", "/api/products", Some(&user), None).await.0, StatusCode::OK);
    assert_eq!(t.call("POST", "/api/products", Some(&user), Some(json!({ "name": "X" }))).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
// Dépenses : saisie, TTC calculé, statut à payer -> payée, justificatif, filtres, export, tableau de bord, droits.
async fn depenses_de_l_entreprise() {
    let t = app().await;
    let c = setup(&t).await;
    let m = ok(&t, &c.admin, "POST", "/api/missions", Some(json!({ "title": "Revit", "client_id": c.client, "trainer_id": c.me }))).await;
    let e1 = ok(&t, &c.admin, "POST", "/api/depenses", Some(json!({ "date": "2026-11-03", "label": "Abonnement Autodesk", "category": "LOGICIEL", "supplier_name": "Autodesk", "amount_ht": 25000, "vat_amount": 5000, "payment_method": "CARTE" }))).await;
    assert_eq!(e1["amount_ttc"], 30000);
    assert_eq!(e1["status"], "PAYEE");
    assert_eq!(e1["payment_date"], "2026-11-03");
    assert_eq!(e1["supplier_display"], "Autodesk");
    let e2 = ok(&t, &c.admin, "POST", "/api/depenses", Some(json!({ "date": "2026-11-10", "label": "Train Bordeaux-Paris", "category": "DEPLACEMENT", "supplier_id": c.client, "amount_ht": 9000, "status": "A_PAYER", "project_id": m["project_id"] }))).await;
    assert_eq!(e2["status"], "A_PAYER");
    assert_eq!(e2["mission_id"], m["id"]);
    assert_eq!(e2["supplier_display"], "Demo Industrie SAS");
    // Contrôles de saisie.
    for bad in [json!({ "date": "2026-11-03", "label": "", "amount_ht": 100 }), json!({ "date": "03/11/2026", "label": "x", "amount_ht": 100 }), json!({ "date": "2026-11-03", "label": "x", "amount_ht": 0 }), json!({ "date": "2026-11-03", "label": "x", "amount_ht": 100, "category": "INCONNUE" })] {
        let (s, _, _) = t.call("POST", "/api/depenses", Some(&c.admin), Some(bad)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    // Liste, totaux, ventilation, filtres.
    let l = ok(&t, &c.admin, "GET", "/api/depenses?from=2026-11-01&to=2026-11-30", None).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 2);
    assert_eq!(l["total_ttc"], 39000);
    assert_eq!(l["to_pay"], 9000);
    assert_eq!(l["by_category"][0]["category"], "LOGICIEL");
    assert_eq!(l["by_month"][0]["month"], "2026-11");
    let l = ok(&t, &c.admin, "GET", "/api/depenses?status=A_PAYER", None).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);
    let l = ok(&t, &c.admin, "GET", "/api/depenses?q=autodesk", None).await;
    assert_eq!(l["items"][0]["id"], e1["id"]);
    // Paiement.
    let p = ok(&t, &c.admin, "POST", &format!("/api/depenses/{}/pay", e2["id"]), Some(json!({ "payment_date": "2026-11-20" }))).await;
    assert_eq!(p["status"], "PAYEE");
    // Justificatif : envoi, téléchargement, remplacement.
    let (s, r) = t.upload(&format!("/api/depenses/{}/receipt", e1["id"]), &c.admin, "facture autodesk.pdf", b"%PDF-1.4 test").await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["receipt_name"], "facture autodesk.pdf");
    let (s, body, _) = t.call("GET", &format!("/api/depenses/{}/receipt", e1["id"]), Some(&c.admin), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.as_str().unwrap().starts_with("%PDF"));
    let (s, _, _) = t.call("GET", &format!("/api/depenses/{}/receipt", e2["id"]), Some(&c.admin), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Modification et export.
    let mut upd = json!({ "date": "2026-11-03", "label": "Abonnement Autodesk AEC", "category": "LOGICIEL", "supplier_name": "Autodesk", "amount_ht": 25000, "vat_amount": 5000, "payment_method": "CARTE", "status": "PAYEE" });
    upd["deductible"] = json!(false);
    let u = ok(&t, &c.admin, "PUT", &format!("/api/depenses/{}", e1["id"]), Some(upd)).await;
    assert_eq!(u["label"], "Abonnement Autodesk AEC");
    assert_eq!(u["deductible"], false);
    let (s, csv, _) = t.call("GET", "/api/depenses/export.csv?from=2026-11-01&to=2026-11-30", Some(&c.admin), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(csv.as_str().unwrap().lines().count(), 3);
    // Tableau de bord et statistiques.
    let d = ok(&t, &c.admin, "GET", "/api/dashboard", None).await;
    assert_eq!(d["expenses_year"], 39000);
    let st = ok(&t, &c.admin, "GET", "/api/stats?from=2026-01-01&to=2026-12-31&group_by=mois", None).await;
    assert_eq!(st["total"]["expenses_ttc"], 39000);
    let st = ok(&t, &c.admin, "GET", "/api/stats?from=2026-01-01&to=2026-12-31&group_by=client", None).await;
    assert_eq!(st["total"]["expenses_ttc"], 9000); // seule la dépense liée à une mission
    // Suppression (justificatif effacé) et droits.
    let (s, _, _) = t.call("DELETE", &format!("/api/depenses/{}", e1["id"]), Some(&c.admin), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(std::fs::read_dir(t.state.paths.documents.join("depenses").join(e1["id"].to_string())).map(|d| d.count() == 0).unwrap_or(true));
    let (_, user) = t.create_user(&c.admin, "u@demo.fr", "UTILISATEUR").await;
    assert_eq!(t.call("GET", "/api/depenses", Some(&user), None).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
// État des lieux : brouillon, validée non envoyée, envoyée en attente, en retard (ancienneté), payée, reste dû par client.
async fn etat_des_lieux_des_factures() {
    let t = app().await;
    let c = setup(&t).await;
    let client2 = ok(&t, &c.admin, "POST", "/api/contacts", Some(json!({ "kind": "ENTREPRISE", "name": "Retardataire SARL", "is_client": true }))).await["id"].as_i64().unwrap();
    let today = chrono::Local::now().date_naive();
    let d = |days: i64| (today + chrono::Duration::days(days)).format("%Y-%m-%d").to_string();
    let mk = |partner: i64, date: String, due: String, amount: i64| json!({ "partner_id": partner, "invoice_date": date, "due_date": due, "lines": [{ "description": "Formation", "quantity_milli": 1000, "unit_price": amount }] });
    // Ordre chronologique imposé par la numérotation : la plus ancienne d'abord.
    // En retard de 45 jours (envoyée), payée partiellement.
    let r = ok(&t, &c.admin, "POST", "/api/invoices", Some(mk(client2, d(-75), d(-45), 40000))).await;
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", r["id"]), None).await;
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/sent", r["id"]), Some(json!({ "sent_at": d(-74) }))).await;
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/payments", r["id"]), Some(json!({ "amount": 10000, "method": "VIREMENT" }))).await;
    // Payée.
    let p = ok(&t, &c.admin, "POST", "/api/invoices", Some(mk(c.client, d(-20), d(10), 50000))).await;
    let p = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", p["id"]), None).await;
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/payments", p["id"]), Some(json!({ "amount": p["total_ttc"], "method": "VIREMENT", "payment_date": d(-10) }))).await;
    // Validée non envoyée, échéance future.
    let a = ok(&t, &c.admin, "POST", "/api/invoices", Some(mk(c.client, d(-5), d(25), 20000))).await;
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", a["id"]), None).await;
    // Envoyée, en attente.
    let b = ok(&t, &c.admin, "POST", "/api/invoices", Some(mk(c.client, d(-4), d(26), 30000))).await;
    ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", b["id"]), None).await;
    let sent = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/sent", b["id"]), Some(json!({}))).await;
    assert_eq!(sent["sent_at"], d(0));
    // Brouillon.
    let draft = ok(&t, &c.admin, "POST", "/api/invoices", Some(mk(c.client, d(0), d(30), 10000))).await;
    // Un brouillon ne peut pas être marqué envoyé.
    let (s, _, _) = t.call("POST", &format!("/api/invoices/{}/sent", draft["id"]), Some(&c.admin), Some(json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let o = ok(&t, &c.admin, "GET", "/api/invoices/overview", None).await;
    assert_eq!(o["drafts"]["count"], 1);
    assert_eq!(o["not_sent"]["count"], 1);
    assert_eq!(o["not_sent"]["amount"], 20000);
    assert_eq!(o["sent_waiting"]["count"], 1);
    assert_eq!(o["sent_waiting"]["amount"], 30000);
    assert_eq!(o["overdue"]["count"], 1);
    assert_eq!(o["overdue"]["amount"], 30000); // 40 000 - 10 000 payés
    assert_eq!(o["overdue_31_60"]["count"], 1);
    assert_eq!(o["overdue_1_30"]["count"], 0);
    assert_eq!(o["paid_year"]["count"], 1);
    assert_eq!(o["paid_year"]["amount"], 50000);
    assert_eq!(o["collected_year"], 60000);
    assert_eq!(o["avg_payment_days"], 10);
    assert_eq!(o["by_client"][0]["partner_name"], "Retardataire SARL");
    assert_eq!(o["by_client"][0]["overdue_due"], 30000);
    assert_eq!(o["pending"][0]["days_late"], 45);
    assert_eq!(o["pending"].as_array().unwrap().len(), 3);
}

#[tokio::test]
// Facturation électronique : activation (SIREN émetteur requis), SIREN client exigé en B2B, statut initial,
// XML CII et UBL, PDF Factur-X (XML embarqué + XMP), suivi de transmission, état des lieux.
async fn facturation_electronique() {
    let t = app().await;
    let c = setup(&t).await;
    // Activation refusée sans SIREN émetteur.
    let mut s = t.call("GET", "/api/settings/company", Some(&c.admin), None).await.1;
    s["einvoicing_enabled"] = json!(true);
    s["einvoicing_platform"] = json!("Ma plateforme agréée");
    let (st, _, _) = t.call("PUT", "/api/settings/company", Some(&c.admin), Some(s.clone())).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    s["siren"] = json!("944642917");
    s["address_line1"] = json!("5 allée des Formateurs");
    s["zip"] = json!("33000");
    s["city"] = json!("Bordeaux");
    s["iban"] = json!("FR7630006000011234567890189");
    ok(&t, &c.admin, "PUT", "/api/settings/company", Some(s)).await;
    // Client B2B sans SIREN : validation refusée.
    let inv = ok(&t, &c.admin, "POST", "/api/invoices", Some(json!({ "partner_id": c.client, "lines": [{ "product_id": 1, "description": "Formation AutoCAD - 7 h", "quantity_milli": 7000, "unit": "h", "unit_price": 5000 }] }))).await;
    assert_eq!(inv["operation_category"], "SERVICES");
    let (st, body, _) = t.call("POST", &format!("/api/invoices/{}/validate", inv["id"]), Some(&c.admin), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("SIREN"));
    // XML indisponible pour un brouillon.
    let (st, _, _) = t.call("GET", &format!("/api/invoices/{}/xml", inv["id"]), Some(&c.admin), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // SIREN renseigné sur le client -> validation OK, statut A_TRANSMETTRE, SIREN figé.
    let mut cl = ok(&t, &c.admin, "GET", &format!("/api/contacts/{}", c.client), None).await;
    cl["siren"] = json!("552100554");
    cl["tag_ids"] = json!([]);
    ok(&t, &c.admin, "PUT", &format!("/api/contacts/{}", c.client), Some(cl)).await;
    let v = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", inv["id"]), None).await;
    assert_eq!(v["einvoice_status"], "A_TRANSMETTRE");
    assert_eq!(v["billing_siren"], "552100554");
    // CII bien formé avec les mentions attendues.
    let (st, cii, _) = t.call("GET", &format!("/api/invoices/{}/xml?format=cii", inv["id"]), Some(&c.admin), None).await;
    assert_eq!(st, StatusCode::OK);
    let cii = cii.as_str().unwrap().to_string();
    for needle in ["<ram:TypeCode>380</ram:TypeCode>", "urn:cen.eu:en16931:2017", "schemeID=\"0002\">944642917<", "schemeID=\"0002\">552100554<",
        "<ram:BilledQuantity unitCode=\"HUR\">7.000</ram:BilledQuantity>", "<ram:GrandTotalAmount>350.00</ram:GrandTotalAmount>",
        "VATEX-FR-FRANCHISE", "<ram:IBANID>FR7630006000011234567890189</ram:IBANID>", "<ram:ID>A2</ram:ID>"] {
        assert!(cii.contains(needle), "CII sans {needle}\n{cii}");
    }
    let (st, ubl, _) = t.call("GET", &format!("/api/invoices/{}/xml?format=ubl", inv["id"]), Some(&c.admin), None).await;
    assert_eq!(st, StatusCode::OK);
    let ubl = ubl.as_str().unwrap().to_string();
    for needle in ["<Invoice xmlns=\"urn:oasis:names:specification:ubl:schema:xsd:Invoice-2\"", "<cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>",
        "<cbc:PayableAmount currencyID=\"EUR\">350.00</cbc:PayableAmount>", "<cbc:TaxExemptionReasonCode>VATEX-FR-FRANCHISE</cbc:TaxExemptionReasonCode>"] {
        assert!(ubl.contains(needle), "UBL sans {needle}\n{ubl}");
    }
    std::fs::write(std::env::temp_dir().join("mercure-test-cii.xml"), &cii).unwrap();
    std::fs::write(std::env::temp_dir().join("mercure-test-ubl.xml"), &ubl).unwrap();
    // PDF hybride : XML embarqué et métadonnées Factur-X.
    let (st, pdf, _) = t.call("GET", &format!("/api/invoices/{}/pdf", inv["id"]), Some(&c.admin), None).await;
    assert_eq!(st, StatusCode::OK);
    let pdf = pdf.as_str().unwrap().to_string();
    assert!(pdf.starts_with("%PDF-1.7"));
    for needle in ["/EmbeddedFiles", "(factur-x.xml)", "/AFRelationship /Alternative", "fx:ConformanceLevel>EN 16931<", "pdfaid:part>3<", "<rsm:CrossIndustryInvoice"] {
        assert!(pdf.contains(needle), "PDF sans {needle}");
    }
    std::fs::write(std::env::temp_dir().join("mercure-test-facturx.pdf"), pdf.as_bytes()).unwrap();
    // Suivi de transmission.
    let tr = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/einvoice", inv["id"]), Some(json!({ "status": "TRANSMISE", "reference": "PA-2026-0001" }))).await;
    assert_eq!(tr["einvoice_status"], "TRANSMISE");
    assert_eq!(tr["einvoice_ref"], "PA-2026-0001");
    let (st, _, _) = t.call("POST", &format!("/api/invoices/{}/einvoice", inv["id"]), Some(&c.admin), Some(json!({ "status": "N_IMPORTE" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // Client particulier : hors champ (e-reporting), pas de SIREN exigé.
    let part = ok(&t, &c.admin, "POST", "/api/contacts", Some(json!({ "kind": "PARTICULIER", "name": "MARTIN", "first_name": "Paul", "is_client": true }))).await["id"].as_i64().unwrap();
    let b2c = ok(&t, &c.admin, "POST", "/api/invoices", Some(json!({ "partner_id": part, "lines": [{ "description": "Cours particulier", "quantity_milli": 2000, "unit": "h", "unit_price": 4000 }] }))).await;
    let b2c = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/validate", b2c["id"]), None).await;
    assert_eq!(b2c["einvoice_status"], "HORS_CHAMP");
    // Avoir : type 381 et référence à la facture d'origine.
    let av = ok(&t, &c.admin, "POST", &format!("/api/invoices/{}/cancel", inv["id"]), Some(json!({ "reason": "test" }))).await;
    let av_id = av["id"].clone();
    assert_eq!(av["kind"], "AVOIR");
    let (_, cii_av, _) = t.call("GET", &format!("/api/invoices/{av_id}/xml"), Some(&c.admin), None).await;
    let cii_av = cii_av.as_str().unwrap();
    assert!(cii_av.contains("<ram:TypeCode>381</ram:TypeCode>"));
    assert!(cii_av.contains("<ram:InvoiceReferencedDocument><ram:IssuerAssignedID>FAC-"));
    // État des lieux.
    let o = ok(&t, &c.admin, "GET", "/api/invoices/overview", None).await;
    assert_eq!(o["einvoicing_enabled"], true);
    assert_eq!(o["einvoice_sent"]["count"], 1); // la facture annulée reste transmise à la plateforme
    assert_eq!(o["einvoice_to_send"]["count"], 1); // l'avoir
    assert_eq!(o["einvoice_out_of_scope"]["count"], 1);
}
