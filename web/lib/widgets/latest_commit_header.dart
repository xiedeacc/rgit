import 'package:flutter/material.dart';

import '../api/models.dart' as models;

String formatBeijingDateTime(String? value) {
  if (value == null || value.isEmpty) return '';
  final parsed = DateTime.tryParse(value);
  if (parsed == null) return value;
  final beijing = parsed.toUtc().add(const Duration(hours: 8));
  return '${_fourDigits(beijing.year)}-${_twoDigits(beijing.month)}-'
      '${_twoDigits(beijing.day)} ${_twoDigits(beijing.hour)}:'
      '${_twoDigits(beijing.minute)}:${_twoDigits(beijing.second)}';
}

String _fourDigits(int value) => value.toString().padLeft(4, '0');
String _twoDigits(int value) => value.toString().padLeft(2, '0');

class LatestCommitHeader extends StatelessWidget {
  const LatestCommitHeader({
    super.key,
    required this.commit,
    required this.commitCount,
    required this.onCommitTap,
    required this.onHistoryTap,
  });

  final models.CommitInfo? commit;
  final int commitCount;
  final VoidCallback? onCommitTap;
  final VoidCallback onHistoryTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = theme.dividerColor;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
      decoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHighest,
        border: Border.all(color: border),
        borderRadius: const BorderRadius.vertical(top: Radius.circular(6)),
      ),
      child: Row(
        children: [
          Expanded(
            child: commit == null
                ? const Text('No commits yet.')
                : InkWell(
                    onTap: onCommitTap,
                    child: Row(
                      children: [
                        Flexible(
                          child: Text(
                            commit!.authorName.isEmpty
                                ? commit!.title
                                : commit!.authorName,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(
                              fontSize: 16,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ),
                        if (commit!.authorName.isNotEmpty) ...[
                          const SizedBox(width: 8),
                          Flexible(
                            flex: 2,
                            child: Text(
                              commit!.title,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(fontSize: 16),
                            ),
                          ),
                        ],
                        const SizedBox(width: 8),
                        Text(
                          commit!.shortSha,
                          style: theme.textTheme.bodyMedium?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                            fontFamily: 'Roboto Mono',
                            letterSpacing: 0,
                          ),
                        ),
                      ],
                    ),
                  ),
          ),
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (commit != null) ...[
                Text(
                  formatBeijingDateTime(commit!.authoredAt),
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.right,
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                    fontFamily: 'Roboto Mono',
                    letterSpacing: 0,
                  ),
                ),
                const SizedBox(width: 16),
              ],
              InkWell(
                onTap: onHistoryTap,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Icon(Icons.history, size: 18),
                    const SizedBox(width: 6),
                    Text(
                      '$commitCount Commits',
                      style: const TextStyle(
                        fontSize: 16,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
