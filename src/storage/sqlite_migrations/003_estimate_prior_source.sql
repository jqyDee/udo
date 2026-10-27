-- Where an estimate's prior came from (#60), so a done task's estimate and
-- its explanation can be read from its row instead of being replayed.
-- NULL: written before; such rows are superseded once by a new row (the
-- backfill in `Core::estimates`), never changed.
ALTER TABLE estimates ADD COLUMN prior_source    TEXT;     -- none | setting | parent
ALTER TABLE estimates ADD COLUMN prior_container TEXT;     -- NodeId: setting, parent
ALTER TABLE estimates ADD COLUMN prior_tasks     INTEGER;  -- parent: tasks pooled
