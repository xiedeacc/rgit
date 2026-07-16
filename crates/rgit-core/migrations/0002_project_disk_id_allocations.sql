-- Monotonic disk id allocator. Keeping allocations after project deletion
-- prevents hashed storage paths from ever being reused for another project.
CREATE TABLE project_disk_id_allocations (
    id INTEGER PRIMARY KEY
);
