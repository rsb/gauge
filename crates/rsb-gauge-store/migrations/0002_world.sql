-- Schema v2: requests from labels, and the ticket a loop opens on (S-0001
-- ticket 05). Columns are added, never rebuilt, so v1 rows stay as they are:
-- a v1 request has no label event, a v1 loop no ticket.

-- A request from a label is keyed by (repo, issue, label event id). The label
-- stays on the issue, so every tick sees it again; only a new event inserts.
ALTER TABLE request ADD COLUMN label_event INTEGER;

CREATE UNIQUE INDEX request_label ON request (repo, issue, label_event)
    WHERE label_event IS NOT NULL;

-- The ticket as `S-NNNN/N` and its file, and the default branch at the
-- commit they and the lockfile (its SHA-256) were read from.
ALTER TABLE loop ADD COLUMN ticket TEXT;
ALTER TABLE loop ADD COLUMN ticket_path TEXT;
ALTER TABLE loop ADD COLUMN base_branch TEXT;
ALTER TABLE loop ADD COLUMN base_sha TEXT;
ALTER TABLE loop ADD COLUMN lockfile TEXT;
