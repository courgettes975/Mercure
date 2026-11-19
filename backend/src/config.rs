//! Configuration locale : fichier `mercure.config.json` à côté de l'exécutable
//! (l'ancien nom `erp.config.json` est encore lu s'il est seul présent).
//! Permet de pointer vers un dossier de données partagé (disque réseau, NAS, clé USB),
//! de fixer le port et d'activer l'accès depuis les autres PC du réseau.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Nom du fichier de configuration.
pub const FILE_NAME: &str = "mercure.config.json";
/// Ancien nom du fichier (versions précédentes), lu en repli.
pub const LEGACY_FILE_NAME: &str = "erp.config.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)] // chaque champ absent prend sa valeur par défaut : le fichier peut être partiel
pub struct Config {
    /// Dossier des données (base, documents, sauvegardes). Vide = dossier `data` à côté de l'exécutable.
    pub data_dir: Option<String>,
    /// Dossier partagé (réseau, synchronisé) : journal SQLite classique au lieu du mode WAL,
    /// qui n'est pas fiable sur un partage réseau.
    pub shared: bool,
    /// Port HTTP (8080 si absent).
    pub port: Option<u16>,
    /// Accès réseau (équivalent de --reseau).
    pub lan: Option<bool>,
}

/// Dossier contenant l'exécutable (repli : dossier courant).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Chemin complet du fichier de configuration.
pub fn config_path() -> PathBuf {
    exe_dir().join(FILE_NAME)
}

/// Lecture tolérante : fichier absent ou illisible = configuration par défaut.
pub fn load() -> Config {
    std::fs::read_to_string(config_path())
        .or_else(|_| std::fs::read_to_string(exe_dir().join(LEGACY_FILE_NAME)))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Écriture lisible (JSON indenté) pour permettre une modification à la main.
pub fn save(c: &Config) -> std::io::Result<()> {
    std::fs::write(config_path(), serde_json::to_string_pretty(c).unwrap_or_else(|_| "{}".into()))
}

/// Dossier de données par défaut : `data` à côté de l'exécutable.
pub fn default_data_dir() -> PathBuf {
    exe_dir().join("data")
}

/// Dossier de données effectif : option --data, sinon configuration, sinon dossier par défaut.
pub fn resolve_data_dir(cli: Option<PathBuf>, c: &Config) -> PathBuf {
    cli.or_else(|| c.data_dir.as_deref().filter(|s| !s.trim().is_empty()).map(PathBuf::from))
        .unwrap_or_else(default_data_dir)
}

/// Chemin UNC (\\serveur\partage) ou dossier synchronisé connu : on suppose un partage.
pub fn looks_shared(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.starts_with("\\\\") || s.starts_with("//") || s.to_lowercase().contains("onedrive") || s.to_lowercase().contains("dropbox")
}
