import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/top_nav.dart';

/// Group settings: rename, members, delete (route: /:ns/settings).
class GroupSettingsPage extends StatefulWidget {
  const GroupSettingsPage({super.key, required this.ns});

  final String ns;

  @override
  State<GroupSettingsPage> createState() => _GroupSettingsPageState();
}

class _GroupSettingsPageState extends State<GroupSettingsPage> {
  models.Namespace? _group;
  List<models.Member> _members = const [];
  Object? _error;
  bool _loading = true;

  final _name = TextEditingController();

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    final api = context.read<ApiClient>();
    try {
      final group = await api.getGroup(widget.ns);
      if (!mounted) return;
      setState(() {
        _group = group;
        // Backend has no group-member listing endpoint yet (DESIGN.md §10);
        // membership changes still work via add/remove below.
        _members = const <models.Member>[];
        _name.text = group.name;
      });
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _snack(String message) {
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _rename() async {
    // Backend has no PATCH /groups/{id} yet.
    _snack('Renaming groups is not implemented yet.');
  }

  Future<void> _addMember() async {
    final userId = TextEditingController();
    var level = models.AccessLevel.developer;
    final ok = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (dialogContext, setState) => AlertDialog(
          title: const Text('Add group member'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: userId,
                decoration: const InputDecoration(labelText: 'User ID'),
                keyboardType: TextInputType.number,
              ),
              const SizedBox(height: 10),
              DropdownButtonFormField<int>(
                initialValue: level,
                decoration:
                    const InputDecoration(labelText: 'Access level'),
                items: [
                  for (final e in models.AccessLevel.labels.entries)
                    DropdownMenuItem(value: e.key, child: Text(e.value)),
                ],
                onChanged: (v) => setState(() => level = v ?? level),
              ),
            ],
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(dialogContext, false),
                child: const Text('Cancel')),
            FilledButton(
                onPressed: () => Navigator.pop(dialogContext, true),
                child: const Text('Add')),
          ],
        ),
      ),
    );
    if (ok == true && mounted) {
      final id = int.tryParse(userId.text);
      if (id == null) {
        _snack('Enter a numeric user ID.');
      } else {
        try {
          await context
              .read<ApiClient>()
              .addGroupMember(_group!.id, userId: id, accessLevel: level);
          await _load();
          if (mounted) _snack('Member added.');
        } catch (e) {
          if (mounted) _snack('$e');
        }
      }
    }
    userId.dispose();
  }

  Future<void> _removeMember(models.Member m) async {
    try {
      await context
          .read<ApiClient>()
          .removeGroupMember(_group!.id, m.userId);
      await _load();
      if (mounted) _snack('Member removed.');
    } catch (e) {
      if (mounted) _snack('$e');
    }
  }

  Future<void> _delete() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Delete group'),
        content: Text(
            'Delete group "${widget.ns}"? All projects must be removed '
            'first. This cannot be undone.'),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dialogContext, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(dialogContext, true),
              child: const Text('Delete')),
        ],
      ),
    );
    if (ok == true && mounted) {
      try {
        await context.read<ApiClient>().deleteGroup(_group!.id);
        if (mounted) context.go('/');
      } catch (e) {
        if (mounted) _snack('$e');
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    if (_loading && _group == null) {
      return const PageShell(child: Loading());
    }
    if (_error != null) {
      return PageShell(
          maxWidth: 720,
          child: ErrorView(error: _error!, onRetry: _load));
    }
    return PageShell(
      maxWidth: 720,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text('Group settings — ${widget.ns}',
              style: const TextStyle(
                  fontSize: 22, fontWeight: FontWeight.w600)),
          const SizedBox(height: 16),
          TextField(
            controller: _name,
            decoration: const InputDecoration(labelText: 'Group name'),
          ),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child:
                FilledButton(onPressed: _rename, child: const Text('Save')),
          ),
          const SizedBox(height: 24),
          const Divider(),
          const SizedBox(height: 12),
          Row(
            children: [
              const Text('Members',
                  style: TextStyle(
                      fontSize: 16, fontWeight: FontWeight.w600)),
              const Spacer(),
              OutlinedButton.icon(
                icon: const Icon(Icons.person_add_outlined, size: 16),
                label: const Text('Add member'),
                onPressed: _addMember,
              ),
            ],
          ),
          const SizedBox(height: 8),
          for (final m in _members)
            ListTile(
              dense: true,
              leading: const Icon(Icons.person_outline),
              title: Text(m.username ?? 'User #${m.userId}'),
              subtitle: Text(models.AccessLevel.label(m.accessLevel)),
              trailing: IconButton(
                tooltip: 'Remove member',
                icon: const Icon(Icons.delete_outline, size: 18),
                onPressed: () => _removeMember(m),
              ),
            ),
          if (_members.isEmpty)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Text('No members.'),
            ),
          const SizedBox(height: 24),
          const Divider(),
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const Text('Delete group',
                        style: TextStyle(fontWeight: FontWeight.w600)),
                    Text('Removes the group namespace.',
                        style: theme.textTheme.bodySmall),
                  ],
                ),
              ),
              OutlinedButton(
                style: OutlinedButton.styleFrom(
                  foregroundColor: theme.colorScheme.error,
                  side: BorderSide(color: theme.colorScheme.error),
                ),
                onPressed: _delete,
                child: const Text('Delete'),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
