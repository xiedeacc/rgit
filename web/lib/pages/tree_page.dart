import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/error_view.dart';
import '../widgets/file_tree_list.dart';
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
  Object? _error;
  bool _loading = true;

  String get _fullPath => '${widget.ns}/${widget.proj}';

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
    });
    final api = context.read<ApiClient>();
    try {
      _project ??= await api.getProject(_fullPath);
      final page = await api.tree(
        _fullPath,
        ref: widget.gitRef,
        path: widget.path,
      );
      if (mounted) setState(() => _entries = page.items);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
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
    return ProjectScaffold(
      project: _project!,
      selected: ProjectTab.code,
      archiveRef: widget.gitRef,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          PathBreadcrumbs(
            fullPath: _fullPath,
            gitRef: widget.gitRef,
            path: widget.path,
          ),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else
            FileTreeList(
              entries: _entries,
              projectFullPath: _fullPath,
              ref: widget.gitRef,
            ),
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
