import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/app_dropdown.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/project_scaffold.dart';
import '../widgets/project_tabs.dart';
import '../widgets/top_nav.dart';

/// Project settings: general / members / danger zone
/// (route: /:ns/:proj/settings).
class ProjectSettingsPage extends StatefulWidget {
  const ProjectSettingsPage({super.key, required this.ns, required this.proj});

  final String ns;
  final String proj;

  @override
  State<ProjectSettingsPage> createState() => _ProjectSettingsPageState();
}

class _ProjectSettingsPageState extends State<ProjectSettingsPage> {
  models.Project? _project;
  List<models.Member> _members = const [];
  Object? _error;
  bool _loading = true;

  final _name = TextEditingController();
  final _description = TextEditingController();
  final _defaultBranch = TextEditingController();
  int _visibility = models.Visibility.private;

  String get _fullPath => '${widget.ns}/${widget.proj}';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    _defaultBranch.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    final api = context.read<ApiClient>();
    try {
      final project = await api.getProject(_fullPath);
      final members = await api
          .listProjectMembers(project.id)
          .catchError((Object _) => const <models.Member>[]);
      if (!mounted) return;
      setState(() {
        _project = project;
        _members = members;
        _name.text = project.name;
        _description.text = project.description;
        _defaultBranch.text = project.defaultBranch ?? '';
        _visibility = project.visibility;
      });
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _snack(String message) {
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _run(Future<void> Function() action, String success) async {
    try {
      await action();
      if (mounted) _snack(success);
    } catch (e) {
      if (mounted) _snack('$e');
    }
  }

  Future<void> _saveGeneral() => _run(() async {
    final updated = await context
        .read<ApiClient>()
        .updateProject(_project!.id, {
          'name': _name.text,
          'description': _description.text,
          'visibility': _visibility,
          if (_defaultBranch.text.isNotEmpty)
            'default_branch': _defaultBranch.text,
        });
    if (mounted) setState(() => _project = updated);
  }, 'Settings saved.');

  Future<void> _addMember() async {
    final userId = TextEditingController();
    var level = models.AccessLevel.developer;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (dialogContext, setState) => AlertDialog(
          title: const Text('Add member'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: userId,
                decoration: const InputDecoration(labelText: 'User ID'),
                keyboardType: TextInputType.number,
              ),
              const SizedBox(height: 10),
              AppDropdown<int>(
                label: 'Access level',
                value: level,
                options: [
                  for (final e in models.AccessLevel.labels.entries)
                    AppDropdownOption(value: e.key, label: e.value),
                ],
                onChanged: (v) => setState(() => level = v),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(dialogContext, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(dialogContext, true),
              child: const Text('Add'),
            ),
          ],
        ),
      ),
    );
    if (confirmed == true && mounted) {
      final id = int.tryParse(userId.text);
      if (id == null) {
        _snack('Enter a numeric user ID.');
      } else {
        await _run(() async {
          await context.read<ApiClient>().addProjectMember(
            _project!.id,
            userId: id,
            accessLevel: level,
          );
          await _load();
        }, 'Member added.');
      }
    }
    userId.dispose();
  }

  Future<bool> _confirm(String title, String body) async =>
      await showDialog<bool>(
        context: context,
        builder: (dialogContext) => AlertDialog(
          title: Text(title),
          content: Text(body),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(dialogContext, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(dialogContext, true),
              child: const Text('Confirm'),
            ),
          ],
        ),
      ) ==
      true;

  @override
  Widget build(BuildContext context) {
    if (_loading && _project == null) {
      return const PageShell(child: Loading());
    }
    if (_error != null) {
      return PageShell(
        child: ErrorView(error: _error!, onRetry: _load),
      );
    }
    final project = _project!;
    final api = context.read<ApiClient>();
    return ProjectScaffold(
      project: project,
      selected: ProjectTab.settings,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // --- General -----------------------------------------------------
          _Section(
            title: 'General',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                TextField(
                  controller: _name,
                  decoration: const InputDecoration(labelText: 'Project name'),
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: _description,
                  decoration: const InputDecoration(labelText: 'Description'),
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: _defaultBranch,
                  decoration: const InputDecoration(
                    labelText: 'Default branch',
                  ),
                ),
                const SizedBox(height: 10),
                AppDropdown<int>(
                  label: 'Visibility',
                  value: _visibility,
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
                  onChanged: (v) => setState(() => _visibility = v),
                ),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: _saveGeneral,
                  child: const Text('Save changes'),
                ),
              ],
            ),
          ),

          // --- Members -----------------------------------------------------
          _Section(
            title: 'Members',
            action: OutlinedButton.icon(
              icon: const Icon(Icons.person_add_outlined, size: 16),
              label: const Text('Add member'),
              onPressed: _addMember,
            ),
            child: Column(
              children: [
                for (final m in _members)
                  ListTile(
                    dense: true,
                    leading: const Icon(Icons.person_outline),
                    title: Text(m.username ?? 'User #${m.userId}'),
                    subtitle: Text(models.AccessLevel.label(m.accessLevel)),
                    trailing: IconButton(
                      tooltip: 'Remove member',
                      icon: const Icon(Icons.delete_outline, size: 18),
                      onPressed: () async {
                        if (await _confirm(
                          'Remove member',
                          'Remove this member from the project?',
                        )) {
                          await _run(() async {
                            await api.removeProjectMember(project.id, m.userId);
                            await _load();
                          }, 'Member removed.');
                        }
                      },
                    ),
                  ),
                if (_members.isEmpty)
                  const Padding(
                    padding: EdgeInsets.all(16),
                    child: Text('No direct members.'),
                  ),
              ],
            ),
          ),

          // --- Danger zone ---------------------------------------------------
          _Section(
            title: 'Danger zone',
            danger: true,
            child: Column(
              children: [
                _DangerRow(
                  title: project.archived
                      ? 'Unarchive project'
                      : 'Archive project',
                  subtitle: project.archived
                      ? 'Restore write access to this project.'
                      : 'Mark read-only; pushes and settings changes are '
                            'rejected.',
                  buttonLabel: project.archived ? 'Unarchive' : 'Archive',
                  onPressed: () async {
                    if (await _confirm(
                      project.archived ? 'Unarchive' : 'Archive',
                      'Are you sure?',
                    )) {
                      await _run(() async {
                        final updated = project.archived
                            ? await api.unarchiveProject(project.id)
                            : await api.archiveProject(project.id);
                        if (mounted) setState(() => _project = updated);
                      }, 'Done.');
                    }
                  },
                ),
                const Divider(),
                _DangerRow(
                  title: 'Transfer project',
                  subtitle: 'Move this project to another namespace.',
                  buttonLabel: 'Transfer',
                  onPressed: () async {
                    final nsId = TextEditingController();
                    final ok = await showDialog<bool>(
                      context: context,
                      builder: (dialogContext) => AlertDialog(
                        title: const Text('Transfer project'),
                        content: TextField(
                          controller: nsId,
                          decoration: const InputDecoration(
                            labelText: 'Target namespace ID',
                          ),
                          keyboardType: TextInputType.number,
                        ),
                        actions: [
                          TextButton(
                            onPressed: () =>
                                Navigator.pop(dialogContext, false),
                            child: const Text('Cancel'),
                          ),
                          FilledButton(
                            onPressed: () => Navigator.pop(dialogContext, true),
                            child: const Text('Transfer'),
                          ),
                        ],
                      ),
                    );
                    final id = int.tryParse(nsId.text);
                    if (ok == true && id != null && mounted) {
                      await _run(() async {
                        final updated = await api.transferProject(
                          project.id,
                          id,
                        );
                        if (mounted && context.mounted) {
                          context.go('/${updated.fullPath}/settings');
                        }
                      }, 'Project transferred.');
                    }
                    nsId.dispose();
                  },
                ),
                const Divider(),
                _DangerRow(
                  title: 'Delete project',
                  subtitle:
                      'Permanently remove this project and its repository.',
                  buttonLabel: 'Delete',
                  onPressed: () async {
                    if (await _confirm(
                      'Delete project',
                      'This cannot be undone. Delete "${project.fullPath}"?',
                    )) {
                      await _run(() async {
                        await api.deleteProject(project.id);
                        if (mounted && context.mounted) context.go('/');
                      }, 'Project deleted.');
                    }
                  },
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({
    required this.title,
    required this.child,
    this.action,
    this.danger = false,
  });

  final String title;
  final Widget child;
  final Widget? action;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = danger ? theme.colorScheme.error : theme.dividerColor;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.only(bottom: 20),
      decoration: BoxDecoration(
        border: Border.all(color: border),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: double.infinity,
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
            decoration: BoxDecoration(
              color: theme.colorScheme.surfaceContainerHighest,
              border: Border(bottom: BorderSide(color: border)),
            ),
            child: Row(
              children: [
                Text(
                  title,
                  style: TextStyle(
                    fontWeight: FontWeight.w600,
                    color: danger ? theme.colorScheme.error : null,
                  ),
                ),
                const Spacer(),
                ?action,
              ],
            ),
          ),
          Padding(padding: const EdgeInsets.all(16), child: child),
        ],
      ),
    );
  }
}

class _DangerRow extends StatelessWidget {
  const _DangerRow({
    required this.title,
    required this.subtitle,
    required this.buttonLabel,
    required this.onPressed,
  });

  final String title;
  final String subtitle;
  final String buttonLabel;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(
      children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(title, style: const TextStyle(fontWeight: FontWeight.w600)),
              Text(subtitle, style: theme.textTheme.bodySmall),
            ],
          ),
        ),
        OutlinedButton(
          style: OutlinedButton.styleFrom(
            foregroundColor: theme.colorScheme.error,
            side: BorderSide(color: theme.colorScheme.error),
          ),
          onPressed: onPressed,
          child: Text(buttonLabel),
        ),
      ],
    );
  }
}
