-- Remove rows left by external/manual SQLite edits that did not enable
-- PRAGMA foreign_keys. Normal application deletes already cascade.

DELETE FROM group_members
WHERE NOT EXISTS (
    SELECT 1 FROM namespaces WHERE namespaces.id = group_members.namespace_id
);

DELETE FROM project_members
WHERE NOT EXISTS (
    SELECT 1 FROM projects WHERE projects.id = project_members.project_id
);

DELETE FROM project_lfs_objects
WHERE NOT EXISTS (
    SELECT 1 FROM projects WHERE projects.id = project_lfs_objects.project_id
)
OR NOT EXISTS (
    SELECT 1 FROM lfs_objects WHERE lfs_objects.id = project_lfs_objects.lfs_object_id
);
