# Modèle de domaine

Mis à jour : 2026-11-18. Sources : cahier des charges + captures TIM-001, CNT-001, CNT-002, INV-001.
Légende : [C] confirmé par capture, [S] exigé par la spec, [D] déduit, [?] à confirmer.

## Conventions
- Base SQLite (voir implementation-plan). Clés primaires INTEGER auto. Références métier lisibles séparées (MF-2026-0001, FAC-2026-000001).
- Montants et taux : INTEGER en centimes. TVA : INTEGER en points de base (2000 = 20 %). Jamais de REAL/FLOAT.
- Dates : TEXT ISO 8601. Clés étrangères activées (PRAGMA foreign_keys = ON).
- Durées : minutes entières (INTEGER). Affichage "2h 30min" [C].
- Toutes les tables : created_at, updated_at, created_by, updated_by. Champ is_demo pour les données fictives [S].
- Suppression : archivage (archived_at) par défaut ; suppression physique refusée si des objets liés existent [S].

## Entités

### User [S]
id, email (unique), password_hash, display_name, role (ADMIN | GESTIONNAIRE | UTILISATEUR), active, default_hourly_rate [?].

### Software [C]
id, name (unique), active. Liste de référence des logiciels enseignés.

### Partner (contact ou société, table unique) [S][C] - implémenté au lot 2 (table partners)
Implémentation : name (nom ou raison sociale) + first_name (personnes) ; customer_number CLI-00001 attribué
automatiquement quand le contact devient client ; client et prospect exclusifs ; seul un particulier peut être
rattaché à une entreprise ou un organisme ; search_text normalisé (sans accents) pour la recherche ;
notes datées dans partner_notes.
id, kind (PARTICULIER | ENTREPRISE | ORGANISME), parent_company_id -> Partner (une société a plusieurs interlocuteurs) [S][C],
is_client, is_prospect, is_stagiaire [C "Status: Stagiaire"], is_trainer [?],
last_name, first_name, company_name, job_title (Poste) [C], email, phone, mobile,
street, street2, zip, city, state, country_code [C], website [C], notes, customer_number (unique),
siren, siret, vat_number [C], duns [C][?], archived_at.
Index : nom, email, société (recherche [S]). Contraintes : email valide, SIREN 9 chiffres, SIRET 14 chiffres [D].

### Tag [C][S]
id, name (unique). Liaison N-N Partner_Tag.

### Project [S]
id, reference (unique), name, client_id -> Partner, description, start_date, end_date, status, billable, hourly_rate, members (N-N User).

### Task [C][D]
id, project_id -> Project, name. Vu dans TIM-001 ("ONE LEARN / certification", "ONE LEARN / Mentora").

### TrainingMission [S][C]
Hypothèse [D] : une mission est un Project de type FORMATION (relation 1-1 project_id unique), ce qui réutilise temps et facturation sans doublon.
reference MF-AAAA-NNNN (séquence serveur), title (Intitulé) [C], client_id -> Partner (Centre) [C], main_contact_id -> Partner, trainer_id -> User,
status (BROUILLON | A_PLANIFIER | PLANIFIEE | CONFIRMEE | EN_COURS | REALISEE | A_FACTURER | FACTUREE | ANNULEE) [S],
software_id -> Software (liste modifiable : AutoCAD, Revit, Fusion, SolidWorks, ...) [C], os (PC | MAC) [C], provide_pc [C], recording [C][?],
modality (PRESENTIEL | DISTANCIEL | HYBRIDE) [C "Type"], group_format (INDIVIDUEL | GROUPE) [C "Type"], start_date, end_date, days_count, planned_minutes, schedule_text, address, room, visio_link,
hourly_rate, daily_rate, flat_fee, planned_total (calculé serveur), special_conditions, description, internal_notes,
checklist : file_created, first_call_done, planned_flag, invoice_sent [C] (à fusionner avec le statut [?]),
pédagogie [S] : objectives, prerequisites, program, methods, means, evaluation_terms, evaluation_result, satisfaction, trainer_comment.

### MissionParticipant [S]
id, mission_id, partner_id -> Partner (stagiaire), progress_status (EN_COURS | TERMINE) [C "Status"], attendance_status, minutes_done, evaluation, comment. Unique (mission_id, partner_id).

### TrainingSession [S]
id, mission_id, date, start_time, end_time, duration_minutes (calculé), trainer_id, location, status (PREVUE | REALISEE | ANNULEE), notes.
SessionAttendance : session_id, participant_id, present. Unique (session_id, participant_id).

### MissionExpense [S]
id, mission_id, type (DEPLACEMENT | KILOMETRIQUE | HEBERGEMENT | RESTAURATION | AUTRE), label, quantity, unit_amount, amount, billable, invoice_line_id (nullable, unique).

### Timesheet [S][C]
id, date, user_id, project_id, task_id (nullable), session_id (nullable), description, duration_minutes (> 0),
billable, hourly_rate (copie au moment de la saisie), amount (calculé serveur),
status (BROUILLON | SOUMIS | VALIDE) [C menu "À valider"][?], invoice_line_id (nullable), invoiced_at.
Anti-doublon : unique (session_id, user_id) pour les temps générés depuis une session [S].
Le client se déduit du projet (pas stocké en double) [D].

### Invoice [S][C]
id, number (unique, attribué à la validation seulement) [D], kind (FACTURE | AVOIR), reversed_invoice_id -> Invoice (pour avoir),
partner_id, billing_address (copie figée à la validation), invoice_date, due_date, currency (EUR),
status (BROUILLON | VALIDEE | PAYEE | ANNULEE) [S][C "Comptabilisé", "Payé"],
total_ht, total_tva, total_ttc, amount_paid, amount_due (calculés serveur), mission_id (nullable), payment_terms,
legal_mentions (copie figée), last_reminder_at [C][?], validated_at, validated_by.

### InvoiceLine [S]
id, invoice_id, position, description, quantity (> 0), unit, unit_price (>= 0), vat_rate, total_ht, total_tva, total_ttc,
source_type (MANUEL | TEMPS | FRAIS | FORFAIT).

### Payment [S]
id, invoice_id, date, amount (> 0), method, reference. Somme des paiements <= total TTC.

### Sequence [S]
key (ex. "FAC-2026", "MF-2026"), prefix, next_value. Incrément dans la même transaction que la validation (BEGIN IMMEDIATE).

### CompanySettings [S]
raison sociale, adresse, SIREN/SIRET, TVA, NDA (n° de déclaration d'activité, 11 chiffres) [demande 2026-11-18], email, téléphone, logo, IBAN, BIC, banque, conditions de paiement, délai d'échéance par défaut,
mentions légales paramétrables, régime TVA (franchise en base ou assujetti) [D][?].

### Document [S][C "fiche detail"]
id, owner_type (MISSION | PARTNER | INVOICE), owner_id, category (CONVENTION | CONVOCATION | PROGRAMME | DEVIS | BON_COMMANDE | EMARGEMENT | ATTESTATION | CERTIFICAT | EVALUATION | FACTURE | CLIENT | AUTRE),
file_name, storage_path, mime_type, size, sha256, uploaded_by.

### AuditLog [S][C historique CNT-002]
id, user_id, action, entity_type, entity_id, changes (JSON ancienne/nouvelle valeur), created_at. Non modifiable.

### Note [C][?]
id, entity_type, entity_id, author_id, body. (Panneau "Note" de CNT-002.)

## Relations principales
- Partner (société) 1-N Partner (interlocuteurs)
- Partner (client) 1-N Project, 1-N Invoice
- Project 1-1 TrainingMission (si formation) [D]
- Project 1-N Task, 1-N Timesheet
- TrainingMission 1-N TrainingSession, 1-N MissionParticipant, 1-N MissionExpense, 1-N Document
- TrainingSession 1-N Timesheet (une par formateur)
- Invoice 1-N InvoiceLine, 1-N Payment
- InvoiceLine 1-N Timesheet (regroupement), 1-1 MissionExpense

## Règles métier
| Règle | Source |
|---|---|
| Une ligne de temps ou un frais ne peut être facturé qu'une fois (invoice_line_id renseigné = verrouillé) | [S] |
| Une facture validée n'est plus modifiable ; correction par avoir | [S] |
| Numéro attribué à la validation, séquentiel, sans trou, côté serveur | [S][D] |
| Totaux recalculés côté serveur, montants du navigateur ignorés | [S] |
| PAYEE quand amount_paid = total_ttc | [D] |
| Facture en retard = échéance dépassée et amount_due > 0 (affichage rouge) | [C][D] |
| Montant temps = minutes / 60 x taux, arrondi au centime | [S][D] |
| Session REALISEE -> génération idempotente des temps (pas de doublon si regénérée) | [S] |
| Modification d'une mission déjà facturée : bloquée sur les éléments facturés | [S][D] |
| Contact non supprimable s'il a factures, temps ou missions | [S] |

## Points à confirmer
(Réglés le 2026-11-18 : pas de TVA par défaut mais régime paramétrable ; nouvelle séquence FAC-AAAA-NNNNNN.)
1. Date de bascule depuis Odoo.
(Réglés : rouge de la grille non retenu ; valeurs Type, OS, Logiciel, Status connues.)
4. Workflow de validation des temps.
6. Reprise des données Odoo (119 contacts, 56 factures).

## Implémentation (lots 3 à 9, 2026-11-18)
Tables : projects, project_members, tasks, missions (1-1 projects, kind FORMATION), mission_participants,
training_sessions, session_attendance, mission_expenses, documents, timesheets, invoices, invoice_lines, payments.
Règles codées :
- Session réalisée -> ligne de temps unique par formateur (index unique session_id + user_id), recalculée si la
  session change, supprimée si la session est annulée ; verrouillée une fois facturée.
- Tarif journalier converti en horaire sur 7 h/jour ; forfait facturé une fois (ligne FORFAIT), temps non facturables.
- Facture : numéro attribué à la validation (séquence par préfixe et année), chronologie des dates imposée,
  adresse client et mentions figées, triggers SQL interdisant toute modification/suppression après validation.
- Correction = avoir total validé immédiatement (préfixe AV) ; facture d'origine ANNULEE ; temps et frais libérés.
- Paiements partiels ; PAYEE quand le total est atteint ; trop-perçu refusé.
- Mission FACTUREE automatiquement quand plus rien n'est à facturer et qu'aucune session n'est prévue.
- UTILISATEUR : ne voit et ne saisit que ses propres temps ; pas d'accès à la facturation.

### Complément 2026-11-18 : missions
- `center_id` -> partners (ENTREPRISE/ORGANISME) : centre de formation donneur d'ordre, distinct du client facturé.
- `invoice_sent` : suivi manuel "facture envoyée" (indépendant du statut FACTUREE calculé).
- `annotations` : texte libre, distinct de `internal_notes`.
- Tarif horaire par défaut : 50,00 EUR si non renseigné en mode HORAIRE.
