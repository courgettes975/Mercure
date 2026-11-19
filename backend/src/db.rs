//! Ouverture de la base SQLite et application des migrations.
//!
//! Les migrations SQL (dossier `migrations/`) sont embarquées dans l'exécutable à la compilation
//! et rejouées au démarrage : une base ancienne est mise à niveau automatiquement.

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::{path::Path, str::FromStr, time::Duration};

/// Migrations embarquées (sqlx vérifie leur empreinte : un fichier déjà appliqué ne doit pas changer).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Ouverture standard (poste unique, journal WAL).
pub async fn open(db_path: &Path) -> anyhow_like::Result<SqlitePool> {
    open_with(db_path, false).await
}

/// `shared` : dossier réseau ou synchronisé -> journal classique (WAL n'est pas sûr sur un partage)
/// et attente plus longue en cas de verrou posé par un autre poste.
pub async fn open_with(db_path: &Path, shared: bool) -> anyhow_like::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite://")?
        .filename(db_path)
        .create_if_missing(true) // première utilisation : le fichier est créé
        .foreign_keys(true) // les clés étrangères sont vérifiées (intégrité des liens)
        .journal_mode(if shared { SqliteJournalMode::Delete } else { SqliteJournalMode::Wal })
        .synchronous(SqliteSynchronous::Full) // écriture durable à chaque validation
        .busy_timeout(Duration::from_secs(if shared { 30 } else { 10 })); // attente si la base est verrouillée
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    // Mise à niveau du schéma avant de servir la première requête.
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// Petit alias d'erreur pour le démarrage (évite une dépendance supplémentaire).
pub mod anyhow_like {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}
