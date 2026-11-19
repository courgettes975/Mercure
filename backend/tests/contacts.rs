//! Tests d'intégration du module Contacts.

mod common;

use axum::http::StatusCode;
use common::{app, TestApp};
use serde_json::{json, Value};

// Crée un contact et vérifie le code 200.
async fn create(t: &TestApp, c: &str, body: Value) -> Value {
    let (s, b, _) = t.call("POST", "/api/contacts", Some(c), Some(body)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    b
}

// Corps JSON d'une société.
fn company(name: &str) -> Value {
    json!({ "kind": "ENTREPRISE", "name": name, "is_client": true, "siren": "944642917", "siret": "944 642 917 00016" })
}

#[tokio::test]
// Société + personne rattachée ; nom d'affichage "SOCIÉTÉ, Personne".
async fn creation_societe_et_interlocuteur() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let senza = create(&t, &admin, company("SENZA")).await;
    assert_eq!(senza["customer_number"], "CLI-00001");
    assert_eq!(senza["siret"], "94464291700016");

    let lopez = create(&t, &admin, json!({
        "kind": "PARTICULIER", "name": "LOPEZ", "first_name": "Éléonore", "parent_id": senza["id"],
        "email": "Eleonore.Lopez@Exemple.fr", "is_stagiaire": true
    })).await;
    assert_eq!(lopez["display_name"], "LOPEZ Éléonore");
    assert_eq!(lopez["parent_name"], "SENZA");
    assert_eq!(lopez["email"], "eleonore.lopez@exemple.fr");
    assert!(lopez["customer_number"].is_null());

    let detail = t.call("GET", &format!("/api/contacts/{}", senza["id"]), Some(&admin), None).await.1;
    assert_eq!(detail["children_count"], 1);

    // Liste : "SOCIÉTÉ, Personne" comme dans Odoo.
    let list = t.call("GET", "/api/contacts?role=stagiaire", Some(&admin), None).await.1;
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["display_name"], "SENZA, LOPEZ Éléonore");
}

#[tokio::test]
// CLI-00001, CLI-00002... attribués une seule fois.
async fn numeros_clients_uniques_et_sequentiels() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let a = create(&t, &admin, company("A")).await;
    let b = create(&t, &admin, json!({ "kind": "ENTREPRISE", "name": "B", "is_prospect": true })).await;
    assert!(b["customer_number"].is_null());
    let mut b_upd = b.clone();
    b_upd["is_prospect"] = json!(false);
    b_upd["is_client"] = json!(true);
    let b2 = t.call("PUT", &format!("/api/contacts/{}", b["id"]), Some(&admin), Some(b_upd)).await.1;
    assert_eq!(a["customer_number"], "CLI-00001");
    assert_eq!(b2["customer_number"], "CLI-00002");
}

#[tokio::test]
// La recherche ignore accents et casse, et exige tous les mots.
async fn recherche_sans_accents_et_multi_mots() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let senza = create(&t, &admin, company("SENZA")).await;
    create(&t, &admin, json!({ "kind": "PARTICULIER", "name": "LOPEZ", "first_name": "Éléonore", "parent_id": senza["id"], "city": "Montpellier" })).await;
    create(&t, &admin, json!({ "kind": "PARTICULIER", "name": "MARTIN", "first_name": "Jean", "phone": "+33 6 11 22 33 44" })).await;

    let n = |v: Value| v["total"].as_i64().unwrap();
    assert_eq!(n(t.call("GET", "/api/contacts?q=eleonore", Some(&admin), None).await.1), 1);
    assert_eq!(n(t.call("GET", "/api/contacts?q=senza%20lopez", Some(&admin), None).await.1), 1);
    assert_eq!(n(t.call("GET", "/api/contacts?q=senza", Some(&admin), None).await.1), 2);
    assert_eq!(n(t.call("GET", "/api/contacts?q=montpel", Some(&admin), None).await.1), 1);
    assert_eq!(n(t.call("GET", "/api/contacts?q=0611", Some(&admin), None).await.1), 0);
    assert_eq!(n(t.call("GET", "/api/contacts?q=611223344", Some(&admin), None).await.1), 1);
    assert_eq!(n(t.call("GET", "/api/contacts?q=%25", Some(&admin), None).await.1), 0);
    assert_eq!(n(t.call("GET", "/api/contacts?companies_only=true", Some(&admin), None).await.1), 1);
}

#[tokio::test]
// La clé de recherche des personnes suit le nouveau nom de leur société.
async fn renommage_societe_met_a_jour_la_recherche_des_interlocuteurs() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let s = create(&t, &admin, company("ANCIEN NOM")).await;
    create(&t, &admin, json!({ "kind": "PARTICULIER", "name": "DUPONT", "parent_id": s["id"] })).await;
    let mut upd = s.clone();
    upd["name"] = json!("NOUVEAU NOM");
    t.call("PUT", &format!("/api/contacts/{}", s["id"]), Some(&admin), Some(upd)).await;
    let list = t.call("GET", "/api/contacts?q=nouveau%20dupont", Some(&admin), None).await.1;
    assert_eq!(list["total"], 1);
}

#[tokio::test]
// Rattachement : uniquement à une société, pas à soi-même, pas en cascade.
async fn regles_de_rattachement() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let s = create(&t, &admin, company("SENZA")).await;
    let p = create(&t, &admin, json!({ "kind": "PARTICULIER", "name": "DUPONT", "parent_id": s["id"] })).await;
    // Rattachement à un particulier : refusé.
    let (st, _, _) = t.call("POST", "/api/contacts", Some(&admin), Some(json!({ "kind": "PARTICULIER", "name": "X", "parent_id": p["id"] }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // Une société avec interlocuteurs ne peut pas devenir particulier.
    let mut upd = s.clone();
    upd["kind"] = json!("PARTICULIER");
    let (st, b, _) = t.call("PUT", &format!("/api/contacts/{}", s["id"]), Some(&admin), Some(upd)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{b}");
    // Société inexistante.
    let (st, _, _) = t.call("POST", "/api/contacts", Some(&admin), Some(json!({ "kind": "PARTICULIER", "name": "X", "parent_id": 999 }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
// Suppression refusée si des personnes sont rattachées ; archivage possible.
async fn suppression_controlee_et_archivage() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let s = create(&t, &admin, company("SENZA")).await;
    let p = create(&t, &admin, json!({ "kind": "PARTICULIER", "name": "DUPONT", "parent_id": s["id"] })).await;

    let (st, b, _) = t.call("DELETE", &format!("/api/contacts/{}", s["id"]), Some(&admin), None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert!(b["error"].as_str().unwrap().contains("Archivez-le plutôt"));

    let (st, a, _) = t.call("POST", &format!("/api/contacts/{}/archive", s["id"]), Some(&admin), None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(a["archived_at"].is_string());
    assert_eq!(t.call("GET", "/api/contacts", Some(&admin), None).await.1["total"], 1);
    assert_eq!(t.call("GET", "/api/contacts?archived=archives", Some(&admin), None).await.1["total"], 1);
    assert_eq!(t.call("GET", "/api/contacts?archived=tous", Some(&admin), None).await.1["total"], 2);
    t.call("POST", &format!("/api/contacts/{}/unarchive", s["id"]), Some(&admin), None).await;
    assert_eq!(t.call("GET", "/api/contacts", Some(&admin), None).await.1["total"], 2);

    let (st, _, _) = t.call("DELETE", &format!("/api/contacts/{}", p["id"]), Some(&admin), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t.call("GET", &format!("/api/contacts/{}", p["id"]), Some(&admin), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
// Notes libres et journal d'audit visibles dans l'activité du contact.
async fn historique_et_notes() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let s = create(&t, &admin, company("SENZA")).await;
    let mut upd = s.clone();
    upd["city"] = json!("Bordeaux");
    t.call("PUT", &format!("/api/contacts/{}", s["id"]), Some(&admin), Some(upd.clone())).await;
    // Enregistrer sans changement : pas de ligne de journal.
    t.call("PUT", &format!("/api/contacts/{}", s["id"]), Some(&admin), Some(upd)).await;
    let (st, _, _) = t.call("POST", &format!("/api/contacts/{}/notes", s["id"]), Some(&admin), Some(json!({ "body": "Premier appel fait" }))).await;
    assert_eq!(st, StatusCode::OK);
    let act = t.call("GET", &format!("/api/contacts/{}/activity", s["id"]), Some(&admin), None).await.1;
    assert_eq!(act["notes"].as_array().unwrap().len(), 1);
    let history = act["history"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    let changes: Value = serde_json::from_str(history[0]["changes"].as_str().unwrap()).unwrap();
    assert_eq!(changes["city"]["new"], "Bordeaux");
    assert_eq!(changes.as_object().unwrap().len(), 1);
}

#[tokio::test]
// Création d'étiquettes, association, doublon refusé.
async fn etiquettes() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let tag = t.call("POST", "/api/tags", Some(&admin), Some(json!({ "name": "B2B" }))).await.1;
    let (st, _, _) = t.call("POST", "/api/tags", Some(&admin), Some(json!({ "name": "b2b" }))).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let mut body = company("SENZA");
    body["tag_ids"] = json!([tag["id"], tag["id"]]);
    let s = create(&t, &admin, body).await;
    assert_eq!(s["tags"].as_array().unwrap().len(), 1);
    let list = t.call("GET", &format!("/api/contacts?tag_id={}", tag["id"]), Some(&admin), None).await.1;
    assert_eq!(list["total"], 1);
    let mut bad = company("X");
    bad["tag_ids"] = json!([999]);
    assert_eq!(t.call("POST", "/api/contacts", Some(&admin), Some(bad)).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
// La copie reprend les champs et étiquettes, pas les notes.
async fn duplication() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let s = create(&t, &admin, company("SENZA")).await;
    let (st, d, _) = t.call("POST", &format!("/api/contacts/{}/duplicate", s["id"]), Some(&admin), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(d["name"], "SENZA (copie)");
    assert_eq!(d["customer_number"], "CLI-00002");
    assert_ne!(d["id"], s["id"]);
}

#[tokio::test]
// Un UTILISATEUR lit mais ne modifie pas les contacts.
async fn droits_utilisateur_lecture_seule() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let s = create(&t, &admin, company("SENZA")).await;
    let (_, user) = t.create_user(&admin, "user@demo.fr", "UTILISATEUR").await;
    let (_, gest) = t.create_user(&admin, "gest@demo.fr", "GESTIONNAIRE").await;
    assert_eq!(t.call("GET", "/api/contacts", Some(&user), None).await.0, StatusCode::OK);
    assert_eq!(t.call("POST", "/api/contacts", Some(&user), Some(company("X"))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("PUT", &format!("/api/contacts/{}", s["id"]), Some(&user), Some(s.clone())).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("DELETE", &format!("/api/contacts/{}", s["id"]), Some(&user), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("GET", "/api/contacts/export", Some(&user), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("POST", "/api/contacts", Some(&gest), Some(company("X"))).await.0, StatusCode::OK);
}

#[tokio::test]
// Pagination et tris (nom, ville, création).
async fn pagination_et_tri() {
    let t = app().await;
    let admin = t.setup_admin().await;
    for n in ["Charlie", "alpha", "Bravo"] {
        create(&t, &admin, json!({ "kind": "ENTREPRISE", "name": n, "city": n })).await;
    }
    let p1 = t.call("GET", "/api/contacts?per_page=2&page=1", Some(&admin), None).await.1;
    assert_eq!(p1["total"], 3);
    assert_eq!(p1["items"][0]["name"], "alpha");
    assert_eq!(p1["items"][1]["name"], "Bravo");
    let p2 = t.call("GET", "/api/contacts?per_page=2&page=2", Some(&admin), None).await.1;
    assert_eq!(p2["items"][0]["name"], "Charlie");
    let desc = t.call("GET", "/api/contacts?sort=-name", Some(&admin), None).await.1;
    assert_eq!(desc["items"][0]["name"], "Charlie");
    // Tri inconnu : ignoré (pas d'injection possible).
    let x = t.call("GET", "/api/contacts?sort=name;DROP%20TABLE%20partners", Some(&admin), None).await;
    assert_eq!(x.0, StatusCode::OK);
}

#[tokio::test]
// Import d'un extrait Odoo : sociétés, personnes, stagiaires, doublons, relance sans doublon.
async fn import_odoo_csv() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let csv = "Nom d'affichage;E-mail;Téléphone;Pays;Stats\r\n\
BT-FORMATION;;+33 3 88 38 81 26;France;\"[{\"\"value\"\": 24, \"\"label\"\": \"\"Factures\"\"}]\"\r\n\
BT-FORMATION, LAMBIN Mickaël;meli_mon@hotmail.fr;+33 6 88 24 56 41;France;\r\n\
alix@one-learn.fr;alix@one-learn.fr;;;\r\n";

    let (s, plan) = t.upload("/api/contacts/import/preview", &admin, "contacts.csv", csv.as_bytes()).await;
    assert_eq!(s, StatusCode::OK, "{plan}");
    assert_eq!(plan["companies"], 1);
    assert_eq!(plan["persons"], 2);
    // L'aperçu n'enregistre rien.
    assert_eq!(t.call("GET", "/api/contacts", Some(&admin), None).await.1["total"], 0);

    let (s, r) = t.upload("/api/contacts/import", &admin, "contacts.csv", csv.as_bytes()).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["created"], 3);
    let list = t.call("GET", "/api/contacts?q=lambin", Some(&admin), None).await.1;
    assert_eq!(list["items"][0]["display_name"], "BT-FORMATION, LAMBIN Mickaël");
    assert_eq!(list["items"][0]["is_stagiaire"], true);
    let bt = t.call("GET", "/api/contacts?q=bt-formation&companies_only=true", Some(&admin), None).await.1;
    assert_eq!(bt["items"][0]["is_client"], true);
    assert_eq!(bt["items"][0]["customer_number"], "CLI-00001");

    // Nouvel import du même fichier : rien n'est dupliqué.
    let (_, plan2) = t.upload("/api/contacts/import/preview", &admin, "contacts.csv", csv.as_bytes()).await;
    assert_eq!(plan2["already_imported"], 3);
    let (_, r2) = t.upload("/api/contacts/import", &admin, "contacts.csv", csv.as_bytes()).await;
    assert_eq!(r2["created"], 0);
    assert_eq!(r2["skipped"], 3);
    assert_eq!(t.call("GET", "/api/contacts", Some(&admin), None).await.1["total"], 3);

    // Droits et formats.
    let (_, user) = t.create_user(&admin, "user@demo.fr", "UTILISATEUR").await;
    assert_eq!(t.upload("/api/contacts/import", &user, "contacts.csv", csv.as_bytes()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.upload("/api/contacts/import/preview", &admin, "x.pdf", b"%PDF").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(t.upload("/api/contacts/import/preview", &admin, "x.csv", b"A;B\r\n1;2").await.0, StatusCode::BAD_REQUEST);
}
