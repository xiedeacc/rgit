import 'package:flutter/material.dart';
import 'package:web/web.dart' as web;

import '../api/models.dart' as models;

/// One project row in a project list (Explore, namespace home, admin).
class ProjectCard extends StatelessWidget {
  const ProjectCard({super.key, required this.project});

  final models.Project project;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      child: ListTile(
        leading: Icon(
          project.visibility == models.Visibility.public
              ? Icons.book_outlined
              : Icons.lock_outline,
          size: 20,
        ),
        title: Row(
          children: [
            Flexible(
              child: Text(
                project.fullPath,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color: theme.colorScheme.primary,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
            const SizedBox(width: 8),
            _Chip(label: models.Visibility.label(project.visibility)),
            if (project.archived) ...const [
              SizedBox(width: 6),
              _Chip(label: 'Archived'),
            ],
            if (project.forkedFromProjectId != null) ...const [
              SizedBox(width: 6),
              _Chip(label: 'Fork'),
            ],
          ],
        ),
        subtitle: project.description.isEmpty
            ? null
            : Text(project.description,
                maxLines: 2, overflow: TextOverflow.ellipsis),
        onTap: () => _openInNewTab('/${project.fullPath}'),
      ),
    );
  }
}

/// Opens [path] in a new browser tab so the list page stays where it is.
void _openInNewTab(String path) {
  // Relative paths resolve against the current origin (path URL strategy).
  web.window.open(path, '_blank');
}

class _Chip extends StatelessWidget {
  const _Chip({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    final border = Theme.of(context).dividerColor;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1),
      decoration: BoxDecoration(
        border: Border.all(color: border),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(label, style: const TextStyle(fontSize: 11)),
    );
  }
}
