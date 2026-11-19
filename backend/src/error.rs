//! Erreurs applicatives : messages compréhensibles pour l'utilisateur,
//! détails techniques uniquement dans les journaux.
//!
//! Chaque variante correspond à un code HTTP ; le corps de réponse est toujours `{ "error": "..." }`.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Donnée saisie invalide (400) : le message est destiné à l'utilisateur.
    #[error("{0}")]
    Validation(String),
    /// Pas de session valide (401).
    #[error("Authentification requise")]
    Unauthorized,
    /// Email ou mot de passe incorrect (401, message volontairement vague).
    #[error("Identifiants incorrects")]
    BadCredentials,
    /// Rôle insuffisant (403).
    #[error("Action non autorisée pour votre rôle")]
    Forbidden,
    /// Élément inexistant (404).
    #[error("{0}")]
    NotFound(String),
    /// Conflit avec l'état actuel : doublon, élément lié, numéro déjà pris (409).
    #[error("{0}")]
    Conflict(String),
    /// Erreur technique (500) : le détail est journalisé, pas renvoyé.
    #[error("Erreur interne")]
    Internal(String),
}

/// Raccourci utilisé par tous les services.
pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    /// Convertit n'importe quelle erreur affichable en erreur interne.
    pub fn internal(e: impl std::fmt::Display) -> Self {
        AppError::Internal(e.to_string())
    }
}

/// Traduction des erreurs SQLite : les contraintes du schéma deviennent des messages métier.
impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e {
            let msg = db.message();
            if msg.contains("UNIQUE constraint failed") {
                return AppError::Conflict("Cette valeur existe déjà".into());
            }
            if msg.contains("FOREIGN KEY constraint failed") {
                return AppError::Conflict(
                    "Opération impossible : des éléments liés existent".into(),
                );
            }
            if msg.contains("CHECK constraint failed") {
                return AppError::Validation("Valeur non autorisée".into());
            }
        }
        AppError::Internal(e.to_string())
    }
}

/// Conversion en réponse HTTP (code + JSON).
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Validation(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized | AppError::BadCredentials => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Internal(detail) => {
                // Le détail technique ne sort jamais vers le navigateur.
                tracing::error!("erreur interne : {detail}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}
