-- Dépenses de l'entreprise (achats, abonnements, déplacements...) avec justificatif, distinctes des frais
-- refacturables des missions (mission_expenses).
CREATE TABLE expenses (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    date            TEXT    NOT NULL CHECK (date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    label           TEXT    NOT NULL CHECK (length(trim(label)) > 0),
    category        TEXT    NOT NULL,
    supplier_id     INTEGER REFERENCES partners(id) ON DELETE SET NULL,
    supplier_name   TEXT    NOT NULL DEFAULT '',          -- fournisseur hors contacts
    amount_ht       INTEGER NOT NULL CHECK (amount_ht >= 0),     -- centimes
    vat_amount      INTEGER NOT NULL DEFAULT 0 CHECK (vat_amount >= 0),
    amount_ttc      INTEGER NOT NULL CHECK (amount_ttc >= 0),
    payment_method  TEXT    NOT NULL DEFAULT 'CARTE',
    status          TEXT    NOT NULL DEFAULT 'PAYEE' CHECK (status IN ('A_PAYER', 'PAYEE')),
    payment_date    TEXT,
    project_id      INTEGER REFERENCES projects(id) ON DELETE SET NULL,   -- mission concernée (facultatif)
    deductible      INTEGER NOT NULL DEFAULT 1 CHECK (deductible IN (0, 1)),
    notes           TEXT    NOT NULL DEFAULT '',
    receipt_name    TEXT,                                   -- justificatif : nom d'origine...
    receipt_storage TEXT    UNIQUE,                         -- ... chemin sous documents/
    receipt_mime    TEXT,
    receipt_size    INTEGER,
    created_by      INTEGER REFERENCES users(id),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX idx_expenses_date ON expenses(date);
CREATE INDEX idx_expenses_category ON expenses(category);
CREATE INDEX idx_expenses_project ON expenses(project_id);
