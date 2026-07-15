import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../api/models.dart' as models;

enum ProjectTab { code, commits, branches, tags, settings }

/// Repo page header: "ns / project" title, badges, and the tab bar
/// (Code / Commits / Branches / Tags / Settings).
class ProjectHeader extends StatelessWidget {
  const ProjectHeader({
    super.key,
    required this.project,
    required this.selected,
  });

  final models.Project project;
  final ProjectTab selected;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final full = project.fullPath;
    final ns = project.namespacePath;
    final defaultRef = project.defaultBranch ?? 'main';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(
              project.visibility == models.Visibility.public
                  ? Icons.book_outlined
                  : Icons.lock_outline,
              size: 18,
            ),
            const SizedBox(width: 8),
            InkWell(
              onTap: () => context.go('/$ns'),
              child: Text(ns,
                  style: TextStyle(
                      fontSize: 18, color: theme.colorScheme.primary)),
            ),
            const Text(' / ', style: TextStyle(fontSize: 18)),
            InkWell(
              onTap: () => context.go('/$full'),
              child: Text(project.path,
                  style: TextStyle(
                      fontSize: 18,
                      fontWeight: FontWeight.w700,
                      color: theme.colorScheme.primary)),
            ),
            const SizedBox(width: 10),
            _Badge(label: models.Visibility.label(project.visibility)),
            if (project.archived) ...const [
              SizedBox(width: 6),
              _Badge(label: 'Archived'),
            ],
          ],
        ),
        if (project.description.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text(project.description,
                style: theme.textTheme.bodyMedium),
          ),
        const SizedBox(height: 12),
        _TabBarRow(full: full, defaultRef: defaultRef, selected: selected),
        const Divider(height: 1),
        const SizedBox(height: 16),
      ],
    );
  }
}

class _TabBarRow extends StatelessWidget {
  const _TabBarRow({
    required this.full,
    required this.defaultRef,
    required this.selected,
  });

  final String full;
  final String defaultRef;
  final ProjectTab selected;

  @override
  Widget build(BuildContext context) {
    final tabs = <(ProjectTab, String, IconData, String)>[
      (ProjectTab.code, 'Code', Icons.code, '/$full'),
      (
        ProjectTab.commits,
        'Commits',
        Icons.history,
        '/$full/commits/$defaultRef'
      ),
      (ProjectTab.branches, 'Branches', Icons.call_split, '/$full/branches'),
      (ProjectTab.tags, 'Tags', Icons.sell_outlined, '/$full/tags'),
      (
        ProjectTab.settings,
        'Settings',
        Icons.settings_outlined,
        '/$full/settings'
      ),
    ];
    final theme = Theme.of(context);
    return Row(
      children: [
        for (final (tab, label, icon, route) in tabs)
          InkWell(
            onTap: () => context.go(route),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
              decoration: BoxDecoration(
                border: Border(
                  bottom: BorderSide(
                    width: 2,
                    color: tab == selected
                        ? theme.colorScheme.primary
                        : Colors.transparent,
                  ),
                ),
              ),
              child: Row(
                children: [
                  Icon(icon, size: 16),
                  const SizedBox(width: 6),
                  Text(
                    label,
                    style: TextStyle(
                      fontWeight: tab == selected
                          ? FontWeight.w600
                          : FontWeight.w400,
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _Badge extends StatelessWidget {
  const _Badge({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1),
        decoration: BoxDecoration(
          border: Border.all(color: Theme.of(context).dividerColor),
          borderRadius: BorderRadius.circular(999),
        ),
        child: Text(label, style: const TextStyle(fontSize: 11)),
      );
}
