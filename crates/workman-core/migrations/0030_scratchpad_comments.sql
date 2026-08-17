ALTER TABLE scratchpads ADD COLUMN comments_revision INTEGER NOT NULL DEFAULT 0;

-- Scratchpad content and comment offsets use one canonical LF coordinate space.
UPDATE scratchpads
SET content = replace(replace(content, char(13) || char(10), char(10)), char(13), char(10));

CREATE TABLE scratchpad_comments (
    id              INTEGER PRIMARY KEY,
    scratchpad_id   INTEGER NOT NULL REFERENCES scratchpads(id) ON DELETE CASCADE,
    actor           TEXT NOT NULL,
    body            TEXT NOT NULL CHECK (length(trim(body)) > 0 AND length(body) <= 65536),
    quote           TEXT CHECK (quote IS NULL OR length(quote) <= 4096),
    anchor_start    INTEGER CHECK (anchor_start IS NULL OR anchor_start >= 0),
    anchor_end      INTEGER CHECK (anchor_end IS NULL OR anchor_end >= 0),
    anchor_prefix   TEXT CHECK (anchor_prefix IS NULL OR length(anchor_prefix) <= 64),
    anchor_suffix   TEXT CHECK (anchor_suffix IS NULL OR length(anchor_suffix) <= 64),
    anchor_revision INTEGER CHECK (anchor_revision IS NULL OR anchor_revision >= 0),
    resolved        INTEGER NOT NULL DEFAULT 0 CHECK (resolved IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    CHECK (
        (quote IS NULL AND anchor_start IS NULL AND anchor_end IS NULL
         AND anchor_prefix IS NULL AND anchor_suffix IS NULL AND anchor_revision IS NULL)
        OR
        (quote IS NOT NULL AND length(quote) > 0
         AND ((anchor_start IS NULL AND anchor_end IS NULL)
              OR (anchor_start IS NOT NULL AND anchor_end IS NOT NULL
                  AND anchor_end > anchor_start)))
    )
);

CREATE INDEX scratchpad_comments_scratchpad_id_idx
    ON scratchpad_comments(scratchpad_id, resolved, created_at, id);

CREATE TRIGGER scratchpad_comments_revision_insert
AFTER INSERT ON scratchpad_comments
BEGIN
    UPDATE scratchpads SET comments_revision = comments_revision + 1 WHERE id = NEW.scratchpad_id;
END;

CREATE TRIGGER scratchpad_comments_revision_update
AFTER UPDATE ON scratchpad_comments
BEGIN
    UPDATE scratchpads SET comments_revision = comments_revision + 1 WHERE id = NEW.scratchpad_id;
END;

CREATE TRIGGER scratchpad_comments_revision_delete
AFTER DELETE ON scratchpad_comments
BEGIN
    UPDATE scratchpads SET comments_revision = comments_revision + 1 WHERE id = OLD.scratchpad_id;
END;
