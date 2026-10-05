-- What udo estimated for a task, at the moments it matters (created, its
-- first session). Append-only like sessions: never changed or deleted,
-- also not when the task is deleted.
CREATE TABLE estimates (
    id            TEXT PRIMARY KEY,   -- UUID v7
    task_id       TEXT NOT NULL,      -- NodeId, no foreign key (tree in files)
    minutes       INTEGER NOT NULL,
    method        TEXT NOT NULL,      -- prior | average (later: manual, median, ai …)
    version       INTEGER NOT NULL,   -- of the method
    done_tasks    INTEGER NOT NULL,   -- learned from: done tasks (full weight)
    open_tasks    INTEGER NOT NULL,   -- open tasks over the estimate (half weight)
    prior_minutes INTEGER,            -- the prior used, if any
    reason        TEXT NOT NULL,      -- created | started (later: manual, ai)
    at            INTEGER NOT NULL,   -- store clock, UTC ms
    at_offset     INTEGER NOT NULL    -- minutes
) STRICT;

CREATE INDEX estimates_by_task ON estimates (task_id, at);