import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/error_view.dart';
import '../widgets/file_tree_list.dart';
import '../widgets/loading.dart';
import '../widgets/markdown_view.dart';
import '../widgets/project_scaffold.dart';
import '../widgets/project_tabs.dart';
import '../widgets/top_nav.dart';

/// Repo home: file tree + README + clone box + branch dropdown
/// (route: /:ns/:proj).
class ProjectHomePage extends StatefulWidget {
  const ProjectHomePage({super.key, required this.ns, required this.proj});

  final String ns;
  final String proj;

  @override
  State<ProjectHomePage> createState() => _ProjectHomePageState();
}

class _ProjectHomePageState extends State<ProjectHomePage> {
  models.Project? _project;
  List<models.Branch> _branches = const [];
  List<models.TreeEntry> _entries = const [];
  models.CommitInfo? _latestCommit;
  int _commitCount = 0;
  models.ReadmeFile? _readme;
  String? _ref;
  Object? _error;
  bool _loading = true;

  String get _fullPath => '${widget.ns}/${widget.proj}';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(ProjectHomePage old) {
    super.didUpdateWidget(old);
    if (old.ns != widget.ns || old.proj != widget.proj) {
      _ref = null;
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
      final project = await api.getProject(_fullPath);
      final ref = _ref ?? project.defaultBranch ?? 'main';
      final results = await Future.wait<Object?>([
        api.branches(_fullPath).catchError((_) => const <models.Branch>[]),
        api
            .tree(_fullPath, ref: ref)
            .then<List<models.TreeEntry>>((p) => p.items)
            .catchError((_) => const <models.TreeEntry>[]),
        api
            .commits(_fullPath, ref: ref, perPage: 1)
            .catchError(
              (_) => const models.Paged<models.CommitInfo>(
                items: <models.CommitInfo>[],
                total: 0,
                page: 1,
              ),
            ),
        api.readme(_fullPath, ref: ref).catchError((_) => null),
      ]);
      if (!mounted) return;
      setState(() {
        _project = project;
        _ref = ref;
        _branches = results[0] as List<models.Branch>;
        _entries = results[1] as List<models.TreeEntry>;
        final commits = results[2] as models.Paged<models.CommitInfo>;
        _latestCommit = commits.items.isEmpty ? null : commits.items.first;
        _commitCount = commits.total;
        _readme = results[3] as models.ReadmeFile?;
      });
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Widget _repositoryContent(String ref) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Row(
        children: [
          _BranchDropdown(
            branches: _branches,
            selected: ref,
            onChanged: (branch) {
              _ref = branch;
              _load();
            },
          ),
          const Spacer(),
          OutlinedButton.icon(
            icon: const Icon(Icons.history, size: 16),
            label: const Text('Commits'),
            onPressed: () => context.go('/$_fullPath/commits/$ref'),
          ),
        ],
      ),
      const SizedBox(height: 10),
      if (_loading)
        const Loading()
      else ...[
        _LatestCommitHeader(
          commit: _latestCommit,
          commitCount: _commitCount,
          onCommitTap: _latestCommit == null
              ? null
              : () => context.go('/$_fullPath/commit/${_latestCommit!.sha}'),
          onHistoryTap: () => context.go('/$_fullPath/commits/$ref'),
        ),
        FileTreeList(
          entries: _entries,
          projectFullPath: _fullPath,
          ref: ref,
          hasHeader: true,
        ),
      ],
      const SizedBox(height: 16),
      if (_readme != null)
        MarkdownView(data: _readme!.content, title: _readme!.path),
    ],
  );
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
    final ref = _ref ?? project.defaultBranch ?? 'main';
    return ProjectScaffold(
      project: project,
      selected: ProjectTab.code,
      archiveRef: ref,
      child: _repositoryContent(ref),
    );
  }
}

class _LatestCommitHeader extends StatelessWidget {
  const _LatestCommitHeader({
    required this.commit,
    required this.commitCount,
    required this.onCommitTap,
    required this.onHistoryTap,
  });

  final models.CommitInfo? commit;
  final int commitCount;
  final VoidCallback? onCommitTap;
  final VoidCallback onHistoryTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = theme.dividerColor;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
      decoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHighest,
        border: Border.all(color: border),
        borderRadius: const BorderRadius.vertical(top: Radius.circular(6)),
      ),
      child: Row(
        children: [
          Expanded(
            child: commit == null
                ? const Text('No commits yet.')
                : InkWell(
                    onTap: onCommitTap,
                    child: Row(
                      children: [
                        Flexible(
                          child: Text(
                            commit!.title,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(
                              fontSize: 16,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ),
                        const SizedBox(width: 8),
                        Text(
                          commit!.shortSha,
                          style: theme.textTheme.bodyMedium?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                            fontFamily: 'Roboto Mono',
                            letterSpacing: 0,
                          ),
                        ),
                      ],
                    ),
                  ),
          ),
          const SizedBox(width: 12),
          InkWell(
            onTap: onHistoryTap,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Icon(Icons.history, size: 18),
                const SizedBox(width: 6),
                Text(
                  '$commitCount Commits',
                  style: const TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _BranchDropdown extends StatelessWidget {
  const _BranchDropdown({
    required this.branches,
    required this.selected,
    required this.onChanged,
  });

  final List<models.Branch> branches;
  final String selected;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    final names = branches.map((b) => b.name).toSet();
    names.add(selected);
    return DropdownMenu<String>(
      initialSelection: selected,
      leadingIcon: const Icon(Icons.call_split, size: 16),
      textStyle: const TextStyle(fontSize: 13),
      inputDecorationTheme: const InputDecorationTheme(
        isDense: true,
        border: OutlineInputBorder(),
      ),
      dropdownMenuEntries: [
        for (final n in names) DropdownMenuEntry(value: n, label: n),
      ],
      onSelected: (v) {
        if (v != null) onChanged(v);
      },
    );
  }
}
