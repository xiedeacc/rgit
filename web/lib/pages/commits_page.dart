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

/// Commit list (route: /:ns/:proj/commits/:ref).
class CommitsPage extends StatefulWidget {
  const CommitsPage({
    super.key,
    required this.ns,
    required this.proj,
    required this.gitRef,
  });

  final String ns;
  final String proj;
  final String gitRef;

  @override
  State<CommitsPage> createState() => _CommitsPageState();
}

class _CommitsPageState extends State<CommitsPage> {
  models.Project? _project;
  models.Paged<models.CommitInfo>? _page;
  Object? _error;
  bool _loading = true;
  int _pageNo = 1;
  static const int _perPage = 20;

  String get _fullPath => '${widget.ns}/${widget.proj}';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(CommitsPage old) {
    super.didUpdateWidget(old);
    if (old.ns != widget.ns ||
        old.proj != widget.proj ||
        old.gitRef != widget.gitRef) {
      _pageNo = 1;
      _load();
    }
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    final api = context.read<ApiClient>();
    try {
      _project ??= await api.getProject(_fullPath);
      final page = await api.commits(
        _fullPath,
        ref: widget.gitRef,
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
    final border = Theme.of(context).dividerColor;
    final commits = _page?.items ?? const <models.CommitInfo>[];
    final totalPages = (((_page?.total ?? 0) + _perPage - 1) ~/ _perPage).clamp(
      1,
      1 << 30,
    );
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
      selected: ProjectTab.commits,
      archiveRef: widget.gitRef,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Commits on ${widget.gitRef}',
            style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else ...[
            Container(
              decoration: BoxDecoration(
                border: Border.all(color: border),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Column(
                children: [
                  for (var i = 0; i < commits.length; i++) ...[
                    if (i > 0) Divider(height: 1, color: border),
                    _CommitRow(
                      commit: commits[i],
                      onTap: () =>
                          context.go('/$_fullPath/commit/${commits[i].sha}'),
                    ),
                  ],
                  if (commits.isEmpty)
                    const Padding(
                      padding: EdgeInsets.all(24),
                      child: Text('No commits found.'),
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
                      child: const Text('Newer'),
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
                      child: const Text('Older'),
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

class _CommitRow extends StatelessWidget {
  const _CommitRow({required this.commit, required this.onTap});

  final models.CommitInfo commit;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    commit.title,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    '${commit.authorName} · ${commit.authoredAt ?? ''}',
                    style: theme.textTheme.bodySmall,
                  ),
                ],
              ),
            ),
            const SizedBox(width: 12),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
              decoration: BoxDecoration(
                border: Border.all(color: theme.dividerColor),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Text(commit.shortSha, style: RgitTheme.mono),
            ),
          ],
        ),
      ),
    );
  }
}
