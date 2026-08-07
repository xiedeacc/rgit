import 'package:flutter/material.dart';

import '../theme.dart';

class DiffFile {
  const DiffFile({
    required this.path,
    required this.patch,
    required this.added,
    required this.removed,
  });

  final String path;
  final String patch;
  final int added;
  final int removed;
}

List<DiffFile> parseUnifiedDiff(String diff) {
  final lines = diff.replaceAll('\r\n', '\n').split('\n');
  final files = <DiffFile>[];
  var current = <String>[];

  void flush() {
    if (current.isEmpty) return;
    files.add(_parseFile(current));
    current = <String>[];
  }

  for (final line in lines) {
    if (line.startsWith('diff --git ') && current.isNotEmpty) {
      flush();
    }
    current.add(line);
  }
  flush();
  return files;
}

DiffFile _parseFile(List<String> lines) {
  final path = _diffPath(lines);
  var added = 0;
  var removed = 0;
  for (final line in lines) {
    if (line.startsWith('+++') || line.startsWith('---')) continue;
    if (line.startsWith('+')) added++;
    if (line.startsWith('-')) removed++;
  }
  return DiffFile(
    path: path,
    patch: lines.join('\n').trimRight(),
    added: added,
    removed: removed,
  );
}

String _diffPath(List<String> lines) {
  for (final prefix in const ['+++ ', '--- ']) {
    for (final line in lines) {
      if (!line.startsWith(prefix)) continue;
      final value = line.substring(prefix.length).trim();
      if (value == '/dev/null') continue;
      return _stripDiffPrefix(value);
    }
  }
  final header = lines.firstWhere(
    (line) => line.startsWith('diff --git '),
    orElse: () => '',
  );
  final parts = header.split(' ');
  if (parts.length >= 4) return _stripDiffPrefix(parts[3]);
  return 'Patch';
}

String _stripDiffPrefix(String value) {
  if (value.startsWith('a/') || value.startsWith('b/')) {
    return value.substring(2);
  }
  return value;
}

class CommitDiffView extends StatelessWidget {
  const CommitDiffView({super.key, required this.diff});

  final String diff;

  @override
  Widget build(BuildContext context) {
    final files = parseUnifiedDiff(diff);
    if (files.isEmpty) return const Text('No diff available.');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _DiffSummary(files: files),
        const SizedBox(height: 10),
        for (var i = 0; i < files.length; i++) ...[
          if (i > 0) const SizedBox(height: 12),
          _DiffFileCard(file: files[i]),
        ],
      ],
    );
  }
}

class _DiffSummary extends StatelessWidget {
  const _DiffSummary({required this.files});

  final List<DiffFile> files;

  @override
  Widget build(BuildContext context) {
    final added = files.fold<int>(0, (sum, file) => sum + file.added);
    final removed = files.fold<int>(0, (sum, file) => sum + file.removed);
    final theme = Theme.of(context);
    return Text(
      '${files.length} changed ${files.length == 1 ? 'file' : 'files'} '
      'with $added additions and $removed deletions',
      style: theme.textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600),
    );
  }
}

class _DiffFileCard extends StatelessWidget {
  const _DiffFileCard({required this.file});

  final DiffFile file;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = theme.dividerColor;
    return Container(
      width: double.infinity,
      decoration: BoxDecoration(
        border: Border.all(color: border),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: double.infinity,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            decoration: BoxDecoration(
              color: theme.colorScheme.surfaceContainerHighest,
              border: Border(bottom: BorderSide(color: border)),
            ),
            child: Row(
              children: [
                const Icon(Icons.insert_drive_file_outlined, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    file.path,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                ),
                const SizedBox(width: 12),
                Text(
                  '+${file.added}',
                  style: RgitTheme.mono.copyWith(
                    color: const Color(0xff1a7f37),
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  '-${file.removed}',
                  style: RgitTheme.mono.copyWith(
                    color: const Color(0xffcf222e),
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ],
            ),
          ),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: SelectableText(file.patch, style: RgitTheme.mono),
            ),
          ),
        ],
      ),
    );
  }
}
