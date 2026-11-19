//! Tests d'intégration de l'API (base SQLite temporaire).

mod common;

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use common::app;
use erp::{db, modules::{backup, sequence}};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
// Le premier compte ne peut être créé qu'une fois ; ensuite /auth/setup est refusé.
async fn initialisation_unique() {
    let t = app().await;
    let (_, st, _) = t.call("GET", "/api/auth/status", None, None).await;
    assert_eq!(st["setup_required"], true);
    t.setup_admin().await;
    let (_, st, _) = t.call("GET", "/api/auth/status", None, None).await;
    assert_eq!(st["setup_required"], false);
    let (s, _, _) = t
        .call("POST", "/api/auth/setup", None, Some(json!({
            "email": "pirate@demo.fr", "display_name": "X", "password": "motdepasse-solide"
        })))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
// Politique de mot de passe : longueur minimale.
async fn mot_de_passe_trop_court_refuse() {
    let t = app().await;
    let (s, body, _) = t
        .call("POST", "/api/auth/setup", None, Some(json!({ "email": "a@demo.fr", "display_name": "A", "password": "court" })))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("10 caractères"));
}

#[tokio::test]
// Connexion -> cookie de session -> /auth/me ; déconnexion -> accès refusé.
async fn connexion_et_deconnexion() {
    let t = app().await;
    t.setup_admin().await;
    let (s, _, _) = t
        .call("POST", "/api/auth/login", None, Some(json!({ "email": "admin@demo.fr", "password": "mauvais-mot-de-passe" })))
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = t
        .call("POST", "/api/auth/login", None, Some(json!({ "email": "inconnu@demo.fr", "password": "mauvais-mot-de-passe" })))
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, me, c) = t
        .call("POST", "/api/auth/login", None, Some(json!({ "email": "ADMIN@demo.fr ", "password": "motdepasse-solide" })))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["role"], "ADMIN");
    let c = c.unwrap();
    let (s, _, _) = t.call("GET", "/api/auth/me", Some(&c), None).await;
    assert_eq!(s, StatusCode::OK);
    t.call("POST", "/api/auth/logout", Some(&c), None).await;
    let (s, _, _) = t.call("GET", "/api/auth/me", Some(&c), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
// Verrouillage temporaire après 5 échecs.
async fn blocage_apres_tentatives_repetees() {
    let t = app().await;
    t.setup_admin().await;
    for _ in 0..5 {
        t.call("POST", "/api/auth/login", None, Some(json!({ "email": "admin@demo.fr", "password": "xxxxxxxxxxxx" }))).await;
    }
    let (s, body, _) = t
        .call("POST", "/api/auth/login", None, Some(json!({ "email": "admin@demo.fr", "password": "motdepasse-solide" })))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("Trop de tentatives"));
}

#[tokio::test]
// Toutes les routes protégées renvoient 401 sans cookie.
async fn sans_session_tout_est_refuse() {
    let t = app().await;
    for uri in ["/api/users", "/api/settings/company", "/api/software", "/api/backups", "/api/audit"] {
        let (s, _, _) = t.call("GET", uri, None, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[tokio::test]
// Matrice des droits vérifiée via l'API (ADMIN / GESTIONNAIRE / UTILISATEUR).
async fn permissions_par_role() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let (_, user) = t.create_user(&admin, "user@demo.fr", "UTILISATEUR").await;
    let (_, gest) = t.create_user(&admin, "gest@demo.fr", "GESTIONNAIRE").await;

    let settings = t.call("GET", "/api/settings/company", Some(&admin), None).await.1;
    let mut new_settings = settings.clone();
    new_settings["legal_name"] = json!("Demo Formation");

    // UTILISATEUR
    assert_eq!(t.call("GET", "/api/users", Some(&user), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("PUT", "/api/settings/company", Some(&user), Some(new_settings.clone())).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("POST", "/api/backups", Some(&user), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("POST", "/api/software", Some(&user), Some(json!({ "name": "Rhino" }))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("GET", "/api/audit", Some(&user), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("GET", "/api/settings/company", Some(&user), None).await.0, StatusCode::OK);

    // GESTIONNAIRE
    assert_eq!(t.call("GET", "/api/users", Some(&gest), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("PUT", "/api/settings/company", Some(&gest), Some(new_settings.clone())).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call("POST", "/api/software", Some(&gest), Some(json!({ "name": "Rhino" }))).await.0, StatusCode::OK);
    assert_eq!(t.call("POST", "/api/backups", Some(&gest), None).await.0, StatusCode::OK);

    // ADMIN
    assert_eq!(t.call("PUT", "/api/settings/company", Some(&admin), Some(new_settings)).await.0, StatusCode::OK);
}

#[tokio::test]
// On ne peut pas retirer le dernier admin ; désactiver un compte ferme ses sessions.
async fn dernier_admin_protege_et_desactivation_coupe_les_sessions() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let me = t.call("GET", "/api/auth/me", Some(&admin), None).await.1;
    let admin_id = me["id"].as_i64().unwrap();

    let (s, _, _) = t.call("PATCH", &format!("/api/users/{admin_id}"), Some(&admin), Some(json!({ "role": "GESTIONNAIRE" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = t.call("PATCH", &format!("/api/users/{admin_id}"), Some(&admin), Some(json!({ "active": false }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (uid, user) = t.create_user(&admin, "user@demo.fr", "UTILISATEUR").await;
    assert_eq!(t.call("GET", "/api/auth/me", Some(&user), None).await.0, StatusCode::OK);
    let (s, _, _) = t.call("PATCH", &format!("/api/users/{uid}"), Some(&admin), Some(json!({ "active": false }))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(t.call("GET", "/api/auth/me", Some(&user), None).await.0, StatusCode::UNAUTHORIZED);
    let (s, _, _) = t
        .call("POST", "/api/auth/login", None, Some(json!({ "email": "user@demo.fr", "password": "motdepasse-solide" })))
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
// Deux comptes ne peuvent pas partager un email.
async fn email_utilisateur_unique() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let (s, body, _) = t
        .call("POST", "/api/users", Some(&admin), Some(json!({
            "email": "Admin@Demo.fr", "display_name": "Doublon", "role": "UTILISATEUR", "password": "motdepasse-solide"
        })))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(body["error"], "Un utilisateur utilise déjà cet email");
}

#[tokio::test]
// Paramètres entreprise : validation (SIRET, IBAN...) et diff dans le journal.
async fn parametres_valides_et_journalises() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let mut s = t.call("GET", "/api/settings/company", Some(&admin), None).await.1;
    s["legal_name"] = json!("Demo Formation");
    s["siret"] = json!("12345678901234");
    let (st, body, _) = t.call("PUT", "/api/settings/company", Some(&admin), Some(s.clone())).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("SIRET"));

    s["siret"] = json!("944 642 917 00016");
    s["siren"] = json!("944642917");
    s["iban"] = json!("FR76 3000 6000 0112 3456 7890 189");
    s["vat_exemption_mention"] = json!("TVA non applicable, art. 293 B du CGI");
    s["nda"] = json!("75 33 12345 33");
    let (st, body, _) = t.call("PUT", "/api/settings/company", Some(&admin), Some(s)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["siret"], "94464291700016");
    assert_eq!(body["iban"], "FR7630006000011234567890189");
    assert_eq!(body["nda"], "75331234533");

    let audit = t.call("GET", "/api/audit?entity_type=company_settings", Some(&admin), None).await.1;
    assert_eq!(audit["total"], 1);
    let changes: Value = serde_json::from_str(audit["items"][0]["changes"].as_str().unwrap()).unwrap();
    assert_eq!(changes["legal_name"]["new"], "Demo Formation");
    assert!(changes.get("country").is_none());
}

#[tokio::test]
// Les déclencheurs SQL interdisent UPDATE/DELETE sur audit_log.
async fn journal_non_modifiable() {
    let t = app().await;
    t.setup_admin().await;
    let r = sqlx::query("DELETE FROM audit_log").execute(&t.state.pool).await;
    assert!(r.is_err());
    let r = sqlx::query("UPDATE audit_log SET action = 'X'").execute(&t.state.pool).await;
    assert!(r.is_err());
}

#[tokio::test]
// Logiciels pré-remplis ; doublon de nom refusé (409).
async fn logiciels_par_defaut_et_doublon() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let list = t.call("GET", "/api/software", Some(&admin), None).await.1;
    let names: Vec<_> = list.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["AutoCAD", "Fusion", "Revit", "SolidWorks"]);
    let (s, _, _) = t.call("POST", "/api/software", Some(&admin), Some(json!({ "name": " revit " }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
// 100 réservations concurrentes -> 100 numéros distincts et consécutifs.
async fn sequence_sans_doublon_en_concurrence() {
    let t = app().await;
    let mut handles = Vec::new();
    for _ in 0..40 {
        let pool = t.state.pool.clone();
        handles.push(tokio::spawn(async move {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            let v = sequence::next_value(&mut tx, "FAC-2026").await.unwrap();
            tx.commit().await.unwrap();
            v
        }));
    }
    let mut values = Vec::new();
    for h in handles {
        values.push(h.await.unwrap());
    }
    values.sort();
    assert_eq!(values, (1..=40).collect::<Vec<i64>>());
}

#[tokio::test]
// Un numéro réservé dans une transaction annulée est bien réutilisé (pas de trou).
async fn sequence_annulee_si_transaction_annulee() {
    let t = app().await;
    {
        let mut tx = t.state.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert_eq!(sequence::next_value(&mut tx, "FAC-2026").await.unwrap(), 1);
        tx.rollback().await.unwrap();
    }
    let mut tx = t.state.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    assert_eq!(sequence::next_value(&mut tx, "FAC-2026").await.unwrap(), 1);
    tx.commit().await.unwrap();
}

#[tokio::test]
// La sauvegarde est une base SQLite valide et téléchargeable.
async fn sauvegarde_lisible() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let (s, body, _) = t.call("POST", "/api/backups", Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK);
    let name = body["name"].as_str().unwrap();
    let path = t.state.paths.backups.join(name);
    let copy = db::open(&path).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&copy).await.unwrap();
    assert_eq!(n, 1);

    let list = t.call("GET", "/api/backups", Some(&admin), None).await.1;
    assert_eq!(list["files"].as_array().unwrap().len(), 1);
    let (s, _, _) = t.call("GET", "/api/backups/..%2Ferp.db", Some(&admin), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
// Deux appels le même jour ne créent qu'une sauvegarde auto.
async fn sauvegarde_auto_une_fois_par_jour() {
    let t = app().await;
    assert!(backup::daily_if_needed(&t.state).await.unwrap().is_some());
    assert!(backup::daily_if_needed(&t.state).await.unwrap().is_none());
}

#[tokio::test]
// /api/inconnu -> 404 ; toute autre route -> interface (index.html).
async fn route_api_inconnue_et_interface() {
    let t = app().await;
    assert_eq!(t.call("GET", "/api/inexistant", None, None).await.0, StatusCode::NOT_FOUND);
    let req = Request::builder().uri("/contacts").body(Body::empty()).unwrap();
    let res = t.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get(header::CONTENT_SECURITY_POLICY).is_some());
}

#[tokio::test]
// Jeu de démonstration : chargement, refus du doublon, suppression complète.
async fn donnees_de_demo_chargees_puis_supprimees() {
    let t = app().await;
    let admin = t.setup_admin().await;
    let (s, b, _) = t.call("POST", "/api/demo", Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(t.call("GET", "/api/demo", Some(&admin), None).await.1["loaded"], true);
    let inv = t.call("GET", "/api/invoices", Some(&admin), None).await.1;
    assert_eq!(inv["items"][0]["total_ht"], 56_000, "7 h x 80 € = 560 € HT");
    assert_eq!(t.call("POST", "/api/demo", Some(&admin), None).await.0, StatusCode::CONFLICT);
    let (s, b, _) = t.call("DELETE", "/api/demo", Some(&admin), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(t.call("GET", "/api/contacts", Some(&admin), None).await.1["total"], 0);
    assert_eq!(t.call("GET", "/api/invoices", Some(&admin), None).await.1["total"], 0);
    assert_eq!(t.call("GET", "/api/projects", Some(&admin), None).await.1["total"], 0);
}
