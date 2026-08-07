ALTER TABLE todos
ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0);

UPDATE todos
SET sort_order = (
    SELECT COUNT(*)
    FROM todos AS newer
    WHERE newer.project_id = todos.project_id
      AND newer.id > todos.id
);

CREATE INDEX todos_project_sort_order_idx
ON todos (project_id, sort_order, id);

ALTER TABLE scratchpads
ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0);

UPDATE scratchpads
SET sort_order = (
    SELECT COUNT(*)
    FROM scratchpads AS newer
    WHERE newer.project_id = scratchpads.project_id
      AND newer.id > scratchpads.id
);

CREATE INDEX scratchpads_project_sort_order_idx
ON scratchpads (project_id, sort_order, id);
