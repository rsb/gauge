-- Schema v1: loops, requests, flags, events and the trigger count.
-- Plain SQLite with no extensions and no pragma the schema relies on, so it
-- carries over to Cloudflare (Durable Objects or D1) at M5. Closed
-- vocabularies are text read back by the library, which refuses an unknown
-- value; no CHECK lists them, so a new member needs no table rebuild.

CREATE TABLE setting (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE loop (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo TEXT NOT NULL,
    issue INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    agent TEXT NOT NULL,
    state TEXT NOT NULL,
    park TEXT,
    situation TEXT,
    opened_at INTEGER NOT NULL,
    UNIQUE (repo, issue, ordinal)
);

-- The loop's natural key: at most one loop that is not closed per issue.
CREATE UNIQUE INDEX loop_one_open ON loop (repo, issue) WHERE state <> 'closed';

CREATE TABLE request (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo TEXT NOT NULL,
    issue INTEGER NOT NULL,
    agent TEXT NOT NULL,
    state TEXT NOT NULL,
    loop_id INTEGER REFERENCES loop (id),
    created_at INTEGER NOT NULL,
    settled_at INTEGER
);

CREATE INDEX request_state ON request (state);

CREATE TABLE flag (
    kind TEXT NOT NULL,
    loop_id INTEGER REFERENCES loop (id),
    set_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX flag_on_loop ON flag (kind, loop_id) WHERE loop_id IS NOT NULL;
CREATE UNIQUE INDEX flag_global ON flag (kind) WHERE loop_id IS NULL;

-- Append-only. AUTOINCREMENT keeps seq rising and never reused.
CREATE TABLE event (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    repo TEXT,
    issue INTEGER,
    loop_id INTEGER REFERENCES loop (id),
    payload TEXT NOT NULL
);

CREATE TABLE tick_trigger (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    count INTEGER NOT NULL
);

INSERT INTO tick_trigger (id, count) VALUES (1, 0);
