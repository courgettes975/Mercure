-- Référence de la source d'import (ex. "odoo:SENZA, LOPEZ ...") : évite les doublons en cas de nouvel import.
ALTER TABLE partners ADD COLUMN import_ref TEXT;
CREATE UNIQUE INDEX idx_partners_import_ref ON partners(import_ref) WHERE import_ref IS NOT NULL;
