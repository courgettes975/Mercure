# Plan d'implémentation

Mis à jour : 2026-11-18

## État du projet existant
Dossier D:\Claude\ERP vide au 2026-11-18 : aucun code, aucune base, aucune stack imposée. Projet à créer de zéro.

## Contraintes exprimées (2026-11-18)
Application locale, sans Docker, base de données sauvegardable, backend en Rust (ou C++).

## Stack retenue
| Couche | Choix | Raison |
|---|---|---|
| Backend/API | Rust : Axum (HTTP REST) + Tokio | un seul exécutable, rapide, sans dépendance installée |
| Accès données | SQLx + migrations SQL versionnées (embarquées dans l'exécutable) | requêtes vérifiées, migrations auto au démarrage |
| Base | SQLite (mode WAL, clés étrangères activées) | un seul fichier, sauvegarde par simple copie |
| Montants | entiers en centimes (i64) + type Rust Money ; taux en centimes ; TVA en points de base | SQLite n'a pas de DECIMAL exact : on n'utilise jamais de flottant |
| Séquences | table sequence + transaction BEGIN IMMEDIATE | SQLite sérialise les écritures : pas de doublon |
| Auth | sessions cookie httpOnly + argon2 | rôles contrôlés côté serveur |
| Frontend | React + TypeScript + Vite, embarqué dans l'exécutable | interface ERP moderne dans le navigateur |
| PDF | génération côté Rust (Typst ou printpdf, à trancher au lot 7) | pas de navigateur requis |
| Tests | cargo test (unitaires + API sur base SQLite temporaire), Vitest côté interface | |
| Fichiers | dossier data\documents à côté de la base | documents de mission |

Fonctionnement : double-clic sur erp.exe -> le serveur démarre sur http://localhost:8080 et ouvre le navigateur.
Accès depuis un autre poste du réseau possible plus tard (option d'écoute réseau).

Sauvegarde : bouton "Sauvegarder" (copie cohérente via VACUUM INTO) + sauvegarde automatique quotidienne horodatée
dans data\backups, avec rotation. Restauration = remplacer le fichier .db.

Prérequis de développement sur le PC : Rust (rustup + outils MSVC) et Node.js (uniquement pour compiler l'interface).
L'utilisateur final n'a besoin que de l'exécutable.

## Architecture
```
backend/                  Rust (cargo)
  src/main.rs             démarrage, config, ouverture navigateur
  src/http/               routes Axum, extraction, erreurs lisibles
  src/modules/<module>/   handlers -> service (règles métier) -> repository (SQLx)
  src/domain/             types Money, Duration, statuts, transitions
  migrations/             fichiers SQL versionnés
frontend/                 React + TypeScript
  src/components/         bibliothèque commune (table, formulaire, badge, modale, recherche, pagination)
  src/modules/<module>/   écrans (aucune règle métier)
data/                     base erp.db, documents, backups (hors dépôt)
docs/                     registre, modèle, fiches écrans, captures
```

## Décisions confirmées (2026-11-18)
- Hébergement : PC Windows local.
- TVA : aucune TVA appliquée aujourd'hui. Le régime reste paramétrable (bascule possible vers assujetti) ;
  taux par ligne prévu dans le modèle, par défaut 0 avec mention légale saisie dans Paramètres.
- Numérotation : nouvelle séquence propre au nouvel ERP, format FAC-AAAA-NNNNNN, générée côté serveur.
  Odoo garde ses factures INV/... ; la bascule se fait à une date nette (à fixer, idéalement 01/01/2027).

## Lots
| Lot | Contenu | Écrans |
|---|---|---|
| 1 | Socle : projet Rust + interface, SQLite, migrations, sauvegarde, auth, rôles, AuditLog, Sequence, CompanySettings, composants UI de base (table, formulaire, badge, modale, recherche, pagination) | AUT-001, SET-001, SET-002 |
| 2 | Contacts : CRUD, société/interlocuteurs, tags, recherche, filtres, tri, pagination, archivage, suppression contrôlée, historique | CNT-001, CNT-002 |
| 3 | Projets et tâches | PRJ-001, PRJ-002 |
| 3b | Missions de formation : fiche, statuts, sessions, participants, frais, documents, pédagogie | MF-001, MF-002, MF-003 |
| 4 | Feuilles de temps : grille, liste, regroupements, totaux, génération depuis sessions, validation | TIM-001, TIM-002, TIM-003 |
| 5 | Facturation : brouillon, lignes, calculs serveur, validation + numérotation, paiements, annulation, avoir | INV-001, INV-002 |
| 6 | Temps et frais -> facture (anti double facturation) | INV-003 |
| 7 | PDF facture + mentions paramétrables | INV-004 |
| 8 | Tableaux de bord (général + missions) | DSH-001, MF-001 |
| 9 | Tests complémentaires, sécurité, finition, données de démo supprimables | tous |
| option | Reprise des données Odoo (export CSV) | |

Après chaque lot : compilation, tests, migrations, non-régression, mise à jour docs, corrections.

## Tests critiques (dès les lots concernés)
calcul HT/TVA/TTC et arrondis, numérotation concurrente sans doublon ni trou, transitions de statut facture,
blocage d'une facture validée, double facturation impossible, génération idempotente des temps depuis une session,
authentification, permissions par rôle sur chaque route.

## Décisions en attente
1. Date de bascule Odoo -> nouvel ERP.
2. Reprise des données Odoo.

## Avancement

### Lot 1 - terminé le 2026-11-18
Livré :
- Exécutable unique `app\erp.exe` (Windows, sans dépendance), interface embarquée, ouverture du navigateur.
- SQLite (WAL, clés étrangères), migration `0001_socle.sql` appliquée au démarrage.
- Authentification : création du premier admin, connexion, déconnexion, sessions cookie httpOnly 12 h,
  mots de passe argon2 (10 caractères min.), blocage 15 min après 5 échecs.
- Rôles ADMIN / GESTIONNAIRE / UTILISATEUR contrôlés côté serveur sur chaque route.
- Utilisateurs : création, modification, désactivation (sessions coupées), dernier admin protégé.
- Paramètres entreprise : contrôles SIREN/SIRET (clé), IBAN (mod 97), BIC, TVA (franchise impose 0 %), logo PNG/JPEG.
- Logiciels enseignés (AutoCAD, Revit, Fusion, SolidWorks + ajout/archivage).
- Journal non modifiable (triggers SQL), différences ancienne/nouvelle valeur.
- Séquences de numérotation transactionnelles (testées en concurrence : 40 appels, aucun doublon ni trou).
- Sauvegardes : manuelle, automatique quotidienne (30 conservées), téléchargement.
- Domaine : type Money en centimes (temps x taux, TVA, arrondi commercial, format français).
- Composants UI communs : bouton, champs, sélecteur, case, badge, alerte, modale, confirmation, recherche,
  pagination, tableau triable, notifications.

Vérifications : `cargo test` 32 tests OK, `cargo clippy` 0 avertissement, `npm test` 3 tests OK,
compilation TypeScript OK, parcours complet testé dans un navigateur (création admin, paramètres avec erreur
puis succès, création utilisateur, sauvegarde, journal).
Non vérifié : lancement de `erp.exe` sur Windows réel (compilé par compilation croisée).

Hypothèses du lot :
- Session de 12 h, sans prolongation automatique.
- Le GESTIONNAIRE peut lancer une sauvegarde et gérer la liste des logiciels, pas les paramètres entreprise.
- Les mentions légales ne sont pas pré-remplies : à saisir et faire valider par le comptable.

### Évolution 2026-11-18 : NDA
Ajout du NDA (numéro de déclaration d'activité) dans Paramètres > Entreprise. Migration `0002_nda.sql`,
contrôle 11 chiffres (espaces acceptés à la saisie). Migration vérifiée sur une base existante : données conservées.

### Lot 2 - Contacts - terminé le 2026-11-18
Livré : migration `0003_contacts.sql` (partners, tags, partner_tags, partner_notes), API /api/contacts
(liste filtrée et paginée, export CSV, fiche, création, modification, archivage, suppression contrôlée,
duplication, notes, activité) et /api/tags ; écrans liste et fiche.
Composants ajoutés : sélecteur avec recherche (AsyncSelect), étiquettes (TagPicker), tri serveur dans DataTable,
filtres dans l'adresse (useUrlState).
Vérifications : `cargo test` 50 tests OK (dont 11 contacts : rattachement, numérotation client, recherche
accents, suppression contrôlée, historique, droits, pagination, tri non injectable), `cargo clippy` 0 avertissement,
`npm test` OK, parcours navigateur (création société + interlocuteur, erreur SIRET, étiquette, note, recherche).
Prochain : lot 3 (Projets) puis 3b (Missions de formation), ou import des contacts Odoo (CSV) si souhaité.

### Import des contacts Odoo - terminé le 2026-11-18
Paramètres : Contacts > Importer (aperçu, puis import). Voir docs/screens/CNT-IMPORT.md.

### Lots 3 à 9 - terminés le 2026-11-18
- Lot 3 Projets : liste, fiche, tâches, intervenants, archivage, suppression contrôlée.
- Lot 3b Missions : tableau de bord (7 compteurs + CA/heures/stagiaires/reste à facturer), vues liste / kanban
  (glisser-déposer des statuts) / calendrier, fiche à 7 onglets (informations, sessions avec présences, participants,
  frais, documents, suivi pédagogique, facturation), bouton "Générer la facture".
- Lot 4 Temps : grille semaine/mois (saisie directe 7, 7h30, 45min), liste avec regroupements et totaux,
  duplication, validation par un gestionnaire, temps des sessions verrouillés.
- Lot 5-6 Facturation : liste type Odoo (échéance relative, montant dû, statuts), fiche brouillon éditable,
  validation, paiements, relance, avoir, page "À facturer", génération depuis temps/frais/forfait.
- Lot 7 PDF : générateur maison (Helvetica WinAnsi, logo PNG/JPEG, multi-pages, mentions paramétrées).
- Lot 8 Tableau de bord : indicateurs réels + CA HT sur 12 mois.
- Lot 9 : données de démonstration (chargement / suppression), journal étendu, clippy 0 avertissement.

Vérifications : `cargo test` 77 tests, `npm test` 6 tests, parcours navigateur complet (import Odoo, mission,
sessions, participant, frais, facture, validation, paiement, temps, kanban, calendrier, tableau de bord), PDF
contrôlé avec qpdf et rendu visuel. Migration testée sur une base existante (lot 1-2) : données conservées.

Hypothèses prises (modifiables) :
- 1 jour de formation = 7 h pour le tarif journalier.
- Un avoir est toujours total (pas d'avoir partiel).
- Les avoirs ont leur propre série AV-AAAA-NNNNNN.
- Une facture ne peut pas être datée avant la dernière facture validée.
- Les frais sont refacturés au prix saisi, sans majoration.
- Les temps hors mission se saisissent par projet ; un projet "FORMATION" se gère depuis sa mission.

Reste à faire / idées : avoir partiel, export comptable, envoi de facture par email, convention et attestation en PDF,
import des factures Odoo (historique), accès réseau multi-postes (option --reseau existante, à sécuriser en HTTPS).

### Évolution 2026-11-18 : mise en page du PDF sur le modèle Odoo (INV/2026/00048)
Logo + adresse émetteur en haut à gauche, client en colonne droite, titre gris "Facture FAC-...", dates de
facturation / échéance sur deux colonnes, tableau sans fond (Description, Quantité avec unité, Prix unitaire, Montant),
total à droite sous un filet, conditions de règlement à gauche, communication de paiement = numéro de facture,
IBAN groupé par 4 + banque, pied de page centré : téléphone email site / SIRET / NDA en gras / numéro / page.

### Évolution 2026-11-18 : emplacement des données configurable (multi-PC)
Fichier `erp.config.json` à côté de l'exe (data_dir, shared, port, lan). Page Paramètres > Stockage : changement
de dossier avec test d'écriture, copie assistée (VACUUM INTO + documents + sauvegardes), détection des chemins
UNC / OneDrive / Dropbox. Mode partagé = journal SQLite DELETE (WAL non fiable sur partage réseau) et attente
de verrou de 30 s. Testé : copie vers un second dossier, redémarrage, données retrouvées, journal_mode=delete.
Limite assumée : un seul poste à la fois ; alternative documentée `--reseau`.

### Évolution 2026-11-18 : option d'accès réseau dans les paramètres
Routes `GET/PUT /api/settings/network` (ADMIN) : lecture/écriture de `lan` et `port` dans `erp.config.json`,
affichage du nom du PC et des IP locales, `restart_required` si différent de l'état actif. Port limité à 1024-65535.
Page Paramètres > Stockage et réseau : option 1 dossier partagé, option 2 accès réseau (case + port + adresses à
communiquer, avertissements pare-feu / réseau de confiance / HTTP non chiffré). Journalisé dans l'audit (`network`).
Testé : activation, redémarrage, réponse depuis l'IP du poste, refus du port 80.

### Évolution 2026-11-18 : fiche mission, centre de formation
Migration 0007 : `missions.center_id` (contact ENTREPRISE/ORGANISME : Senza, One Learn, BT Formation...),
`invoice_sent` (case "Facture envoyée"), `annotations` (zone libre). Tarif horaire par défaut 50,00 EUR quand
le mode est HORAIRE et le tarif vide (constante `DEFAULT_HOURLY_RATE`). OS, Fournir PC, Enregistrer, Dossier créé,
1er appel existaient déjà (lot 3). Liste des missions : colonne et filtre Centre, recherche sur le nom du centre.
Test `centre_annotations_et_tarif_par_defaut` (78 tests).

### Évolution 2026-11-18 : liste de référence des centres de formation
Migration 0008 : `partners.is_training_center` (les centres déjà référencés par des missions sont marqués).
Paramètres > Centres de formation (ADMIN, GESTIONNAIRE) : ajout d'un contact société/organisme existant ou création
d'un nouvel organisme, retrait de la liste (contact et missions conservés). Routes `/api/training-centers`
GET (tous) / POST / DELETE (ManageReferenceLists). Le champ "Centre de formation" de la mission (création, fiche,
filtre de la liste) est un menu alimenté par cette liste ; un centre retiré reste affiché "(hors liste)" sur les
missions qui le référencent. Test `liste_des_centres_de_formation` (79 tests).

### Évolution 2026-11-18 : facturation mensuelle par centre de formation
Besoin : les feuilles de temps quantifient le travail par tâche et par centre ; en fin de mois, une facture par centre.
- `GET /api/billing/monthly?month=AAAA-MM` : temps facturables non facturés datés jusqu'à la fin du mois (arriérés
  inclus et signalés), groupés par partenaire facturé (centre de la mission, sinon client direct) > mission > tâche
  et taux ; frais en attente des missions. `POST /api/billing/monthly {partner_id, month, invoice_date,
  include_expenses}` : une facture brouillon adressée au centre, une ligne par mission/tâche/taux
  ("MF-... Intitulé - Client - Tâche (du .. au ..)", heures x tarif de la mission), lignes de frais, temps et frais
  marqués facturés dans la même transaction (anti double facturation), note "Prestations de formation - mois".
- Page Factures > "Facturation mensuelle par centre" (/facturation/mensuelle) : choix du mois, date de facture,
  frais inclus ou non, un bloc par centre avec bouton "Créer la facture", total du mois. Lien depuis Feuilles de temps.
- Feuilles de temps : colonne centre sous le client, regroupements "Par centre de formation" et "Par tâche".
Test `facturation_mensuelle_par_centre` (80 tests).

### Évolution 2026-11-18 : statistiques par critères
Module `stats` : `GET /api/stats?from&to&group_by&center_id&client_id&trainer_id&software_id&modality&group_format&os`
(ADMIN, GESTIONNAIRE). Faits lus en SQL puis agrégés en Rust avec des dimensions communes (centre, client, formateur,
logiciel, modalité, format, OS) : temps (date de saisie), missions (première session ou date de début, hors annulées,
stagiaires distincts, sessions réalisées), factures validées/payées par date de facture (avoirs déduits, ventilation
temps/frais/forfait), paiements par date. Regroupements : centre, client, formateur, logiciel, modalité, format, OS,
mois, année, total. Factures/paiements sans mission rattachés au partenaire (centre si `is_training_center`).
`GET /api/stats/export.csv?...&dataset=synthese|timesheets|missions|invoices|payments` : CSV Excel (BOM, ;) avec les
dimensions sur chaque ligne. Page Statistiques (menu) : critères, 5 indicateurs, tableau, exports.
Correctif : la facture mensuelle d'un centre à mission unique n'est plus rattachée au projet (client différent).
Test `statistiques_par_centre_et_export` (81 tests).

### Évolution 2026-11-18 : produits et TVA optionnelle
Migration 0009 : table `products` (nom unique, libellé facture, unité, prix HT en centimes, TVA en points de
base, actif, ordre) pré-remplie avec "Heure de formation" (50 EUR/h) et "Journée de formation" (350 EUR/jour) ;
`invoice_lines.product_id`. Routes `/api/products` GET / POST, `/api/products/{id}` PATCH (ManageReferenceLists).
Paramètres > Produits : création / modification / archivage. Facture : sélecteur de produit sur chaque ligne
(remplit libellé, unité, prix, TVA) ; case "Appliquer la TVA sur cette facture" en franchise (lignes à 20 % par
défaut, modifiables) ; le serveur accepte désormais un taux explicite quel que soit le régime et n'imprime la
mention d'exonération que si la facture ne porte aucune TVA. Mission : sélecteur "Produit (remplit le tarif)"
(unité jour -> tarif journalier, sinon horaire). Test `produits_et_tva_optionnelle` (82 tests).

### Évolution 2026-11-18 : gestion des dépenses
Migration 0010 : table `expenses` (date, libellé, catégorie parmi 13, fournisseur contact ou libre, HT / TVA / TTC
en centimes, mode de paiement, statut A_PAYER / PAYEE + date, mission liée facultative, déductible, notes,
justificatif stocké sous documents/depenses/<id>/). Module `expenses` : `/api/depenses` GET (liste filtrée +
totaux + ventilation par catégorie et par mois) / POST, `/api/depenses/{id}` GET / PUT / DELETE,
`/api/depenses/{id}/pay`, `/api/depenses/{id}/receipt` GET / POST (10 Mo), `/api/depenses/export.csv`
(ADMIN, GESTIONNAIRE). Page Dépenses (menu) : filtres, 4 indicateurs, ventilations cliquables, liste, fenêtre de
saisie (TVA par taux ou montant, justificatif), marquage payé. Tableau de bord : dépenses de l'année / du mois,
à payer, résultat brut indicatif. Statistiques : colonne "Dépenses TTC" et export CSV des dépenses.
Test `depenses_de_l_entreprise` (83 tests).

### Évolution 2026-11-18 : état des lieux de la facturation
Migration 0011 : `invoices.sent_at`. `POST /api/invoices/{id}/sent` (marque envoyée, brouillon refusé), filtre
`sent=true|false` sur la liste, colonne "Envoyée". `GET /api/invoices/overview?year` : brouillons, validées non
envoyées, envoyées dans les délais, en retard (reste dû), retards par ancienneté (1-30, 31-60, 61-90, 90+ jours),
bilan de l'année (facturé HT avoirs déduits, payées, encaissé année / mois, avoirs, délai moyen de paiement),
reste dû par client, liste des factures en attente avec jours de retard. Page Factures > "État des lieux"
(/facturation/etat) avec actions rapides (Envoyée, Relance faite, PDF). Test `etat_des_lieux_des_factures` (84 tests).

### Évolution 2026-11-18 : renommage en Mercure
L'application s'appelle Mercure : exécutable `app\mercure.exe` (binaire Cargo `mercure`, la bibliothèque interne
garde le nom `erp`), configuration `mercure.config.json` (l'ancien `erp.config.json` est lu en repli), titre,
menu, écrans de connexion, messages console, producteur du PDF, README et build.ps1. Dossier de données,
base `erp.db` et routes API inchangés : aucune migration.

### Évolution 2026-11-19 : facturation électronique (réforme française)
Calendrier : réception obligatoire pour toutes les entreprises le 1er septembre 2026, émission obligatoire pour les
TPE/PME le 1er septembre 2027, via une Plateforme Agréée (PA). Mercure produit les fichiers conformes et suit leur
transmission ; le dépôt sur la PA reste manuel (pas d'API de PA raccordée).
- Migration 0012 : réglages (activation, nom de la plateforme, TVA sur les débits, catégorie d'opération par
  défaut BIENS/SERVICES/MIXTE) ; sur la facture : SIREN client figé à la validation, catégorie d'opération, statut
  de transmission (A_TRANSMETTRE, TRANSMISE, ACCEPTEE, REFUSEE, HORS_CHAMP pour les particuliers), référence et
  message de la plateforme.
- Validation : si l'option est active, le SIREN du client professionnel (ou de sa société mère) est obligatoire.
- `einvoice.rs` : XML CII profil EN 16931 (TypeCode 380/381, BusinessProcess A1/A2/A3, SIREN schemeID 0002,
  mention VATEX-FR-FRANCHISE), XML UBL 2.1, métadonnées XMP Factur-X.
- `pdf.rs` : le PDF des factures validées devient un PDF hybride Factur-X (PDF 1.7, factur-x.xml embarqué,
  /AF, XMP pdfaid part 3 + schéma fx). Limite : polices non embarquées, donc PDF/A-3 non strict.
- Routes : GET /invoices/{id}/xml?format=cii|ubl, POST /invoices/{id}/einvoice (statut/référence/message).
- Interface : Paramètres > Entreprise (bloc "Facturation électronique"), fiche facture (catégorie d'opération,
  carte "Facturation électronique" avec téléchargements et suivi), liste (badge + filtre e-facture), état des
  lieux (compteurs à transmettre / transmises / acceptées / refusées / hors champ).
- Test d'intégration `facturation_electronique` (87 tests au total).
