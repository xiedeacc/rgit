import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/account_shell.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';

/// Admin dashboard: instance stats (route: /admin).
class AdminDashboardPage extends StatefulWidget {
  const AdminDashboardPage({super.key});

  @override
  State<AdminDashboardPage> createState() => _AdminDashboardPageState();
}

class _AdminDashboardPageState extends State<AdminDashboardPage> {
  models.AdminStats? _stats;
  Object? _error;
  bool _loading = true;

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
      final stats = await context.read<ApiClient>().adminStats();
      if (mounted) setState(() => _stats = stats);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  static String _formatBytes(int bytes) {
    const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
    var value = bytes.toDouble();
    var unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit++;
    }
    return '${value.toStringAsFixed(unit == 0 ? 0 : 1)} ${units[unit]}';
  }

  @override
  Widget build(BuildContext context) {
    return AccountShell(
      selected: AccountSection.admin,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const AdminTabs(selected: 'dashboard'),
          const SizedBox(height: 16),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else
            Wrap(
              spacing: 12,
              runSpacing: 12,
              children: [
                _StatCard(label: 'Users', value: '${_stats!.users}'),
                _StatCard(label: 'Projects', value: '${_stats!.projects}'),
                _StatCard(label: 'Groups', value: '${_stats!.groups}'),
                _StatCard(label: 'LFS objects', value: '${_stats!.lfsObjects}'),
                _StatCard(
                  label: 'LFS size',
                  value: _formatBytes(_stats!.lfsBytes),
                ),
                _StatCard(label: 'Version', value: _stats!.version),
              ],
            ),
        ],
      ),
    );
  }
}

class _StatCard extends StatelessWidget {
  const _StatCard({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      width: 220,
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        border: Border.all(color: theme.dividerColor),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: theme.textTheme.bodySmall),
          const SizedBox(height: 4),
          Text(
            value,
            style: const TextStyle(fontSize: 22, fontWeight: FontWeight.w700),
          ),
        ],
      ),
    );
  }
}

/// Shared tab strip for the admin area.
class AdminTabs extends StatelessWidget {
  const AdminTabs({super.key, required this.selected});

  final String selected;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    const tabs = [
      ('dashboard', 'Dashboard', '/admin'),
      ('users', 'Users', '/admin/users'),
      ('projects', 'Projects', '/admin/projects'),
    ];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Admin area',
          style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 12),
        Row(
          children: [
            for (final (id, label, route) in tabs)
              InkWell(
                onTap: () => context.go(route),
                child: Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 14,
                    vertical: 9,
                  ),
                  decoration: BoxDecoration(
                    border: Border(
                      bottom: BorderSide(
                        width: 2,
                        color: id == selected
                            ? theme.colorScheme.primary
                            : Colors.transparent,
                      ),
                    ),
                  ),
                  child: Text(
                    label,
                    style: TextStyle(
                      fontWeight: id == selected
                          ? FontWeight.w600
                          : FontWeight.w400,
                    ),
                  ),
                ),
              ),
          ],
        ),
        const Divider(height: 1),
      ],
    );
  }
}
