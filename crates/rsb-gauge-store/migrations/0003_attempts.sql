-- Schema v3: attempts and their briefs (S-0001 ticket 06). New tables only,
-- so v2 rows stay as they are.

-- The brief issued for (loop, attempt), recorded before its unit starts. The
-- brief is JSON text; the `brief_issued` event carries the same payload.
CREATE TABLE brief (
    loop_id INTEGER NOT NULL REFERENCES loop (id),
    attempt INTEGER NOT NULL,
    brief TEXT NOT NULL,
    issued_at INTEGER NOT NULL,
    PRIMARY KEY (loop_id, attempt)
);

-- One attempt of a loop. The unit name `gauge-run-<loop>-<attempt>` is the
-- unit's natural key. The skills checkout is its commit and whether the
-- mounted tree was clean (0 or 1). The end columns stay NULL until the
-- attempt ends; `cause` is set only for one that does not hand back.
CREATE TABLE attempt (
    loop_id INTEGER NOT NULL REFERENCES loop (id),
    number INTEGER NOT NULL,
    unit TEXT NOT NULL UNIQUE,
    agent TEXT NOT NULL,
    model TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    runtime_bound INTEGER NOT NULL,
    skills_commit TEXT NOT NULL,
    skills_clean INTEGER NOT NULL,
    ended_at INTEGER,
    exit_code INTEGER,
    cause TEXT,
    PRIMARY KEY (loop_id, number)
);
