import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../api/models.dart';

/// GitHub-style file listing for a tree (directories first).
class FileTreeList extends StatelessWidget {
  const FileTreeList({
    super.key,
    required this.entries,
    required this.projectFullPath,
    required this.ref,
    this.hasHeader = false,
  });

  final List<TreeEntry> entries;
  final String projectFullPath;
  final String ref;
  final bool hasHeader;

  @override
  Widget build(BuildContext context) {
    final sorted = [...entries]
      ..sort((a, b) {
        if (a.isDir != b.isDir) return a.isDir ? -1 : 1;
        return a.name.toLowerCase().compareTo(b.name.toLowerCase());
      });
    final border = Theme.of(context).dividerColor;
    return Container(
      decoration: BoxDecoration(
        border: hasHeader
            ? Border(
                left: BorderSide(color: border),
                right: BorderSide(color: border),
                bottom: BorderSide(color: border),
              )
            : Border.all(color: border),
        borderRadius: hasHeader
            ? const BorderRadius.vertical(bottom: Radius.circular(6))
            : BorderRadius.circular(6),
      ),
      child: Column(
        children: [
          for (var i = 0; i < sorted.length; i++) ...[
            if (i > 0) Divider(height: 1, color: border),
            _FileRow(
              entry: sorted[i],
              onTap: () {
                final e = sorted[i];
                final kind = e.isDir ? 'tree' : 'blob';
                context.go('/$projectFullPath/$kind/$ref/${e.path}');
              },
            ),
          ],
          if (sorted.isEmpty)
            const Padding(
              padding: EdgeInsets.all(24),
              child: Text('This directory is empty.'),
            ),
        ],
      ),
    );
  }
}

class _FileRow extends StatelessWidget {
  const _FileRow({required this.entry, required this.onTap});

  final TreeEntry entry;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 9),
        child: Row(
          children: [
            Icon(
              entry.isDir ? Icons.folder : Icons.insert_drive_file_outlined,
              size: 18,
              color: entry.isDir
                  ? theme.colorScheme.primary
                  : theme.colorScheme.onSurface.withValues(alpha: 0.6),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                entry.name,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 16, height: 1.5),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
