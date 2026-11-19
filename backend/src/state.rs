//! État partagé entre toutes les requêtes HTTP (cloné à chaque requête : ne contient que des
//! poignées légères : pool de connexions, chemins, compteurs protégés).

use sqlx::SqlitePool;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppState {
    /// Pool de connexions SQLite.
    pub pool: SqlitePool,
    /// Emplacements des fichiers (base, documents, sauvegardes).
    pub paths: DataPaths,
    /// Cookie "Secure" seulement si l'application est servie en HTTPS.
    pub secure_cookies: bool,
    /// Protection contre les essais de mot de passe répétés.
    pub login_guard: crate::auth::LoginGuard,
    /// Dossier partagé (journal SQLite classique).
    pub shared_storage: bool,
    /// Accès réseau actif (écoute sur toutes les interfaces).
    pub lan: bool,
    /// Port d'écoute effectif.
    pub port: u16,
}

/// Arborescence du dossier de données.
#[derive(Clone, Debug)]
pub struct DataPaths {
    /// Dossier racine des données.
    pub root: PathBuf,
    /// Fichier de base de données `erp.db`.
    pub db_file: PathBuf,
    /// Sous-dossier des sauvegardes.
    pub backups: PathBuf,
    /// Sous-dossier des documents joints aux missions.
    pub documents: PathBuf,
}

impl DataPaths {
    /// Dérive tous les chemins à partir du dossier racine.
    pub fn new(root: PathBuf) -> Self {
        Self {
            db_file: root.join("erp.db"),
            backups: root.join("backups"),
            documents: root.join("documents"),
            root,
        }
    }

    /// Crée les dossiers manquants (idempotent).
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.backups)?;
        std::fs::create_dir_all(&self.documents)?;
        Ok(())
    }
}
