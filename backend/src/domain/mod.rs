//! Règles métier pures, sans accès à la base ni au réseau : testables isolément.

pub mod money; // montants en centimes, arrondis, TVA, format français
pub mod roles; // rôles ADMIN / GESTIONNAIRE / UTILISATEUR et permissions
pub mod validation; // email, SIREN/SIRET, IBAN, dates, clé de recherche
