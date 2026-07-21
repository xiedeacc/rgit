import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/app_dropdown.dart';

/// "New project" dialog: name/path/visibility/description → POST /projects.
Future<void> showNewProjectDialog(BuildContext context) async {
  final api = context.read<ApiClient>();
  final name = TextEditingController();
  final path = TextEditingController();
  final description = TextEditingController();
  var visibility = models.Visibility.private;
  String? error;
  var busy = false;

  await showDialog<void>(
    context: context,
    builder: (dialogContext) => StatefulBuilder(
      builder: (dialogContext, setState) {
        Future<void> submit() async {
          setState(() {
            busy = true;
            error = null;
          });
          try {
            final p = path.text.isNotEmpty ? path.text : name.text;
            final project = await api.createProject(
              name: name.text,
              path: p,
              visibility: visibility,
              description: description.text.isEmpty ? null : description.text,
            );
            if (dialogContext.mounted) {
              Navigator.of(dialogContext).pop();
              dialogContext.go('/${project.fullPath}');
            }
          } catch (e) {
            setState(() {
              error = '$e';
              busy = false;
            });
          }
        }

        return AlertDialog(
          title: const Text('New project'),
          content: SizedBox(
            width: 420,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (error != null) ...[
                  Text(
                    error!,
                    style: TextStyle(
                      color: Theme.of(dialogContext).colorScheme.error,
                    ),
                  ),
                  const SizedBox(height: 10),
                ],
                TextField(
                  controller: name,
                  autofocus: true,
                  decoration: const InputDecoration(labelText: 'Name'),
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: path,
                  decoration: const InputDecoration(
                    labelText: 'Path (defaults to name)',
                  ),
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: description,
                  decoration: const InputDecoration(labelText: 'Description'),
                ),
                const SizedBox(height: 10),
                AppDropdown<int>(
                  label: 'Visibility',
                  value: visibility,
                  options: const [
                    AppDropdownOption(
                      value: models.Visibility.private,
                      label: 'Private',
                    ),
                    AppDropdownOption(
                      value: models.Visibility.internal,
                      label: 'Internal',
                    ),
                    AppDropdownOption(
                      value: models.Visibility.public,
                      label: 'Public',
                    ),
                  ],
                  onChanged: (v) => setState(() => visibility = v),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: busy ? null : submit,
              child: Text(busy ? 'Creating…' : 'Create project'),
            ),
          ],
        );
      },
    ),
  );
  name.dispose();
  path.dispose();
  description.dispose();
}
