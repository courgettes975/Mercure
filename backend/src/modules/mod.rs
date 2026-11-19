//! Modules fonctionnels. Chacun expose ses handlers HTTP et, pour les plus gros, un `service`
//! (logique + SQL) et un `model` (structures échangées avec l'interface).

pub mod audit; // journal des actions (append-only)
pub mod backup; // sauvegardes de la base
pub mod contacts; // sociétés, organismes, particuliers, import Odoo
pub mod dashboard; // tableau de bord général
pub mod demo;
pub mod expenses; // dépenses de l'entreprise (achats, abonnements, justificatifs) // données de démonstration
pub mod invoices; // factures, avoirs, paiements, PDF, facturation mensuelle
pub mod missions; // missions de formation : sessions, participants, frais, documents
pub mod products; // liste des produits / prestations (pré-remplissage des factures)
pub mod projects; // projets et tâches (une mission est un projet FORMATION)
pub mod sequence; // numérotation transactionnelle (FAC-, AV-, MF-, PRJ-, CLI-)
pub mod settings; // identité de l'entreprise, mentions, logo
pub mod software; // liste des logiciels enseignés
pub mod stats; // statistiques par critères et exports CSV
pub mod static_files; // interface React embarquée
pub mod storage; // emplacement des données, accès réseau
pub mod timesheets; // feuilles de temps
pub mod training_centers; // liste des centres de formation
pub mod users; // gestion des comptes
