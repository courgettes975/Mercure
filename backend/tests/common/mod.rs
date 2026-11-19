//! Outils partagés par les tests d'intégration.
#![allow(dead_code)]

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use erp::{
    auth::LoginGuard,
    build_router, db,
    state::{AppState, DataPaths},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

// Application de test : routeur Axum en mémoire + base SQLite temporaire.
pub struct TestApp {
    pub _dir: tempfile::TempDir,
    pub state: AppState,
    pub router: Router,
}

// Construit une application neuve (base vide, migrations appliquées).
pub async fn app() -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path().join("data"));
    paths.ensure().unwrap();
    let pool = db::open(&paths.db_file).await.unwrap();
    let state = AppState { pool, paths, secure_cookies: false, login_guard: LoginGuard::default(), shared_storage: false, lan: false, port: 8080 };
    let router = build_router(state.clone());
    TestApp { _dir: dir, state, router }
}

impl TestApp {
    pub async fn call(&self, method: &str, uri: &str, cookie: Option<&str>, body: Option<Value>) -> (StatusCode, Value, Option<String>) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        let body = match body {
            Some(b) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let res = self.router.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = res.status();
        let set_cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, json, set_cookie)
    }

    pub async fn upload(&self, uri: &str, cookie: &str, file_name: &str, content: &[u8]) -> (StatusCode, Value) {
        let boundary = "----erptestboundary";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body))
            .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    pub async fn setup_admin(&self) -> String {
        let (s, _, c) = self
            .call("POST", "/api/auth/setup", None, Some(json!({
                "email": "admin@demo.fr", "display_name": "Admin Démo", "password": "motdepasse-solide"
            })))
            .await;
        assert_eq!(s, StatusCode::OK);
        c.expect("cookie de session")
    }

    pub async fn create_user(&self, admin: &str, email: &str, role: &str) -> (i64, String) {
        let (s, body, _) = self
            .call("POST", "/api/users", Some(admin), Some(json!({
                "email": email, "display_name": "Utilisateur", "role": role, "password": "motdepasse-solide"
            })))
            .await;
        assert_eq!(s, StatusCode::OK, "{body}");
        let (s, _, c) = self
            .call("POST", "/api/auth/login", None, Some(json!({ "email": email, "password": "motdepasse-solide" })))
            .await;
        assert_eq!(s, StatusCode::OK);
        (body["id"].as_i64().unwrap(), c.unwrap())
    }
}

