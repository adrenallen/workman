//! Profile-scoped, single-level folders for the desktop project rail.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::{ProfileId, ProjectId, Store, StoreError, StoreResult};

pub type ProjectFolderId = i64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFolder {
    pub id: ProjectFolderId,
    pub name: String,
    pub icon: Option<String>,
    pub name_color: Option<String>,
    pub collapsed: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedProjectFolder {
    pub name: String,
    pub icon: Option<String>,
    pub name_color: Option<String>,
    pub collapsed: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedProjectFolderMembership {
    pub project_path: String,
    pub folder_name: Option<String>,
    pub sort_order: i64,
}

/// The complete single-level project rail. Projects occur exactly once, either directly at the
/// top level or as a child of one folder. Folders cannot be children of folders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectLayoutEntry {
    Project {
        id: ProjectId,
    },
    Folder {
        id: ProjectFolderId,
        project_ids: Vec<ProjectId>,
    },
}

impl Store {
    pub fn list_project_folders(&self) -> StoreResult<Vec<ProjectFolder>> {
        self.list_project_folders_for(self.active_profile_id()?)
    }

    pub fn list_project_folders_for(
        &self,
        profile_id: ProfileId,
    ) -> StoreResult<Vec<ProjectFolder>> {
        let mut statement = self.connection().prepare(
            "SELECT id, name, icon, name_color, collapsed, sort_order
             FROM project_folders
             WHERE profile_id = ?1
             ORDER BY sort_order, id",
        )?;
        Ok(statement
            .query_map([profile_id], |row| {
                Ok(ProjectFolder {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    icon: row.get(2)?,
                    name_color: row.get(3)?,
                    collapsed: row.get(4)?,
                    sort_order: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn project_folder_id(&self, project_id: ProjectId) -> StoreResult<Option<ProjectFolderId>> {
        self.project_folder_id_for(self.active_profile_id()?, project_id)
    }

    pub fn project_folder_id_for(
        &self,
        profile_id: ProfileId,
        project_id: ProjectId,
    ) -> StoreResult<Option<ProjectFolderId>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT folder_id FROM profile_projects
                 WHERE profile_id = ?1 AND project_id = ?2",
                params![profile_id, project_id],
                |row| row.get::<_, Option<ProjectFolderId>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn create_project_folder(&self, name: &str) -> StoreResult<ProjectFolder> {
        let name = normalized_folder_name(name)?;
        let profile_id = self.active_profile_id()?;
        let transaction = self.connection().unchecked_transaction()?;
        let id = transaction.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM project_folders",
            [],
            |row| row.get::<_, ProjectFolderId>(0),
        )?;
        let sort_order = next_top_level_order(&transaction, profile_id)?;
        transaction.execute(
            "INSERT INTO project_folders (id, profile_id, name, collapsed, sort_order)
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![id, profile_id, name, sort_order],
        )?;
        transaction.commit()?;
        Ok(ProjectFolder {
            id,
            name,
            icon: None,
            name_color: None,
            collapsed: false,
            sort_order,
        })
    }

    pub fn rename_project_folder(
        &self,
        folder_id: ProjectFolderId,
        name: &str,
    ) -> StoreResult<Option<ProjectFolder>> {
        let name = normalized_folder_name(name)?;
        let profile_id = self.active_profile_id()?;
        let changed = self.connection().execute(
            "UPDATE project_folders SET name = ?1 WHERE id = ?2 AND profile_id = ?3",
            params![name, folder_id, profile_id],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.project_folder_for_profile(profile_id, folder_id)
    }

    pub fn update_project_folder_settings(
        &self,
        folder_id: ProjectFolderId,
        name: &str,
        icon: Option<&str>,
        name_color: Option<&str>,
    ) -> StoreResult<Option<ProjectFolder>> {
        let name = normalized_folder_name(name)?;
        let profile_id = self.active_profile_id()?;
        let changed = self.connection().execute(
            "UPDATE project_folders
             SET name = ?1, icon = ?2, name_color = ?3
             WHERE id = ?4 AND profile_id = ?5",
            params![name, icon, name_color, folder_id, profile_id],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.project_folder_for_profile(profile_id, folder_id)
    }

    pub fn set_project_folder_collapsed(
        &self,
        folder_id: ProjectFolderId,
        collapsed: bool,
    ) -> StoreResult<bool> {
        let profile_id = self.active_profile_id()?;
        Ok(self.connection().execute(
            "UPDATE project_folders SET collapsed = ?1 WHERE id = ?2 AND profile_id = ?3",
            params![collapsed, folder_id, profile_id],
        )? > 0)
    }

    /// Delete only the container, lifting its children into the folder's former top-level slot.
    pub fn delete_project_folder(&self, folder_id: ProjectFolderId) -> StoreResult<bool> {
        let profile_id = self.active_profile_id()?;
        let transaction = self.connection().unchecked_transaction()?;
        let folder_order = transaction
            .query_row(
                "SELECT sort_order FROM project_folders WHERE id = ?1 AND profile_id = ?2",
                params![folder_id, profile_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(folder_order) = folder_order else {
            return Ok(false);
        };
        let child_ids = {
            let mut statement = transaction.prepare(
                "SELECT project_id FROM profile_projects
                 WHERE profile_id = ?1 AND folder_id = ?2
                 ORDER BY sort_order, project_id",
            )?;
            statement
                .query_map(params![profile_id, folder_id], |row| {
                    row.get::<_, ProjectId>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let delta = i64::try_from(child_ids.len()).unwrap_or(i64::MAX) - 1;
        transaction.execute(
            "UPDATE project_folders SET sort_order = sort_order + ?1
             WHERE profile_id = ?2 AND id != ?3 AND sort_order > ?4",
            params![delta, profile_id, folder_id, folder_order],
        )?;
        transaction.execute(
            "UPDATE profile_projects SET sort_order = sort_order + ?1
             WHERE profile_id = ?2 AND folder_id IS NULL AND sort_order > ?3",
            params![delta, profile_id, folder_order],
        )?;
        for (offset, project_id) in child_ids.into_iter().enumerate() {
            transaction.execute(
                "UPDATE profile_projects SET folder_id = NULL, sort_order = ?1
                 WHERE profile_id = ?2 AND project_id = ?3",
                params![folder_order + offset as i64, profile_id, project_id],
            )?;
        }
        transaction.execute(
            "DELETE FROM project_folders WHERE id = ?1 AND profile_id = ?2",
            params![folder_id, profile_id],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn project_layout(&self) -> StoreResult<Vec<ProjectLayoutEntry>> {
        self.project_layout_for(self.active_profile_id()?)
    }

    pub fn project_layout_for(
        &self,
        profile_id: ProfileId,
    ) -> StoreResult<Vec<ProjectLayoutEntry>> {
        let folders = self.list_project_folders_for(profile_id)?;
        let mut projects_by_folder: HashMap<ProjectFolderId, Vec<(i64, ProjectId)>> =
            HashMap::new();
        let mut top_projects = Vec::new();
        {
            let mut statement = self.connection().prepare(
                "SELECT project_id, folder_id, sort_order
                 FROM profile_projects WHERE profile_id = ?1
                 ORDER BY sort_order, project_id",
            )?;
            for row in statement.query_map([profile_id], |row| {
                Ok((
                    row.get::<_, ProjectId>(0)?,
                    row.get::<_, Option<ProjectFolderId>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })? {
                let (project_id, folder_id, sort_order) = row?;
                if let Some(folder_id) = folder_id {
                    projects_by_folder
                        .entry(folder_id)
                        .or_default()
                        .push((sort_order, project_id));
                } else {
                    top_projects.push((sort_order, project_id));
                }
            }
        }

        let mut top_level: Vec<(i64, bool, i64, ProjectLayoutEntry)> = folders
            .into_iter()
            .map(|folder| {
                let mut children = projects_by_folder.remove(&folder.id).unwrap_or_default();
                children.sort_unstable();
                (
                    folder.sort_order,
                    false,
                    folder.id,
                    ProjectLayoutEntry::Folder {
                        id: folder.id,
                        project_ids: children.into_iter().map(|(_, id)| id).collect(),
                    },
                )
            })
            .collect();
        top_level.extend(
            top_projects
                .into_iter()
                .map(|(sort_order, id)| (sort_order, true, id, ProjectLayoutEntry::Project { id })),
        );
        top_level.sort_by_key(|(sort_order, project, id, _)| (*sort_order, *project, *id));
        Ok(top_level
            .into_iter()
            .map(|(_, _, _, entry)| entry)
            .collect())
    }

    pub fn update_project_layout(&self, entries: &[ProjectLayoutEntry]) -> StoreResult<()> {
        self.update_project_layout_for(self.active_profile_id()?, entries)
    }

    pub fn update_project_layout_for(
        &self,
        profile_id: ProfileId,
        entries: &[ProjectLayoutEntry],
    ) -> StoreResult<()> {
        validate_layout(self, profile_id, entries)?;
        let transaction = self.connection().unchecked_transaction()?;
        for (top_order, entry) in entries.iter().enumerate() {
            match entry {
                ProjectLayoutEntry::Project { id } => {
                    transaction.execute(
                        "UPDATE profile_projects SET folder_id = NULL, sort_order = ?1
                         WHERE profile_id = ?2 AND project_id = ?3",
                        params![top_order as i64, profile_id, id],
                    )?;
                }
                ProjectLayoutEntry::Folder { id, project_ids } => {
                    transaction.execute(
                        "UPDATE project_folders SET sort_order = ?1
                         WHERE profile_id = ?2 AND id = ?3",
                        params![top_order as i64, profile_id, id],
                    )?;
                    for (child_order, project_id) in project_ids.iter().enumerate() {
                        transaction.execute(
                            "UPDATE profile_projects SET folder_id = ?1, sort_order = ?2
                             WHERE profile_id = ?3 AND project_id = ?4",
                            params![id, child_order as i64, profile_id, project_id],
                        )?;
                    }
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Restore the folder portion of an imported profile after its canonical projects exist.
    /// The caller can delete the newly imported profile if this transaction is rejected.
    pub fn restore_imported_project_folders(
        &self,
        profile_id: ProfileId,
        folders: &[ImportedProjectFolder],
        memberships: &[ImportedProjectFolderMembership],
    ) -> StoreResult<()> {
        let expected_projects = {
            let mut statement = self.connection().prepare(
                "SELECT pr.path, pr.id
                 FROM projects AS pr
                 JOIN profile_projects AS pp ON pp.project_id = pr.id
                 WHERE pp.profile_id = ?1",
            )?;
            statement
                .query_map([profile_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, ProjectId>(1)?))
                })?
                .collect::<rusqlite::Result<HashMap<_, _>>>()?
        };
        if memberships.len() != expected_projects.len() {
            return invalid_layout(
                "imported folder layout must contain every project exactly once",
            );
        }

        let mut seen_paths = HashSet::new();
        let mut folder_names = HashSet::new();
        for folder in folders {
            let normalized = normalized_folder_name(&folder.name)?;
            let valid_icon = folder.icon.as_deref().is_none_or(|icon| {
                icon.len() <= 80
                    && !icon.is_empty()
                    && !icon.starts_with('-')
                    && !icon.ends_with('-')
                    && icon.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            });
            let valid_color = folder.name_color.as_deref().is_none_or(|color| {
                ["amber", "blue", "rose", "slate", "teal", "violet"].contains(&color)
            });
            if folder.sort_order < 0
                || !folder_names.insert(normalized.to_lowercase())
                || !valid_icon
                || !valid_color
            {
                return invalid_layout(
                    "imported project folders must have valid names, appearance, and order",
                );
            }
        }
        for membership in memberships {
            if membership.sort_order < 0
                || !expected_projects.contains_key(&membership.project_path)
                || !seen_paths.insert(membership.project_path.clone())
            {
                return invalid_layout("imported folder layout contains an invalid project");
            }
            if membership
                .folder_name
                .as_ref()
                .is_some_and(|name| !folder_names.contains(&name.trim().to_lowercase()))
            {
                return invalid_layout("imported folder layout references a missing folder");
            }
        }

        let transaction = self.connection().unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM project_folders WHERE profile_id = ?1",
            [profile_id],
        )?;
        let first_folder_id: ProjectFolderId = transaction.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM project_folders",
            [],
            |row| row.get(0),
        )?;
        let mut folder_ids = HashMap::new();
        for (offset, folder) in folders.iter().enumerate() {
            let folder_id = first_folder_id + offset as i64;
            let name = normalized_folder_name(&folder.name)?;
            transaction.execute(
                "INSERT INTO project_folders (
                    id, profile_id, name, icon, name_color, collapsed, sort_order
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    folder_id,
                    profile_id,
                    name,
                    folder.icon.as_deref(),
                    folder.name_color.as_deref(),
                    folder.collapsed,
                    folder.sort_order
                ],
            )?;
            folder_ids.insert(folder.name.trim().to_lowercase(), folder_id);
        }
        for membership in memberships {
            let folder_id = membership
                .folder_name
                .as_ref()
                .and_then(|name| folder_ids.get(&name.trim().to_lowercase()))
                .copied();
            transaction.execute(
                "UPDATE profile_projects SET folder_id = ?1, sort_order = ?2
                 WHERE profile_id = ?3 AND project_id = ?4",
                params![
                    folder_id,
                    membership.sort_order,
                    profile_id,
                    expected_projects[&membership.project_path]
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn project_folder_for_profile(
        &self,
        profile_id: ProfileId,
        folder_id: ProjectFolderId,
    ) -> StoreResult<Option<ProjectFolder>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT id, name, icon, name_color, collapsed, sort_order
                 FROM project_folders WHERE profile_id = ?1 AND id = ?2",
                params![profile_id, folder_id],
                |row| {
                    Ok(ProjectFolder {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        icon: row.get(2)?,
                        name_color: row.get(3)?,
                        collapsed: row.get(4)?,
                        sort_order: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }
}

fn validate_layout(
    store: &Store,
    profile_id: ProfileId,
    entries: &[ProjectLayoutEntry],
) -> StoreResult<()> {
    let expected_folders = store
        .list_project_folders_for(profile_id)?
        .into_iter()
        .map(|folder| folder.id)
        .collect::<HashSet<_>>();
    let expected_projects = {
        let mut statement = store
            .connection()
            .prepare("SELECT project_id FROM profile_projects WHERE profile_id = ?1")?;
        statement
            .query_map([profile_id], |row| row.get::<_, ProjectId>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?
    };
    let mut seen_folders = HashSet::new();
    let mut seen_projects = HashSet::new();
    for entry in entries {
        match entry {
            ProjectLayoutEntry::Project { id } => {
                if !expected_projects.contains(id) || !seen_projects.insert(*id) {
                    return invalid_layout(
                        "layout contains a missing, foreign, or duplicate project",
                    );
                }
            }
            ProjectLayoutEntry::Folder { id, project_ids } => {
                if !expected_folders.contains(id) || !seen_folders.insert(*id) {
                    return invalid_layout(
                        "layout contains a missing, foreign, or duplicate folder",
                    );
                }
                for project_id in project_ids {
                    if !expected_projects.contains(project_id) || !seen_projects.insert(*project_id)
                    {
                        return invalid_layout(
                            "layout contains a missing, foreign, or duplicate project",
                        );
                    }
                }
            }
        }
    }
    if seen_folders != expected_folders || seen_projects != expected_projects {
        return invalid_layout("layout must contain every folder and project exactly once");
    }
    Ok(())
}

fn normalized_folder_name(name: &str) -> StoreResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return invalid_layout("project folder name cannot be empty");
    }
    if name.chars().count() > 80 || name.chars().any(char::is_control) {
        return invalid_layout("project folder name must be at most 80 visible characters");
    }
    Ok(name.to_owned())
}

fn invalid_layout<T>(message: &str) -> StoreResult<T> {
    Err(StoreError::InvalidReorder(message.to_owned()))
}

fn next_top_level_order(
    transaction: &Transaction<'_>,
    profile_id: ProfileId,
) -> rusqlite::Result<i64> {
    transaction.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1
         FROM (
             SELECT sort_order FROM project_folders WHERE profile_id = ?1
             UNION ALL
             SELECT sort_order FROM profile_projects
             WHERE profile_id = ?1 AND folder_id IS NULL
         )",
        [profile_id],
        |row| row.get(0),
    )
}

pub(crate) fn next_project_top_level_order(
    connection: &Connection,
    profile_id: ProfileId,
) -> StoreResult<i64> {
    Ok(connection.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1
         FROM (
             SELECT sort_order FROM project_folders WHERE profile_id = ?1
             UNION ALL
             SELECT sort_order FROM profile_projects
             WHERE profile_id = ?1 AND folder_id IS NULL
         )",
        [profile_id],
        |row| row.get(0),
    )?)
}
