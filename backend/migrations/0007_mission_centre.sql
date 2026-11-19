-- Centre de formation (organisme donneur d'ordre), suivi facture envoyée, annotations libres
ALTER TABLE missions ADD COLUMN center_id INTEGER REFERENCES partners(id) ON DELETE RESTRICT;
ALTER TABLE missions ADD COLUMN invoice_sent INTEGER NOT NULL DEFAULT 0 CHECK (invoice_sent IN (0, 1));
ALTER TABLE missions ADD COLUMN annotations TEXT NOT NULL DEFAULT '';
CREATE INDEX idx_missions_center ON missions(center_id);
