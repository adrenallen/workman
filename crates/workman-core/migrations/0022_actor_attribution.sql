ALTER TABLE scratchpads ADD COLUMN created_by TEXT NOT NULL DEFAULT 'workman';
ALTER TABLE scratchpads ADD COLUMN updated_by TEXT NOT NULL DEFAULT 'workman';
