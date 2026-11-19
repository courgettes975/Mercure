-- LOT 1 : socle (utilisateurs, sessions, journal, séquences, paramètres, logiciels)
-- Montants en centimes (INTEGER), taux de TVA en points de base (2000 = 20 %), dates en TEXT ISO 8601 UTC.

CREATE TABLE users (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    email           TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    display_name    TEXT    NOT NULL CHECK (length(trim(display_name)) > 0),
    password_hash   TEXT    NOT NULL,
    role            TEXT    NOT NULL CHECK (role IN ('ADMIN', 'GESTIONNAIRE', 'UTILISATEUR')),
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE sessions (
    token_hash      TEXT    PRIMARY KEY,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    expires_at      TEXT    NOT NULL
);
CREATE INDEX idx_sessions_user ON sessions(user_id);

CREATE TABLE audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id         INTEGER REFERENCES users(id),
    action          TEXT    NOT NULL,
    entity_type     TEXT    NOT NULL,
    entity_id       TEXT,
    changes         TEXT,   -- JSON
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX idx_audit_entity ON audit_log(entity_type, entity_id);
CREATE INDEX idx_audit_created ON audit_log(created_at);

-- Le journal est en ajout seul.
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'Le journal est en lecture seule'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'Le journal est en lecture seule'); END;

CREATE TABLE sequences (
    key             TEXT    PRIMARY KEY,      -- ex. FAC-2026
    next_value      INTEGER NOT NULL CHECK (next_value >= 1)
);

CREATE TABLE company_settings (
    id                      INTEGER PRIMARY KEY CHECK (id = 1),
    legal_name              TEXT    NOT NULL DEFAULT '',
    address_line1           TEXT    NOT NULL DEFAULT '',
    address_line2           TEXT    NOT NULL DEFAULT '',
    zip                     TEXT    NOT NULL DEFAULT '',
    city                    TEXT    NOT NULL DEFAULT '',
    country                 TEXT    NOT NULL DEFAULT 'France',
    siren                   TEXT    NOT NULL DEFAULT '',
    siret                   TEXT    NOT NULL DEFAULT '',
    vat_number              TEXT    NOT NULL DEFAULT '',
    email                   TEXT    NOT NULL DEFAULT '',
    phone                   TEXT    NOT NULL DEFAULT '',
    website                 TEXT    NOT NULL DEFAULT '',
    bank_name               TEXT    NOT NULL DEFAULT '',
    iban                    TEXT    NOT NULL DEFAULT '',
    bic                     TEXT    NOT NULL DEFAULT '',
    payment_terms           TEXT    NOT NULL DEFAULT '',
    default_due_days        INTEGER NOT NULL DEFAULT 30 CHECK (default_due_days BETWEEN 0 AND 365),
    vat_regime              TEXT    NOT NULL DEFAULT 'FRANCHISE' CHECK (vat_regime IN ('FRANCHISE', 'ASSUJETTI')),
    default_vat_rate_bp     INTEGER NOT NULL DEFAULT 0 CHECK (default_vat_rate_bp BETWEEN 0 AND 10000),
    vat_exemption_mention   TEXT    NOT NULL DEFAULT '',
    legal_mentions          TEXT    NOT NULL DEFAULT '',
    invoice_prefix          TEXT    NOT NULL DEFAULT 'FAC' CHECK (length(invoice_prefix) BETWEEN 1 AND 10),
    invoice_number_width    INTEGER NOT NULL DEFAULT 6 CHECK (invoice_number_width BETWEEN 3 AND 10),
    logo_file               TEXT,
    updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
INSERT INTO company_settings (id) VALUES (1);

CREATE TABLE software (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0),
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1))
);
INSERT INTO software (name) VALUES ('AutoCAD'), ('Revit'), ('Fusion'), ('SolidWorks');
