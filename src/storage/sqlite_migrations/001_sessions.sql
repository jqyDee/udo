CREATE TABLE sessions (
    id               TEXT PRIMARY KEY,   -- UUID v7
    task_id          TEXT NOT NULL,      -- NodeId, no foreign key (tree in files)
    task_name        TEXT NOT NULL,
    task_description TEXT NOT NULL,
    container_path   TEXT NOT NULL,
    started_at       INTEGER NOT NULL,   -- UTC ms
    start_offset     INTEGER NOT NULL,   -- minutes
    ended_at         INTEGER,            -- NULL = running
    end_offset       INTEGER,
    source           TEXT NOT NULL,      -- manual | nvim | tmux | …
    created_at       INTEGER NOT NULL,   -- store clock
    created_offset   INTEGER NOT NULL,
    deleted_at       INTEGER,            -- NULL = visible
    deleted_offset   INTEGER
) STRICT;

CREATE TABLE session_edits (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions (id),
    at          INTEGER NOT NULL,        -- store clock
    at_offset   INTEGER NOT NULL,
    kind        TEXT NOT NULL,           -- edit | split | cut | delete
    old_start   INTEGER, old_start_offset INTEGER,
    old_end     INTEGER, old_end_offset   INTEGER,
    new_start   INTEGER, new_start_offset INTEGER,
    new_end     INTEGER, new_end_offset   INTEGER
) STRICT;

-- one timer: every running row indexes the constant 1, a second one collides
CREATE UNIQUE INDEX one_running ON sessions ((1))
    WHERE ended_at IS NULL AND deleted_at IS NULL;

CREATE INDEX sessions_by_task  ON sessions (task_id);
CREATE INDEX sessions_by_start ON sessions (started_at);
CREATE INDEX edits_by_session  ON session_edits (session_id, at);
