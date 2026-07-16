-- Keep membership rows attached to the intended namespace type and role set,
-- even if a future internal caller bypasses the HTTP validators.

CREATE TRIGGER group_members_require_group_insert
BEFORE INSERT ON group_members
WHEN NOT EXISTS (
    SELECT 1 FROM namespaces WHERE id = NEW.namespace_id AND kind = 'group'
)
BEGIN
    SELECT RAISE(ABORT, 'group membership requires group namespace');
END;

CREATE TRIGGER group_members_require_group_update
BEFORE UPDATE OF namespace_id ON group_members
WHEN NOT EXISTS (
    SELECT 1 FROM namespaces WHERE id = NEW.namespace_id AND kind = 'group'
)
BEGIN
    SELECT RAISE(ABORT, 'group membership requires group namespace');
END;

CREATE TRIGGER group_members_access_level_insert
BEFORE INSERT ON group_members
WHEN NEW.access_level NOT IN (10, 20, 30, 40, 50)
BEGIN
    SELECT RAISE(ABORT, 'invalid group access level');
END;

CREATE TRIGGER group_members_access_level_update
BEFORE UPDATE OF access_level ON group_members
WHEN NEW.access_level NOT IN (10, 20, 30, 40, 50)
BEGIN
    SELECT RAISE(ABORT, 'invalid group access level');
END;

CREATE TRIGGER project_members_access_level_insert
BEFORE INSERT ON project_members
WHEN NEW.access_level NOT IN (10, 20, 30, 40, 50)
BEGIN
    SELECT RAISE(ABORT, 'invalid project access level');
END;

CREATE TRIGGER project_members_access_level_update
BEFORE UPDATE OF access_level ON project_members
WHEN NEW.access_level NOT IN (10, 20, 30, 40, 50)
BEGIN
    SELECT RAISE(ABORT, 'invalid project access level');
END;
