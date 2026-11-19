-- LOT 2 : contacts (personnes, entreprises, organismes), étiquettes, notes.

CREATE TABLE partners (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    kind            TEXT    NOT NULL CHECK (kind IN ('PARTICULIER', 'ENTREPRISE', 'ORGANISME')),
    parent_id       INTEGER REFERENCES partners(id) ON DELETE RESTRICT,
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    first_name      TEXT    NOT NULL DEFAULT '',
    is_client       INTEGER NOT NULL DEFAULT 0 CHECK (is_client IN (0, 1)),
    is_prospect     INTEGER NOT NULL DEFAULT 0 CHECK (is_prospect IN (0, 1)),
    is_stagiaire    INTEGER NOT NULL DEFAULT 0 CHECK (is_stagiaire IN (0, 1)),
    job_title       TEXT    NOT NULL DEFAULT '',
    email           TEXT    NOT NULL DEFAULT '',
    phone           TEXT    NOT NULL DEFAULT '',
    mobile          TEXT    NOT NULL DEFAULT '',
    street          TEXT    NOT NULL DEFAULT '',
    street2         TEXT    NOT NULL DEFAULT '',
    zip             TEXT    NOT NULL DEFAULT '',
    city            TEXT    NOT NULL DEFAULT '',
    country         TEXT    NOT NULL DEFAULT 'France',
    website         TEXT    NOT NULL DEFAULT '',
    notes           TEXT    NOT NULL DEFAULT '',
    customer_number TEXT    UNIQUE,
    siren           TEXT    NOT NULL DEFAULT '',
    siret           TEXT    NOT NULL DEFAULT '',
    vat_number      TEXT    NOT NULL DEFAULT '',
    duns            TEXT    NOT NULL DEFAULT '',
    search_text     TEXT    NOT NULL DEFAULT '',
    archived_at     TEXT,
    is_demo         INTEGER NOT NULL DEFAULT 0 CHECK (is_demo IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    created_by      INTEGER REFERENCES users(id),
    updated_by      INTEGER REFERENCES users(id),
    CHECK (parent_id IS NULL OR parent_id <> id),
    -- Seule une personne peut être rattachée à une société.
    CHECK (parent_id IS NULL OR kind = 'PARTICULIER')
);
CREATE INDEX idx_partners_parent ON partners(parent_id);
CREATE INDEX idx_partners_name ON partners(name COLLATE NOCASE);
CREATE INDEX idx_partners_archived ON partners(archived_at);

CREATE TABLE tags (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    name    TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0)
);

CREATE TABLE partner_tags (
    partner_id  INTEGER NOT NULL REFERENCES partners(id) ON DELETE CASCADE,
    tag_id      INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (partner_id, tag_id)
);
CREATE INDEX idx_partner_tags_tag ON partner_tags(tag_id);

CREATE TABLE partner_notes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    partner_id  INTEGER NOT NULL REFERENCES partners(id) ON DELETE CASCADE,
    author_id   INTEGER REFERENCES users(id),
    body        TEXT    NOT NULL CHECK (length(trim(body)) > 0),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX idx_partner_notes_partner ON partner_notes(partner_id);
