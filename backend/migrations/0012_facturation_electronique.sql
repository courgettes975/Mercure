-- Facturation électronique (réforme française : réception obligatoire depuis le 1er septembre 2026,
-- émission par les TPE/PME au 1er septembre 2027, via une plateforme agréée, formats Factur-X / UBL / CII).
ALTER TABLE company_settings ADD COLUMN einvoicing_enabled INTEGER NOT NULL DEFAULT 0 CHECK (einvoicing_enabled IN (0, 1));
ALTER TABLE company_settings ADD COLUMN einvoicing_platform TEXT NOT NULL DEFAULT '';        -- nom de la plateforme agréée choisie
ALTER TABLE company_settings ADD COLUMN vat_on_debits INTEGER NOT NULL DEFAULT 0 CHECK (vat_on_debits IN (0, 1)); -- option TVA d'après les débits
ALTER TABLE company_settings ADD COLUMN default_operation_category TEXT NOT NULL DEFAULT 'SERVICES'
    CHECK (default_operation_category IN ('SERVICES', 'BIENS', 'MIXTE'));
-- Mentions figées sur la facture et suivi de transmission.
ALTER TABLE invoices ADD COLUMN billing_siren TEXT NOT NULL DEFAULT '';                      -- SIREN du client figé à la validation
ALTER TABLE invoices ADD COLUMN operation_category TEXT NOT NULL DEFAULT 'SERVICES'
    CHECK (operation_category IN ('SERVICES', 'BIENS', 'MIXTE'));
ALTER TABLE invoices ADD COLUMN einvoice_status TEXT
    CHECK (einvoice_status IN ('A_TRANSMETTRE', 'TRANSMISE', 'ACCEPTEE', 'REFUSEE', 'HORS_CHAMP'));
ALTER TABLE invoices ADD COLUMN einvoice_ref TEXT NOT NULL DEFAULT '';                       -- référence donnée par la plateforme
ALTER TABLE invoices ADD COLUMN einvoice_message TEXT NOT NULL DEFAULT '';                   -- motif de refus, remarque
ALTER TABLE invoices ADD COLUMN einvoice_updated_at TEXT;
CREATE INDEX idx_invoices_einvoice ON invoices(einvoice_status);
