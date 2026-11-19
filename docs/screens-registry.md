# Registre des écrans

Mis à jour : 2026-11-18 (fin des lots 1 à 9)

Statuts : à faire / en cours / terminé (au sens de la Definition of Done)

## Écrans issus des captures Odoo

| ID | Module | Nom | Capture | Fonctionnalités | Fiche | Statut |
|---|---|---|---|---|---|---|
| TIM-001 | Feuilles de temps | Grille "Mes feuilles de temps" | captures/TIM-001_grille-temps.png | grille projet/tâche x jours, navigation période, totaux jour/ligne, heures travaillées/facturables, ajout de ligne | screens/TIM-001.md | terminé (lot 4) |
| CNT-001 | Contacts | Liste des contacts | captures/CNT-001_liste-contacts.png | recherche, liste paginée (80/page), colonnes nom/email/tél/activités/pays, sélection multiple, création | screens/CNT-001.md | terminé (lot 2) |
| CNT-002 | Contacts | Fiche contact | captures/CNT-002_fiche-contact.png | identité, adresse, identifiants légaux, statut stagiaire, onglets dont "formation", compteurs liés, historique | screens/CNT-002.md | terminé (lot 2) pour la partie contact ; onglet formation -> lot 3b |
| INV-001 | Facturation | Liste des factures clients | captures/INV-001_liste-factures.png | liste, filtres, échéance relative, HT/TTC/montant dû, statuts Payé/Comptabilisé | screens/INV-001.md | terminé (lot 5) |

## Écrans issus du cahier des charges (sans capture à ce jour)

| ID | Module | Nom | Source | Statut |
|---|---|---|---|---|
| AUT-001 | Sécurité | Première utilisation (création admin) + connexion | spec section 13 | terminé (lot 1) |
| SET-001 | Paramètres | Entreprise (identité dont NDA, logo, banque, TVA, mentions, numérotation) | spec section 12 | terminé (lot 1) |
| SET-002 | Paramètres | Utilisateurs et rôles | spec section 13 | terminé (lot 1) |
| SET-003 | Paramètres | Logiciels enseignés (liste de référence) | réponse du 2026-11-18 | terminé (lot 1) |
| SET-004 | Paramètres | Sauvegardes (manuelle, auto quotidienne, téléchargement) | contrainte "base sauvegardable" | terminé (lot 1) |
| SET-005 | Paramètres | Journal des opérations | spec section 14 | terminé (lot 1) |
| SET-006 | Paramètres | Mon compte (changement de mot de passe) | spec section 13 | terminé (lot 1) |
| DSH-001 | Tableau de bord | Tableau de bord général | spec section 19 | terminé (lot 8) |
| PRJ-001 | Projets | Liste des projets | spec section 5 | terminé (lot 3) |
| PRJ-002 | Projets | Fiche projet | spec section 5 | terminé (lot 3) |
| MF-001 | Missions de formation | Tableau de bord missions | spec module 4 section 8 | terminé (lot 3b) |
| MF-002 | Missions de formation | Liste / Kanban / Calendrier | spec module 4 section 8 | terminé (lot 3b) |
| MF-003 | Missions de formation | Fiche mission (sessions, participants, frais, documents, pédagogie) | spec module 4 sections 1-7 | terminé (lot 3b) |
| TIM-002 | Feuilles de temps | Liste des lignes de temps (filtres, regroupements) | spec section 6 | terminé (lot 4) |
| TIM-003 | Feuilles de temps | Validation ("À valider", vu dans le menu TIM-001) | capture TIM-001 (menu seul) | terminé (lot 4) : validation par sélection dans la vue liste |
| INV-002 | Facturation | Fiche facture (lignes, validation, paiements, avoir) | spec section 8 | terminé (lot 5) |
| INV-003 | Facturation | Facturer depuis les temps / la mission | spec section 7 et module 4 section 5 | terminé (lot 6) |
| INV-004 | Facturation | PDF facture | spec section 11 | terminé (lot 7) |
| CNT-003 | Contacts | Import depuis Odoo (aperçu puis import) | fichier Contact_res.partner.xlsx | terminé |
| SET-007 | Paramètres | Données de démonstration | spec section 24 | terminé (lot 9) |
