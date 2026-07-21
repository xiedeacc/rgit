import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../theme.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/project_scaffold.dart';
import '../widgets/project_tabs.dart';
import '../widgets/top_nav.dart';

/// Branch list with archive download URLs (route: /:ns/:proj/branches).
class BranchesPage extends StatefulWidget {
  const BranchesPage({super.key, required this.ns, required this.proj});

  final String ns;
  final String proj;

  @override
  State<BranchesPage> createState() => _BranchesPageState();
}

class _BranchesPageState extends State<BranchesPage> {
  models.Project? _project;
  List<models.Branch> _branches = const [];
  Object? _error;
  bool _loading = true;

  String get _fullPath => '${widget.ns}/${widget.proj}';

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
    final api = context.read<ApiClient>();
    try {
      _project ??= await api.getProject(_fullPath);
      final branches = await api.branches(_fullPath);
      if (mounted) setState(() => _branches = branches);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = theme.dividerColor;
    if (_error != null && _project == null) {
      return PageShell(
        child: ErrorView(error: _error!, onRetry: _load),
      );
    }
    if (_project == null) {
      return const PageShell(child: Loading());
    }
    return ProjectScaffold(
      project: _project!,
      selected: ProjectTab.branches,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            'Branches',
            style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else
            Container(
              decoration: BoxDecoration(
                border: Border.all(color: border),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Column(
                children: [
                  for (var i = 0; i < _branches.length; i++) ...[
                    if (i > 0) Divider(height: 1, color: border),
                    ListTile(
                      dense: true,
                      leading: const Icon(Icons.call_split, size: 18),
                      title: Row(
                        children: [
                          Text(
                            _branches[i].name,
                            style: const TextStyle(fontWeight: FontWeight.w600),
                          ),
                          if (_branches[i].name == _project?.defaultBranch) ...[
                            const SizedBox(width: 8),
                            Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 6,
                                vertical: 1,
                              ),
                              decoration: BoxDecoration(
                                border: Border.all(color: border),
                                borderRadius: BorderRadius.circular(999),
                              ),
                              child: const Text(
                                'default',
                                style: TextStyle(fontSize: 11),
                              ),
                            ),
                          ],
                        ],
                      ),
                      subtitle: _branches[i].sha == null
                          ? null
                          : Text(
                              _branches[i].sha!.substring(
                                0,
                                _branches[i].sha!.length > 8
                                    ? 8
                                    : _branches[i].sha!.length,
                              ),
                              style: RgitTheme.mono.copyWith(fontSize: 11),
                            ),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          TextButton(
                            onPressed: () => context.go(
                              '/$_fullPath/commits/${_branches[i].name}',
                            ),
                            child: const Text('Commits'),
                          ),
                          const SizedBox(width: 4),
                          Tooltip(
                            message: context
                                .read<ApiClient>()
                                .archiveUrl(_fullPath, ref: _branches[i].name)
                                .toString(),
                            child: const Icon(
                              Icons.download_outlined,
                              size: 18,
                            ),
                          ),
                        ],
                      ),
                      onTap: () =>
                          context.go('/$_fullPath/tree/${_branches[i].name}'),
                    ),
                  ],
                  if (_branches.isEmpty)
                    const Padding(
                      padding: EdgeInsets.all(24),
                      child: Text('No branches.'),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
