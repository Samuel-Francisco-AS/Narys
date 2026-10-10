ALTER TABLE cognitive_continuations
 ADD COLUMN cancel_requested INTEGER NOT NULL DEFAULT 0
 CHECK(typeof(cancel_requested)='integer' AND cancel_requested IN (0,1)
 AND (cancel_requested=0 OR state IN ('running','paused')));
