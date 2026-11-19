//! Rôles et permissions. Toute vérification se fait côté serveur.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
// Les trois rôles, stockés en base sous leur forme SCREAMING_SNAKE_CASE ("ADMIN"...).
pub enum Role {
    Admin,
    Gestionnaire,
    Utilisateur,
}

/// Permissions applicatives. Les lots suivants en ajouteront.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    // Créer / modifier les comptes utilisateurs.
    ManageUsers,
    // Identité de l'entreprise, stockage, réseau.
    ManageSettings,
    // Consulter le journal d'audit.
    ViewAudit,
    // Lancer et télécharger une sauvegarde.
    RunBackup,
    // Logiciels, centres de formation.
    ManageReferenceLists,
    ManageBusinessData, // contacts, projets, missions, facturation
    // Saisir ses propres temps.
    EnterOwnTime,
}

impl Role {
    // Forme stockée en base.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "ADMIN",
            Role::Gestionnaire => "GESTIONNAIRE",
            Role::Utilisateur => "UTILISATEUR",
        }
    }

    // Lecture depuis la base ; None si la valeur est inconnue.
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "ADMIN" => Some(Role::Admin),
            "GESTIONNAIRE" => Some(Role::Gestionnaire),
            "UTILISATEUR" => Some(Role::Utilisateur),
            _ => None,
        }
    }

    // Matrice des droits : ADMIN tout, GESTIONNAIRE le métier, UTILISATEUR ses temps uniquement.
    pub fn can(self, p: Permission) -> bool {
        use Permission::*;
        match self {
            Role::Admin => true,
            Role::Gestionnaire => matches!(
                p,
                RunBackup | ManageReferenceLists | ManageBusinessData | EnterOwnTime
            ),
            Role::Utilisateur => matches!(p, EnterOwnTime),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrice_des_droits() {
        assert!(Role::Admin.can(Permission::ManageUsers));
        assert!(!Role::Gestionnaire.can(Permission::ManageUsers));
        assert!(!Role::Gestionnaire.can(Permission::ManageSettings));
        assert!(Role::Gestionnaire.can(Permission::ManageBusinessData));
        assert!(!Role::Utilisateur.can(Permission::ManageBusinessData));
        assert!(Role::Utilisateur.can(Permission::EnterOwnTime));
        assert!(!Role::Utilisateur.can(Permission::RunBackup));
    }
}
