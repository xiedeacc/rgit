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
        api.readme(_fullPath, ref: ref).catchError((_) => null),
      ]);
      if (!mounted) return;
      setState(() {
        _project = project;
        _ref = ref;
        _branches = results[0] as List<models.Branch>;
        _entries = results[1] as List<models.TreeEntry>;
        _readme = results[2] as models.ReadmeFile?;
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
      else
        FileTreeList(entries: _entries, projectFullPath: _fullPath, ref: ref),
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
