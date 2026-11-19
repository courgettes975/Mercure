//! ERP formation : socle applicatif (bibliothèque utilisée par l'exécutable et par les tests).
//!
//! Couches : http (routes, ce fichier) -> modules (services métier) -> base SQLite.
//! Chaque module expose des "handlers" Axum (fonctions async recevant l'état et l'utilisateur
//! connecté) ; la logique métier et les requêtes SQL vivent dans les services des modules.
#![allow(clippy::type_complexity)] // tuples de lecture SQL explicites, volontairement locaux

pub mod auth; // sessions, connexion, rôles de l'utilisateur courant
pub mod config; // fichier erp.config.json
pub mod db; // ouverture SQLite + migrations
pub mod domain; // règles métier pures : argent, rôles, validation
pub mod error; // AppError -> réponse HTTP
pub mod modules; // un sous-module par domaine fonctionnel
pub mod state; // AppState partagé par les requêtes

use axum::{
    extract::{DefaultBodyLimit, Request},
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, patch, post, put},
    Router,
};
use modules::{audit, backup, contacts, dashboard, demo, expenses, invoices, missions, products, projects, settings, software, static_files, stats, storage, timesheets, training_centers, users};
use state::AppState;

/// Middleware : en-têtes de sécurité ajoutés à toutes les réponses (API et fichiers statiques).
/// - nosniff : le navigateur respecte le type MIME annoncé ;
/// - DENY / frame-ancestors : l'ERP ne peut pas être affiché dans une iframe (clickjacking) ;
/// - CSP : scripts et styles uniquement depuis l'ERP lui-même (styles inline tolérés pour React).
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; frame-ancestors 'none'",
        ),
    );
    res
}

/// Construit le routeur complet : toutes les routes `/api/...` puis l'interface embarquée en repli.
/// Les droits ne sont PAS vérifiés ici mais dans chaque handler (`CurrentUser::require`).
pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        // --- Authentification et compte ---
        .route("/auth/status", get(auth::status)) // Mercure a-t-il déjà un administrateur ?
        .route("/auth/setup", post(auth::setup)) // création du premier compte
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", post(auth::change_password))
        // --- Utilisateurs (ADMIN) ---
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}", patch(users::update))
        // --- Paramètres : entreprise, logo, stockage des données, accès réseau ---
        .route("/settings/company", get(settings::get).put(settings::update))
        .route("/settings/company/logo", get(settings::get_logo).post(settings::upload_logo))
        .route("/settings/storage", get(storage::get).put(storage::update))
        .route("/settings/network", get(storage::get_network).put(storage::update_network))
        // --- Listes de référence : logiciels enseignés, centres de formation ---
        .route("/software", get(software::list).post(software::create))
        .route("/software/{id}", patch(software::update))
        .route("/products", get(products::list).post(products::create))
        .route("/products/{id}", patch(products::update))
        .route("/training-centers", get(training_centers::list).post(training_centers::create))
        .route("/training-centers/{id}", delete(training_centers::remove))
        // --- Sauvegardes, journal d'audit, tableau de bord, données de démonstration ---
        .route("/backups", get(backup::list).post(backup::create))
        .route("/backups/{name}", get(backup::download))
        .route("/audit", get(audit::list))
        .route("/dashboard", get(dashboard::get))
        .route("/demo", get(demo::status).post(demo::load).delete(demo::remove))
        // --- Contacts (sociétés, organismes, particuliers) ---
        .route("/contacts", get(contacts::list).post(contacts::create))
        .route("/contacts/export", get(contacts::export)) // CSV
        .route("/contacts/import/preview", post(contacts::import_preview)) // import Odoo : aperçu
        .route("/contacts/import", post(contacts::import_execute)) // import Odoo : exécution
        .route("/contacts/{id}", get(contacts::get).put(contacts::update).delete(contacts::delete))
        .route("/contacts/{id}/archive", post(contacts::archive))
        .route("/contacts/{id}/unarchive", post(contacts::unarchive))
        .route("/contacts/{id}/duplicate", post(contacts::duplicate))
        .route("/contacts/{id}/activity", get(contacts::activity)) // missions, factures, notes du contact
        .route("/contacts/{id}/notes", post(contacts::add_note))
        .route("/tags", get(contacts::list_tags).post(contacts::create_tag))
        // --- Projets et tâches (une mission de formation est un projet de type FORMATION) ---
        .route("/people", get(projects::list_people)) // utilisateurs affectables
        .route("/projects", get(projects::list_handler).post(projects::create_handler))
        .route("/projects/{id}", get(projects::get_handler).put(projects::update_handler).delete(projects::delete_handler))
        .route("/projects/{id}/archive", post(projects::archive_handler))
        .route("/projects/{id}/tasks", post(projects::create_task))
        .route("/tasks/{id}", patch(projects::update_task))
        // --- Missions de formation : fiche, sessions, participants, frais, documents ---
        .route("/missions", get(missions::list).post(missions::create))
        .route("/missions/dashboard", get(missions::dashboard))
        .route("/missions/calendar", get(missions::calendar))
        .route("/missions/{id}", get(missions::get).put(missions::update).delete(missions::delete))
        .route("/missions/{id}/status", post(missions::set_status))
        .route("/missions/{id}/sessions", post(missions::add_session))
        .route("/missions/{id}/participants", post(missions::add_participant))
        .route("/missions/{id}/expenses", post(missions::add_expense))
        .route(
            "/missions/{id}/documents",
            // Les pièces jointes peuvent être volumineuses : limite portée à 25 Mo sur cette route.
            post(missions::upload_document).layer(DefaultBodyLimit::max(25 * 1024 * 1024)),
        )
        .route("/sessions/{id}", put(missions::update_session).delete(missions::delete_session))
        .route("/sessions/{id}/realize", post(missions::realize_session)) // session réalisée -> temps créé
        .route("/sessions/{id}/status", post(missions::session_status))
        .route("/participants/{id}", put(missions::update_participant).delete(missions::remove_participant))
        .route("/expenses/{id}", put(missions::update_expense).delete(missions::delete_expense))
        // --- Feuilles de temps ---
        .route("/timesheets", get(timesheets::list).post(timesheets::create))
        .route("/timesheets/choices", get(timesheets::choices)) // projets/tâches saisissables
        .route("/timesheets/cell", put(timesheets::set_cell)) // grille hebdomadaire
        .route("/timesheets/validate", post(timesheets::validate))
        .route("/timesheets/{id}", put(timesheets::update).delete(timesheets::delete))
        .route("/timesheets/{id}/duplicate", post(timesheets::duplicate))
        // --- Facturation : factures, avoirs, paiements, PDF ---
        .route("/invoices", get(invoices::list).post(invoices::create))
        .route("/invoices/overview", get(invoices::overview)) // état des lieux
        .route("/invoices/{id}", get(invoices::get).put(invoices::update).delete(invoices::delete))
        .route("/invoices/{id}/validate", post(invoices::validate)) // attribue le numéro définitif
        .route("/invoices/{id}/cancel", post(invoices::cancel)) // annulation par avoir
        .route("/invoices/{id}/payments", post(invoices::add_payment))
        .route("/invoices/{id}/reminder", post(invoices::reminder))
        .route("/invoices/{id}/sent", post(invoices::mark_sent)) // marquée envoyée au client
        .route("/invoices/{id}/pdf", get(invoices::pdf)) // PDF, hybride Factur-X si la facturation électronique est active
        .route("/invoices/{id}/xml", get(invoices::xml)) // XML CII ou UBL
        .route("/invoices/{id}/einvoice", post(invoices::einvoice_status)) // suivi de transmission
        .route("/payments/{id}", axum::routing::delete(invoices::delete_payment))
        // --- Facturation depuis les temps et frais ---
        .route("/billing/projects/{id}", get(invoices::billable)) // éléments en attente d'un projet
        .route("/billing/from-timesheets", post(invoices::from_items)) // facture d'un projet
        .route("/billing/monthly", get(invoices::monthly_report).post(invoices::monthly_invoice)) // facture mensuelle par centre
        // --- Dépenses de l'entreprise ---
        .route("/depenses", get(expenses::list).post(expenses::create))
        .route("/depenses/export.csv", get(expenses::export))
        .route("/depenses/{id}", get(expenses::get).put(expenses::update).delete(expenses::delete))
        .route("/depenses/{id}/pay", post(expenses::pay))
        .route("/depenses/{id}/receipt", get(expenses::download_receipt).post(expenses::upload_receipt).layer(DefaultBodyLimit::max(12 * 1024 * 1024)))
        // --- Statistiques et exports ---
        .route("/stats", get(stats::get))
        .route("/stats/export.csv", get(stats::export))
        // --- Documents joints aux missions ---
        .route("/documents/{id}", get(missions::download_document).delete(missions::delete_document));

    Router::new()
        .nest("/api", api)
        // Tout ce qui n'est pas /api est l'interface React embarquée dans l'exécutable.
        .fallback(static_files::serve)
        // Limite générale des corps de requête (les documents ont leur propre limite ci-dessus).
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
