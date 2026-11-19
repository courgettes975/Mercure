-- Numéro de déclaration d'activité (organisme de formation), 11 chiffres.
ALTER TABLE company_settings ADD COLUMN nda TEXT NOT NULL DEFAULT '';
