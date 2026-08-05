ALTER TABLE projects ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0);

UPDATE projects
SET sort_order = (
    SELECT COUNT(*)
    FROM projects AS earlier
    WHERE earlier.id < projects.id
);

ALTER TABLE processes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0);

UPDATE processes
SET sort_order = (
    SELECT COUNT(*)
    FROM processes AS earlier
    WHERE earlier.project_id = processes.project_id
      AND earlier.kind = processes.kind
      AND earlier.id < processes.id
);

CREATE INDEX projects_sort_order_idx ON projects(sort_order, id);
CREATE INDEX processes_group_sort_order_idx
ON processes(project_id, kind, sort_order, id);
