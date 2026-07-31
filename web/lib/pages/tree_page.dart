import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../api/ref_resolution.dart';
import '../widgets/error_view.dart';
import '../widgets/file_tree_list.dart';
import '../widgets/latest_commit_header.dart';
import '../widgets/loading.dart';
import '../widgets/project_scaffold.dart';
import '../widgets/project_tabs.dart';
import '../widgets/top_nav.dart';

/// Tree browser (route: /:ns/:proj/tree/:ref/*path).
class TreePage extends StatefulWidget {
  const TreePage({
    super.key,
    required this.ns,
    required this.proj,
    required this.gitRef,
    required this.path,
  });

  final String ns;
  final String proj;
  final String gitRef;
  final String path;

  @override
  State<TreePage> createState() => _TreePageState();
}

class _TreePageState extends State<TreePage> {
  models.Project? _project;
  List<models.TreeEntry> _entries = const [];
  models.CommitInfo? _latestCommit;
  int _commitCount = 0;
  String? _resolvedRef;
  String _resolvedPath = '';
  Object? _error;
  bool _loading = true;

  String get _fullPath => '${widget.ns}/${widget.proj}';
  String get _routeTail =>
      widget.path.isEmpty ? widget.gitRef : '${widget.gitRef}/${widget.path}';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(TreePage old) {
    super.didUpdateWidget(old);
    if (old.ns != widget.ns ||
        old.proj != widget.proj ||
        old.gitRef != widget.gitRef ||
        old.path != widget.path) {
      _load();
    }
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
      _resolvedRef = null;
      _resolvedPath = '';
    });
    final api = context.read<ApiClient>();
    try {
      _project ??= await api.getProject(_fullPath);
      final resolved = await _resolveRouteRef(api);
      final results = await Future.wait<Object?>([
        api.tree(_fullPath, ref: resolved.ref, path: resolved.path),
        api
            .commits(_fullPath, ref: resolved.ref, perPage: 1)
            .catchError(
              (_) => const models.Paged<models.CommitInfo>(
                items: <models.CommitInfo>[],
                total: 0,
                page: 1,
              ),
            ),
      ]);
      if (!mounted) return;
      final page = results[0] as models.Paged<models.TreeEntry>;
      final commits = results[1] as models.Paged<models.CommitInfo>;
      setState(() {
        _entries = page.items;
        _latestCommit = commits.items.isEmpty ? null : commits.items.first;
        _commitCount = commits.total;
        _resolvedRef = resolved.ref;
        _resolvedPath = resolved.path;
      });
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<ResolvedRefPath> _resolveRouteRef(ApiClient api) async {
    final refs = <String>{};
    try {
      refs.addAll((await api.branches(_fullPath)).map((branch) => branch.name));
    } catch (_) {
      // Keep the page usable if a repository does not expose refs yet.
    }
    try {
      refs.addAll((await api.tags(_fullPath)).map((tag) => tag.name));
    } catch (_) {
      // Tags are optional for resolving tree URLs.
    }
    return resolveRefPath(_routeTail, refs);
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null && _project == null) {
      return PageShell(
        child: ErrorView(error: _error!, onRetry: _load),
      );
    }
    if (_project == null) {
      return const PageShell(child: Loading());
    }
    final ref = _resolvedRef ?? widget.gitRef;
    final path = _resolvedRef == null ? widget.path : _resolvedPath;
    return ProjectScaffold(
      project: _project!,
      selected: ProjectTab.code,
      archiveRef: ref,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          PathBreadcrumbs(fullPath: _fullPath, gitRef: ref, path: path),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else ...[
            LatestCommitHeader(
              commit: _latestCommit,
              commitCount: _commitCount,
              onCommitTap: _latestCommit == null
                  ? null
                  : () =>
                        context.go('/$_fullPath/commit/${_latestCommit!.sha}'),
              onHistoryTap: () => context.go('/$_fullPath/commits/$ref'),
            ),
            FileTreeList(
              entries: _entries,
              projectFullPath: _fullPath,
              ref: ref,
              hasHeader: true,
            ),
          ],
        ],
      ),
    );
  }
}

/// "proj / dir / subdir" breadcrumb row shared by tree and blob pages.
class PathBreadcrumbs extends StatelessWidget {
  const PathBreadcrumbs({
    super.key,
    required this.fullPath,
    required this.gitRef,
    required this.path,
  });

  final String fullPath;
  final String gitRef;
  final String path;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final parts = path.isEmpty ? const <String>[] : path.split('/');
    final crumbs = <Widget>[
      InkWell(
        onTap: () => context.go('/$fullPath'),
        child: Text(
          fullPath.split('/').last,
          style: TextStyle(
            color: theme.colorScheme.primary,
            fontWeight: FontWeight.w600,
          ),
        ),
      ),
    ];
    var acc = '';
    for (var i = 0; i < parts.length; i++) {
      acc = acc.isEmpty ? parts[i] : '$acc/${parts[i]}';
      final target = acc;
      final isLast = i == parts.length - 1;
      crumbs
        ..add(const Text(' / '))
        ..add(
          isLast
              ? Text(
                  parts[i],
                  style: const TextStyle(fontWeight: FontWeight.w600),
                )
              : InkWell(
                  onTap: () => context.go('/$fullPath/tree/$gitRef/$target'),
                  child: Text(
                    parts[i],
                    style: TextStyle(color: theme.colorScheme.primary),
                  ),
                ),
        );
    }
    return Wrap(
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        Container(
          margin: const EdgeInsets.only(right: 10),
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
          decoration: BoxDecoration(
            border: Border.all(color: theme.dividerColor),
            borderRadius: BorderRadius.circular(6),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.call_split, size: 14),
              const SizedBox(width: 4),
              Text(gitRef, style: const TextStyle(fontSize: 12)),
            ],
          ),
        ),
        ...crumbs,
      ],
    );
  }
}
