-- Suivi de l'envoi des factures au client (date d'envoi), pour l'état des lieux de la facturation.
ALTER TABLE invoices ADD COLUMN sent_at TEXT;
