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
      child: LayoutBuilder(
        builder: (context, constraints) {
          final showCommit = constraints.maxWidth >= 560;
          final showTime = constraints.maxWidth >= 760;
          final latestCommit = entry.latestCommit;
          return Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 9),
            child: Row(
              children: [
                Expanded(
                  flex: showCommit ? 5 : 1,
                  child: Row(
                    children: [
                      Icon(
                        entry.isDir
                            ? Icons.folder
                            : Icons.insert_drive_file_outlined,
                        size: 18,
                        color: entry.isDir
                            ? theme.colorScheme.primary
                            : theme.colorScheme.onSurface.withValues(
                                alpha: 0.6,
                              ),
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
                if (showCommit) ...[
                  const SizedBox(width: 16),
                  Expanded(
                    flex: 4,
                    child: Text(
                      latestCommit?.title ?? '',
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: theme.colorScheme.onSurfaceVariant,
                        fontSize: 14,
                        height: 1.45,
                      ),
                    ),
                  ),
                ],
                if (showTime) ...[
                  const SizedBox(width: 16),
                  SizedBox(
                    width: 112,
                    child: Text(
                      _relativeTime(latestCommit?.authoredAt),
                      overflow: TextOverflow.ellipsis,
                      textAlign: TextAlign.end,
                      style: TextStyle(
                        color: theme.colorScheme.onSurfaceVariant,
                        fontSize: 14,
                        height: 1.45,
                      ),
                    ),
                  ),
                ],
              ],
            ),
          );
        },
      ),
    );
  }

  String _relativeTime(String? isoTime) {
    final parsed = isoTime == null ? null : DateTime.tryParse(isoTime);
    if (parsed == null) return '';
    final diff = DateTime.now().difference(parsed.toLocal());
    if (diff.isNegative || diff.inSeconds < 60) return 'now';
    if (diff.inMinutes < 60) return _unit(diff.inMinutes, 'minute');
    if (diff.inHours < 24) return _unit(diff.inHours, 'hour');
    if (diff.inDays < 30) return _unit(diff.inDays, 'day');
    if (diff.inDays < 365) return _unit(diff.inDays ~/ 30, 'month');
    return _unit(diff.inDays ~/ 365, 'year');
  }

  String _unit(int value, String unit) =>
      '$value $unit${value == 1 ? '' : 's'} ago';
}
