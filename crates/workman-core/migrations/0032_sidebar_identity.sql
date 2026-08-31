ALTER TABLE projects
ADD COLUMN name_color TEXT CHECK (
    name_color IS NULL OR name_color IN ('amber', 'blue', 'rose', 'slate', 'teal', 'violet')
);

ALTER TABLE project_folders
ADD COLUMN icon TEXT CHECK (
    icon IS NULL OR (length(icon) BETWEEN 1 AND 80)
);

ALTER TABLE project_folders
ADD COLUMN name_color TEXT CHECK (
    name_color IS NULL OR name_color IN ('amber', 'blue', 'rose', 'slate', 'teal', 'violet')
);
