import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/account_shell.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import 'admin_dashboard_page.dart' show AdminTabs;

/// Admin project overview (route: /admin/projects).
class AdminProjectsPage extends StatefulWidget {
  const AdminProjectsPage({super.key});

  @override
  State<AdminProjectsPage> createState() => _AdminProjectsPageState();
}

class _AdminProjectsPageState extends State<AdminProjectsPage> {
  models.Paged<models.Project>? _page;
  Object? _error;
  bool _loading = true;
  int _pageNo = 1;
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
      final page = await context.read<ApiClient>().adminListProjects(
        page: _pageNo,
        perPage: _perPage,
      );
      if (mounted) setState(() => _page = page);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final projects = _page?.items ?? const <models.Project>[];
    final totalPages = (((_page?.total ?? 0) + _perPage - 1) ~/ _perPage).clamp(
      1,
      1 << 30,
    );
    return AccountShell(
      selected: AccountSection.admin,
      maxContentWidth: 920,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const AdminTabs(selected: 'projects'),
          const SizedBox(height: 16),
          Text(
            'Projects (${_page?.total ?? 0})',
            style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else ...[
            SizedBox(
              width: double.infinity,
              child: DataTable(
                columns: const [
                  DataColumn(label: Text('ID')),
                  DataColumn(label: Text('Path')),
                  DataColumn(label: Text('Visibility')),
                  DataColumn(label: Text('Archived')),
                  DataColumn(label: Text('Default branch')),
                ],
                rows: [
                  for (final p in projects)
                    DataRow(
                      cells: [
                        DataCell(Text('${p.id}')),
                        DataCell(
                          Text(
                            p.fullPath,
                            style: TextStyle(
                              color: Theme.of(context).colorScheme.primary,
                            ),
                          ),
                          onTap: () => context.go('/${p.fullPath}'),
                        ),
                        DataCell(Text(models.Visibility.label(p.visibility))),
                        DataCell(
                          Icon(
                            p.archived ? Icons.check : Icons.close,
                            size: 16,
                          ),
                        ),
                        DataCell(Text(p.defaultBranch ?? '-')),
                      ],
                    ),
                ],
              ),
            ),
            if (totalPages > 1)
              Padding(
                padding: const EdgeInsets.only(top: 12),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    TextButton(
                      onPressed: _pageNo > 1
                          ? () {
                              _pageNo--;
                              _load();
                            }
                          : null,
                      child: const Text('Previous'),
                    ),
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      child: Text('Page $_pageNo of $totalPages'),
                    ),
                    TextButton(
                      onPressed: _pageNo < totalPages
                          ? () {
                              _pageNo++;
                              _load();
                            }
                          : null,
                      child: const Text('Next'),
                    ),
                  ],
                ),
              ),
          ],
        ],
      ),
    );
  }
}
