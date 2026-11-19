-- Produits / prestations : liste de référence qui pré-remplit les lignes de facture (libellé, unité, prix, TVA).
CREATE TABLE products (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0),
    description     TEXT    NOT NULL DEFAULT '',      -- libellé proposé sur la ligne (vide = nom)
    unit            TEXT    NOT NULL DEFAULT 'h',
    unit_price      INTEGER NOT NULL DEFAULT 0 CHECK (unit_price >= 0),   -- centimes HT
    vat_rate_bp     INTEGER NOT NULL DEFAULT 0 CHECK (vat_rate_bp BETWEEN 0 AND 10000),
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    position        INTEGER NOT NULL DEFAULT 0
);
INSERT INTO products (name, description, unit, unit_price, vat_rate_bp, position)
VALUES ('Heure de formation', 'Formation - heure', 'h', 5000, 0, 1),
       ('Journée de formation', 'Formation - journée (7 h)', 'jour', 35000, 0, 2);
-- Trace du produit d'origine sur la ligne de facture (statistiques, relecture).
ALTER TABLE invoice_lines ADD COLUMN product_id INTEGER REFERENCES products(id) ON DELETE SET NULL;
