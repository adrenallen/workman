CREATE TABLE project_folders (
    id         INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name       TEXT NOT NULL COLLATE NOCASE CHECK (length(trim(name)) > 0),
    collapsed  INTEGER NOT NULL DEFAULT 0 CHECK (collapsed IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    UNIQUE (profile_id, name),
    UNIQUE (profile_id, id)
);

ALTER TABLE profile_projects
ADD COLUMN folder_id INTEGER REFERENCES project_folders(id) ON DELETE SET NULL;

CREATE INDEX project_folders_profile_order_idx
ON project_folders(profile_id, sort_order, id);

CREATE INDEX profile_projects_folder_order_idx
ON profile_projects(profile_id, folder_id, sort_order, project_id);

CREATE TRIGGER profile_projects_folder_profile_insert
BEFORE INSERT ON profile_projects
WHEN NEW.folder_id IS NOT NULL
 AND NOT EXISTS (
     SELECT 1 FROM project_folders
     WHERE id = NEW.folder_id AND profile_id = NEW.profile_id
 )
BEGIN
    SELECT RAISE(ABORT, 'project folder belongs to another profile');
END;
CREATE TRIGGER profile_projects_folder_profile_update
BEFORE UPDATE OF profile_id, folder_id ON profile_projects
WHEN NEW.folder_id IS NOT NULL
 AND NOT EXISTS (
     SELECT 1 FROM project_folders
     WHERE id = NEW.folder_id AND profile_id = NEW.profile_id
 )
BEGIN
    SELECT RAISE(ABORT, 'project folder belongs to another profile');
END;
