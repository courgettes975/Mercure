-- LOT 3 : projets et tâches. LOT 3b : missions de formation (une mission = un projet de type FORMATION).

CREATE TABLE projects (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    reference       TEXT    NOT NULL UNIQUE,
    kind            TEXT    NOT NULL DEFAULT 'PROJET' CHECK (kind IN ('PROJET', 'FORMATION')),
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    client_id       INTEGER NOT NULL REFERENCES partners(id) ON DELETE RESTRICT,
    description     TEXT    NOT NULL DEFAULT '',
    start_date      TEXT,
    end_date        TEXT,
    status          TEXT    NOT NULL DEFAULT 'EN_COURS' CHECK (status IN ('BROUILLON', 'EN_COURS', 'TERMINE', 'ANNULE')),
    billable        INTEGER NOT NULL DEFAULT 1 CHECK (billable IN (0, 1)),
    hourly_rate     INTEGER NOT NULL DEFAULT 0 CHECK (hourly_rate >= 0),   -- centimes
    archived_at     TEXT,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    created_by      INTEGER REFERENCES users(id),
    CHECK (end_date IS NULL OR start_date IS NULL OR end_date >= start_date)
);
CREATE INDEX idx_projects_client ON projects(client_id);

CREATE TABLE project_members (
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    PRIMARY KEY (project_id, user_id)
);

CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    active      INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    UNIQUE (project_id, name)
);

CREATE TABLE missions (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id          INTEGER NOT NULL UNIQUE REFERENCES projects(id) ON DELETE RESTRICT,
    reference           TEXT    NOT NULL UNIQUE,     -- MF-2026-0001
    status              TEXT    NOT NULL DEFAULT 'BROUILLON' CHECK (status IN (
                            'BROUILLON', 'A_PLANIFIER', 'PLANIFIEE', 'CONFIRMEE', 'EN_COURS',
                            'REALISEE', 'A_FACTURER', 'FACTUREE', 'ANNULEE')),
    contact_id          INTEGER REFERENCES partners(id) ON DELETE RESTRICT,
    trainer_id          INTEGER REFERENCES users(id) ON DELETE RESTRICT,
    software_id         INTEGER REFERENCES software(id) ON DELETE RESTRICT,
    os                  TEXT    NOT NULL DEFAULT '' CHECK (os IN ('', 'PC', 'MAC')),
    modality            TEXT    NOT NULL DEFAULT 'PRESENTIEL' CHECK (modality IN ('PRESENTIEL', 'DISTANCIEL', 'HYBRIDE')),
    group_format        TEXT    NOT NULL DEFAULT 'INDIVIDUEL' CHECK (group_format IN ('INDIVIDUEL', 'GROUPE')),
    provide_pc          INTEGER NOT NULL DEFAULT 0 CHECK (provide_pc IN (0, 1)),
    recording           INTEGER NOT NULL DEFAULT 0 CHECK (recording IN (0, 1)),
    planned_minutes     INTEGER NOT NULL DEFAULT 0 CHECK (planned_minutes >= 0),
    days_count          INTEGER NOT NULL DEFAULT 0 CHECK (days_count >= 0),
    schedule_text       TEXT    NOT NULL DEFAULT '',
    address             TEXT    NOT NULL DEFAULT '',
    room                TEXT    NOT NULL DEFAULT '',
    visio_link          TEXT    NOT NULL DEFAULT '',
    pricing_mode        TEXT    NOT NULL DEFAULT 'HORAIRE' CHECK (pricing_mode IN ('HORAIRE', 'JOURNALIER', 'FORFAIT')),
    daily_rate          INTEGER NOT NULL DEFAULT 0 CHECK (daily_rate >= 0),
    flat_fee            INTEGER NOT NULL DEFAULT 0 CHECK (flat_fee >= 0),
    special_conditions  TEXT    NOT NULL DEFAULT '',
    internal_notes      TEXT    NOT NULL DEFAULT '',
    file_created        INTEGER NOT NULL DEFAULT 0 CHECK (file_created IN (0, 1)),
    first_call_done     INTEGER NOT NULL DEFAULT 0 CHECK (first_call_done IN (0, 1)),
    objectives          TEXT    NOT NULL DEFAULT '',
    prerequisites       TEXT    NOT NULL DEFAULT '',
    program             TEXT    NOT NULL DEFAULT '',
    methods             TEXT    NOT NULL DEFAULT '',
    means               TEXT    NOT NULL DEFAULT '',
    evaluation_terms    TEXT    NOT NULL DEFAULT '',
    evaluation_result   TEXT    NOT NULL DEFAULT '',
    satisfaction        TEXT    NOT NULL DEFAULT '',
    trainer_comment     TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX idx_missions_status ON missions(status);

CREATE TABLE mission_participants (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id      INTEGER NOT NULL REFERENCES missions(id) ON DELETE CASCADE,
    partner_id      INTEGER NOT NULL REFERENCES partners(id) ON DELETE RESTRICT,
    progress_status TEXT    NOT NULL DEFAULT 'EN_COURS' CHECK (progress_status IN ('EN_COURS', 'TERMINE', 'ABANDON')),
    evaluation      TEXT    NOT NULL DEFAULT '',
    comment         TEXT    NOT NULL DEFAULT '',
    UNIQUE (mission_id, partner_id)
);

CREATE TABLE training_sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id      INTEGER NOT NULL REFERENCES missions(id) ON DELETE CASCADE,
    date            TEXT    NOT NULL,
    start_time      TEXT    NOT NULL,   -- HH:MM
    end_time        TEXT    NOT NULL,
    break_minutes   INTEGER NOT NULL DEFAULT 0 CHECK (break_minutes >= 0),
    duration_minutes INTEGER NOT NULL CHECK (duration_minutes > 0),
    trainer_id      INTEGER REFERENCES users(id) ON DELETE RESTRICT,
    location        TEXT    NOT NULL DEFAULT '',
    status          TEXT    NOT NULL DEFAULT 'PREVUE' CHECK (status IN ('PREVUE', 'REALISEE', 'ANNULEE')),
    notes           TEXT    NOT NULL DEFAULT '',
    CHECK (end_time > start_time)
);
CREATE INDEX idx_sessions_mission ON training_sessions(mission_id, date);
CREATE INDEX idx_sessions_date ON training_sessions(date);

CREATE TABLE session_attendance (
    session_id      INTEGER NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
    participant_id  INTEGER NOT NULL REFERENCES mission_participants(id) ON DELETE CASCADE,
    present         INTEGER NOT NULL CHECK (present IN (0, 1)),
    PRIMARY KEY (session_id, participant_id)
);

CREATE TABLE mission_expenses (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id      INTEGER NOT NULL REFERENCES missions(id) ON DELETE CASCADE,
    kind            TEXT    NOT NULL CHECK (kind IN ('DEPLACEMENT', 'KILOMETRIQUE', 'HEBERGEMENT', 'RESTAURATION', 'AUTRE')),
    label           TEXT    NOT NULL DEFAULT '',
    expense_date    TEXT,
    quantity_milli  INTEGER NOT NULL DEFAULT 1000 CHECK (quantity_milli > 0),   -- 1000 = 1 ; km pour les frais kilométriques
    unit_price      INTEGER NOT NULL CHECK (unit_price >= 0),                   -- centimes
    amount          INTEGER NOT NULL CHECK (amount >= 0),                       -- calculé par le serveur
    billable        INTEGER NOT NULL DEFAULT 1 CHECK (billable IN (0, 1)),
    invoice_line_id INTEGER,                                                    -- lot 6
    invoiced_at     TEXT
);
CREATE INDEX idx_expenses_mission ON mission_expenses(mission_id);

CREATE TABLE documents (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_type      TEXT    NOT NULL CHECK (owner_type IN ('MISSION', 'PARTNER', 'INVOICE')),
    owner_id        INTEGER NOT NULL,
    category        TEXT    NOT NULL,
    file_name       TEXT    NOT NULL,
    storage_name    TEXT    NOT NULL UNIQUE,
    mime_type       TEXT    NOT NULL,
    size            INTEGER NOT NULL,
    sha256          TEXT    NOT NULL,
    uploaded_by     INTEGER REFERENCES users(id),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX idx_documents_owner ON documents(owner_type, owner_id);
