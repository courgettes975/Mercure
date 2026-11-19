-- LOT 4 : feuilles de temps. LOTS 5-6 : factures, lignes, paiements, liaison temps/frais -> facture.

CREATE TABLE invoices (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    number              TEXT    UNIQUE,              -- attribué à la validation
    kind                TEXT    NOT NULL DEFAULT 'FACTURE' CHECK (kind IN ('FACTURE', 'AVOIR')),
    reversed_invoice_id INTEGER REFERENCES invoices(id) ON DELETE RESTRICT,
    partner_id          INTEGER NOT NULL REFERENCES partners(id) ON DELETE RESTRICT,
    project_id          INTEGER REFERENCES projects(id) ON DELETE RESTRICT,
    billing_name        TEXT    NOT NULL DEFAULT '',
    billing_address     TEXT    NOT NULL DEFAULT '',  -- figée à la validation
    billing_vat_number  TEXT    NOT NULL DEFAULT '',
    invoice_date        TEXT    NOT NULL,
    due_date            TEXT    NOT NULL,
    currency            TEXT    NOT NULL DEFAULT 'EUR',
    status              TEXT    NOT NULL DEFAULT 'BROUILLON' CHECK (status IN ('BROUILLON', 'VALIDEE', 'PAYEE', 'ANNULEE')),
    total_ht            INTEGER NOT NULL DEFAULT 0,
    total_vat           INTEGER NOT NULL DEFAULT 0,
    total_ttc           INTEGER NOT NULL DEFAULT 0,
    amount_paid         INTEGER NOT NULL DEFAULT 0,
    payment_terms       TEXT    NOT NULL DEFAULT '',
    vat_mention         TEXT    NOT NULL DEFAULT '',
    legal_mentions      TEXT    NOT NULL DEFAULT '',
    notes               TEXT    NOT NULL DEFAULT '',
    validated_at        TEXT,
    validated_by        INTEGER REFERENCES users(id),
    last_reminder_at    TEXT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    created_by          INTEGER REFERENCES users(id),
    CHECK (due_date >= invoice_date),
    CHECK ((status = 'BROUILLON') = (number IS NULL)),
    CHECK (amount_paid >= 0)
);
CREATE INDEX idx_invoices_partner ON invoices(partner_id);
CREATE INDEX idx_invoices_status ON invoices(status);

CREATE TABLE invoice_lines (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_id      INTEGER NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    position        INTEGER NOT NULL DEFAULT 0,
    description     TEXT    NOT NULL CHECK (length(trim(description)) > 0),
    quantity_milli  INTEGER NOT NULL CHECK (quantity_milli <> 0),
    unit            TEXT    NOT NULL DEFAULT '',
    unit_price      INTEGER NOT NULL CHECK (unit_price >= 0),
    vat_rate_bp     INTEGER NOT NULL DEFAULT 0 CHECK (vat_rate_bp BETWEEN 0 AND 10000),
    total_ht        INTEGER NOT NULL,
    total_vat       INTEGER NOT NULL,
    total_ttc       INTEGER NOT NULL,
    source_type     TEXT    NOT NULL DEFAULT 'MANUEL' CHECK (source_type IN ('MANUEL', 'TEMPS', 'FRAIS', 'FORFAIT'))
);
CREATE INDEX idx_invoice_lines_invoice ON invoice_lines(invoice_id);

CREATE TABLE payments (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_id      INTEGER NOT NULL REFERENCES invoices(id) ON DELETE RESTRICT,
    payment_date    TEXT    NOT NULL,
    amount          INTEGER NOT NULL CHECK (amount > 0),
    method          TEXT    NOT NULL DEFAULT 'VIREMENT' CHECK (method IN ('VIREMENT', 'CHEQUE', 'ESPECES', 'CARTE', 'PRELEVEMENT', 'AUTRE')),
    reference       TEXT    NOT NULL DEFAULT '',
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    created_by      INTEGER REFERENCES users(id)
);
CREATE INDEX idx_payments_invoice ON payments(invoice_id);

CREATE TABLE timesheets (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    date            TEXT    NOT NULL,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    task_id         INTEGER REFERENCES tasks(id) ON DELETE RESTRICT,
    session_id      INTEGER REFERENCES training_sessions(id) ON DELETE RESTRICT,
    description     TEXT    NOT NULL DEFAULT '',
    duration_minutes INTEGER NOT NULL CHECK (duration_minutes > 0 AND duration_minutes <= 1440),
    billable        INTEGER NOT NULL DEFAULT 1 CHECK (billable IN (0, 1)),
    hourly_rate     INTEGER NOT NULL DEFAULT 0 CHECK (hourly_rate >= 0),
    amount          INTEGER NOT NULL DEFAULT 0 CHECK (amount >= 0),
    status          TEXT    NOT NULL DEFAULT 'SAISI' CHECK (status IN ('SAISI', 'VALIDE')),
    invoice_line_id INTEGER REFERENCES invoice_lines(id),
    invoiced_at     TEXT,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    CHECK ((invoice_line_id IS NULL) = (invoiced_at IS NULL))
);
-- Une session ne génère qu'une ligne de temps par formateur : pas de double saisie.
CREATE UNIQUE INDEX idx_timesheets_session_user ON timesheets(session_id, user_id) WHERE session_id IS NOT NULL;
CREATE INDEX idx_timesheets_date ON timesheets(date);
CREATE INDEX idx_timesheets_project ON timesheets(project_id);
CREATE INDEX idx_timesheets_invoice_line ON timesheets(invoice_line_id);

-- Une facture validée ne peut plus être modifiée dans ses données comptables.
CREATE TRIGGER invoices_locked BEFORE UPDATE OF partner_id, invoice_date, total_ht, total_vat, total_ttc, number, kind, billing_address
ON invoices WHEN OLD.status <> 'BROUILLON'
BEGIN SELECT RAISE(ABORT, 'Facture validée : modification interdite'); END;
CREATE TRIGGER invoice_lines_locked_upd BEFORE UPDATE ON invoice_lines
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'BROUILLON'
BEGIN SELECT RAISE(ABORT, 'Facture validée : modification interdite'); END;
CREATE TRIGGER invoice_lines_locked_del BEFORE DELETE ON invoice_lines
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'BROUILLON'
BEGIN SELECT RAISE(ABORT, 'Facture validée : modification interdite'); END;
CREATE TRIGGER invoice_lines_locked_ins BEFORE INSERT ON invoice_lines
WHEN (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'BROUILLON'
BEGIN SELECT RAISE(ABORT, 'Facture validée : modification interdite'); END;
CREATE TRIGGER invoices_no_delete BEFORE DELETE ON invoices WHEN OLD.status <> 'BROUILLON'
BEGIN SELECT RAISE(ABORT, 'Facture validée : suppression interdite'); END;
