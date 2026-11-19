# Migrations SQL

Appliquées automatiquement au démarrage (sqlx). Un fichier déjà appliqué ne doit jamais être modifié
(son empreinte est vérifiée) : toute évolution du schéma = un nouveau fichier numéroté.

| Fichier | Contenu |
|---|---|
| 0001_socle.sql | users, sessions (cookies), audit_log (append-only par triggers), sequences (numérotation), company_settings (ligne unique id=1), software |
| 0002_nda.sql | company_settings.nda (numéro de déclaration d'activité) |
| 0003_contacts.sql | partners (particuliers, entreprises, organismes, rattachement parent_id), tags, partner_tags, partner_notes |
| 0004_import.sql | partners.import_ref unique ("odoo:<nom>") : relance d'import sans doublon |
| 0005_projets_missions.sql | projects, project_members, tasks ; missions (projet FORMATION), mission_participants, training_sessions, session_attendance, mission_expenses, documents |
| 0006_temps_facturation.sql | invoices, invoice_lines, payments, timesheets ; triggers de verrouillage des factures validées ; index unique session/formateur |
| 0007_mission_centre.sql | missions.center_id (centre de formation), invoice_sent, annotations |
| 0008_centres_formation.sql | partners.is_training_center (liste de référence des centres) |
| 0009_produits.sql | products (liste des prestations : unité, prix HT, TVA) ; invoice_lines.product_id |
| 0010_depenses.sql | expenses (dépenses de l'entreprise : montants HT/TVA/TTC, catégorie, fournisseur, statut, mission liée, justificatif) |
| 0011_facture_envoyee.sql | invoices.sent_at (date d'envoi au client) |
| 0012_facturation_electronique.sql | company_settings.einvoicing_* / vat_on_debits / default_operation_category ; invoices.billing_siren, operation_category, einvoice_status/ref/message/updated_at |

Conventions : montants en centimes (INTEGER), quantités en millièmes, taux de TVA en points de base,
dates "AAAA-MM-JJ", horodatages ISO 8601 UTC. Toutes les clés étrangères sont vérifiées (foreign_keys = ON).
