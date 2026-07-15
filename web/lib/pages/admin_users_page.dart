import 'dart:math';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../theme.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/top_nav.dart';
import 'admin_dashboard_page.dart' show AdminTabs;

/// Admin user management (route: /admin/users).
class AdminUsersPage extends StatefulWidget {
  const AdminUsersPage({super.key});

  @override
  State<AdminUsersPage> createState() => _AdminUsersPageState();
}

class _AdminUsersPageState extends State<AdminUsersPage> {
  static const _passwordChars =
      'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789';

  String _generatePassword() {
    final rng = Random.secure();
    return List.generate(
        16, (_) => _passwordChars[rng.nextInt(_passwordChars.length)]).join();
  }

  models.Paged<models.User>? _page;
  Object? _error;
  bool _loading = true;
  final int _pageNo = 1;
  static const int _perPage = 20;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final page = await context
          .read<ApiClient>()
          .adminListUsers(page: _pageNo, perPage: _perPage);
      if (mounted) setState(() => _page = page);
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

  Future<void> _createUser() async {
    final username = TextEditingController();
    final email = TextEditingController();
    final name = TextEditingController();
    var isAdmin = false;
    final ok = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (dialogContext, setState) => AlertDialog(
          title: const Text('New user'),
          content: SizedBox(
            width: 380,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                    controller: username,
                    decoration:
                        const InputDecoration(labelText: 'Username')),
                const SizedBox(height: 10),
                TextField(
                    controller: email,
                    decoration: const InputDecoration(labelText: 'Email')),
                const SizedBox(height: 10),
                TextField(
                    controller: name,
                    decoration: const InputDecoration(labelText: 'Name')),
                const SizedBox(height: 10),
                CheckboxListTile(
                  value: isAdmin,
                  onChanged: (v) => setState(() => isAdmin = v ?? false),
                  title: const Text('Administrator'),
                  contentPadding: EdgeInsets.zero,
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(dialogContext, false),
                child: const Text('Cancel')),
            FilledButton(
                onPressed: () => Navigator.pop(dialogContext, true),
                child: const Text('Create')),
          ],
        ),
      ),
    );
    if (ok == true && mounted) {
      try {
        // Backend requires the initial password in the request; generate one
        // client-side and show it once (user changes it after first login).
        final initialPassword = _generatePassword();
        await context.read<ApiClient>().adminCreateUser(
              username: username.text,
              email: email.text,
              name: name.text,
              password: initialPassword,
              isAdmin: isAdmin,
            );
        await _load();
        if (mounted) {
          await showDialog<void>(
            context: context,
            builder: (dialogContext) => AlertDialog(
              title: const Text('User created'),
              content: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text(
                      'Initial password (shown once, copy it now):'),
                  const SizedBox(height: 8),
                  SelectableText(initialPassword,
                      style: RgitTheme.mono),
                ],
              ),
              actions: [
                FilledButton(
                    onPressed: () => Navigator.pop(dialogContext),
                    child: const Text('Close')),
              ],
            ),
          );
        } else if (mounted) {
          _snack('User created.');
        }
      } catch (e) {
        if (mounted) _snack('$e');
      }
    }
    username.dispose();
    email.dispose();
    name.dispose();
  }

  Future<void> _update(models.User user, Map<String, dynamic> fields,
      String success) async {
    try {
      await context.read<ApiClient>().adminUpdateUser(user.id, fields);
      await _load();
      if (mounted) _snack(success);
    } catch (e) {
      if (mounted) _snack('$e');
    }
  }

  Future<void> _delete(models.User user) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Delete user'),
        content: Text('Delete "${user.username}"? This cannot be undone.'),
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
        await context.read<ApiClient>().adminDeleteUser(user.id);
        await _load();
        if (mounted) _snack('User deleted.');
      } catch (e) {
        if (mounted) _snack('$e');
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final users = _page?.items ?? const <models.User>[];
    return PageShell(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const AdminTabs(selected: 'users'),
          const SizedBox(height: 16),
          Row(
            children: [
              Text('Users (${_page?.total ?? 0})',
                  style: const TextStyle(
                      fontSize: 16, fontWeight: FontWeight.w600)),
              const Spacer(),
              FilledButton.icon(
                icon: const Icon(Icons.person_add_outlined, size: 16),
                label: const Text('New user'),
                onPressed: _createUser,
              ),
            ],
          ),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else
            SizedBox(
              width: double.infinity,
              child: DataTable(
                columns: const [
                  DataColumn(label: Text('ID')),
                  DataColumn(label: Text('Username')),
                  DataColumn(label: Text('Email')),
                  DataColumn(label: Text('Name')),
                  DataColumn(label: Text('State')),
                  DataColumn(label: Text('Admin')),
                  DataColumn(label: Text('Actions')),
                ],
                rows: [
                  for (final u in users)
                    DataRow(cells: [
                      DataCell(Text('${u.id}')),
                      DataCell(Text(u.username)),
                      DataCell(Text(u.email)),
                      DataCell(Text(u.name)),
                      DataCell(Text(u.state)),
                      DataCell(Icon(
                          u.isAdmin ? Icons.check : Icons.close,
                          size: 16)),
                      DataCell(Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          IconButton(
                            tooltip:
                                u.isActive ? 'Block user' : 'Unblock user',
                            icon: Icon(
                                u.isActive
                                    ? Icons.block
                                    : Icons.lock_open,
                                size: 16),
                            onPressed: () => _update(
                                u,
                                {
                                  'state':
                                      u.isActive ? 'blocked' : 'active'
                                },
                                u.isActive
                                    ? 'User blocked.'
                                    : 'User unblocked.'),
                          ),
                          IconButton(
                            tooltip: u.isAdmin
                                ? 'Revoke admin'
                                : 'Grant admin',
                            icon: const Icon(
                                Icons.admin_panel_settings_outlined,
                                size: 16),
                            onPressed: () => _update(
                                u,
                                {'is_admin': !u.isAdmin},
                                'Role updated.'),
                          ),
                          IconButton(
                            tooltip: 'Delete user',
                            icon: const Icon(Icons.delete_outline,
                                size: 16),
                            onPressed: () => _delete(u),
                          ),
                        ],
                      )),
                    ]),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
