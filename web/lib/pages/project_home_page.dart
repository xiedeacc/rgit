import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../state/session.dart';
import '../widgets/clone_url_box.dart';
import '../widgets/error_view.dart';
import '../widgets/file_tree_list.dart';
import '../widgets/loading.dart';
import '../widgets/markdown_view.dart';
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

  Future<void> _forkProject(models.Project project) async {
    final namespace = TextEditingController();
    final path = TextEditingController();
    final name = TextEditingController();
    String? error;
    var busy = false;
    await showDialog<void>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (dialogContext, setDialogState) => AlertDialog(
          title: const Text('Fork project'),
          content: SizedBox(
            width: 420,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (error != null) ...[
                  Text(
                    error!,
                    style: TextStyle(
                      color: Theme.of(dialogContext).colorScheme.error,
                    ),
                  ),
                  const SizedBox(height: 10),
                ],
                TextField(
                  controller: namespace,
                  decoration: const InputDecoration(
                    labelText: 'Target namespace ID (optional)',
                  ),
                  keyboardType: TextInputType.number,
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: path,
                  decoration: const InputDecoration(
                    labelText: 'Path (optional)',
                  ),
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: name,
                  decoration: const InputDecoration(
                    labelText: 'Name (optional)',
                  ),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: busy ? null : () => Navigator.pop(dialogContext),
              child: const Text('Cancel'),
            ),
            FilledButton.icon(
              icon: const Icon(Icons.fork_right, size: 18),
              onPressed: busy
                  ? null
                  : () async {
                      final namespaceId = namespace.text.isEmpty
                          ? null
                          : int.tryParse(namespace.text);
                      if (namespace.text.isNotEmpty && namespaceId == null) {
                        setDialogState(
                          () => error = 'Namespace ID must be numeric.',
                        );
                        return;
                      }
                      setDialogState(() {
                        busy = true;
                        error = null;
                      });
                      try {
                        final fork = await context
                            .read<ApiClient>()
                            .forkProject(
                              project.id,
                              namespaceId: namespaceId,
                              path: path.text.isEmpty ? null : path.text,
                              name: name.text.isEmpty ? null : name.text,
                            );
                        if (dialogContext.mounted) {
                          Navigator.pop(dialogContext);
                        }
                        if (mounted) {
                          context.go('/${fork.fullPath}');
                        }
                      } catch (exception) {
                        if (dialogContext.mounted) {
                          setDialogState(() {
                            error = '$exception';
                            busy = false;
                          });
                        }
                      }
                    },
              label: Text(busy ? 'Forking...' : 'Create fork'),
            ),
          ],
        ),
      ),
    );
    namespace.dispose();
    path.dispose();
    name.dispose();
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

  Widget _cloneSidebar(
    ApiClient api,
    models.Project project,
    String ref,
  ) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      CloneUrlBox(
        httpsUrl: project.httpCloneUrl ?? api.httpCloneUrl(project.fullPath),
        sshUrl: project.sshCloneUrl ?? api.sshCloneUrl(project.fullPath),
      ),
      if (context.watch<SessionState>().isSignedIn) ...[
        const SizedBox(height: 10),
        SizedBox(
          width: double.infinity,
          child: OutlinedButton.icon(
            icon: const Icon(Icons.fork_right, size: 18),
            label: const Text('Fork'),
            onPressed: () => _forkProject(project),
          ),
        ),
      ],
      const SizedBox(height: 12),
      Text('Download source', style: Theme.of(context).textTheme.titleSmall),
      const SizedBox(height: 4),
      SelectableText(
        api.archiveUrl(project.fullPath, ref: ref).toString(),
        style: Theme.of(context).textTheme.bodySmall,
      ),
    ],
  );

  @override
  Widget build(BuildContext context) {
    final api = context.read<ApiClient>();
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
    return PageShell(
      maxWidth: 1480,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ProjectHeader(project: project, selected: ProjectTab.code),
          LayoutBuilder(
            builder: (context, constraints) {
              const gap = 16.0;
              const sidebarMinWidth = 300.0;
              final repoAvailable =
                  constraints.maxWidth - gap - sidebarMinWidth;
              if (constraints.maxWidth < 920 || repoAvailable < 520) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _repositoryContent(ref),
                    const SizedBox(height: 20),
                    _cloneSidebar(api, project, ref),
                  ],
                );
              }
              final targetRepoWidth = MediaQuery.sizeOf(context).width * 0.5;
              final repoWidth = targetRepoWidth
                  .clamp(520.0, repoAvailable)
                  .toDouble();
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SizedBox(width: repoWidth, child: _repositoryContent(ref)),
                  const SizedBox(width: gap),
                  Expanded(child: _cloneSidebar(api, project, ref)),
                ],
              );
            },
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
