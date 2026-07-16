-- Keep administrative control recoverable even when concurrent requests or
-- maintenance code writes directly to SQLite.

CREATE TRIGGER protect_last_active_admin_update
BEFORE UPDATE OF is_admin, state ON users
WHEN OLD.is_admin = 1
 AND OLD.state = 'active'
 AND NOT (NEW.is_admin = 1 AND NEW.state = 'active')
 AND NOT EXISTS (
     SELECT 1 FROM users
     WHERE id <> OLD.id AND is_admin = 1 AND state = 'active'
 )
BEGIN
    SELECT RAISE(ABORT, 'instance must retain at least one active admin');
END;

CREATE TRIGGER protect_last_active_admin_delete
BEFORE DELETE ON users
WHEN OLD.is_admin = 1
 AND OLD.state = 'active'
 AND NOT EXISTS (
     SELECT 1 FROM users
     WHERE id <> OLD.id AND is_admin = 1 AND state = 'active'
 )
BEGIN
    SELECT RAISE(ABORT, 'instance must retain at least one active admin');
END;

CREATE TRIGGER protect_last_group_owner_update
BEFORE UPDATE OF access_level ON group_members
WHEN OLD.access_level = 50
 AND NEW.access_level <> 50
 AND EXISTS (SELECT 1 FROM namespaces WHERE id = OLD.namespace_id)
 AND NOT EXISTS (
     SELECT 1 FROM group_members
     WHERE namespace_id = OLD.namespace_id
       AND user_id <> OLD.user_id
       AND access_level = 50
 )
BEGIN
    SELECT RAISE(ABORT, 'group must retain at least one owner');
END;

CREATE TRIGGER protect_last_group_owner_delete
BEFORE DELETE ON group_members
WHEN OLD.access_level = 50
 AND EXISTS (SELECT 1 FROM namespaces WHERE id = OLD.namespace_id)
 AND NOT EXISTS (
     SELECT 1 FROM group_members
     WHERE namespace_id = OLD.namespace_id
       AND user_id <> OLD.user_id
       AND access_level = 50
 )
BEGIN
    SELECT RAISE(ABORT, 'group must retain at least one owner');
END;
