import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../state/session.dart';
import 'clone_url_box.dart';
import 'project_tabs.dart';
import 'top_nav.dart';

/// Shared project page layout: header, tab bar, main content and clone sidebar.
class ProjectScaffold extends StatelessWidget {
  const ProjectScaffold({
    super.key,
    required this.project,
    required this.selected,
    required this.child,
    this.archiveRef,
  });

  final models.Project project;
  final ProjectTab selected;
  final Widget child;
  final String? archiveRef;

  @override
  Widget build(BuildContext context) {
    final ref = archiveRef ?? project.defaultBranch ?? 'main';
    return Scaffold(
      appBar: const TopNav(),
      body: SingleChildScrollView(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: LayoutBuilder(
            builder: (context, constraints) => _ProjectLayout(
              maxWidth: constraints.maxWidth,
              header: ProjectHeader(project: project, selected: selected),
              sidebar: _ProjectSidebar(project: project, ref: ref),
              child: child,
            ),
          ),
        ),
      ),
    );
  }
}

class _ProjectLayout extends StatelessWidget {
  const _ProjectLayout({
    required this.maxWidth,
    required this.header,
    required this.sidebar,
    required this.child,
  });

  final double maxWidth;
  final Widget header;
  final Widget sidebar;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    const gap = 16.0;
    const sidebarWidth = 300.0;
    final mainWidth = maxWidth.clamp(0.0, kPageContentMaxWidth).toDouble();
    final mainLeft = (maxWidth - mainWidth) / 2;
    final sidebarFits =
        mainLeft + mainWidth + gap + sidebarWidth <= maxWidth && mainWidth > 0;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Center(
          child: SizedBox(width: mainWidth, child: header),
        ),
        if (sidebarFits)
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(width: mainLeft),
              SizedBox(width: mainWidth, child: child),
              const SizedBox(width: gap),
              SizedBox(width: sidebarWidth, child: sidebar),
              const Expanded(child: SizedBox.shrink()),
            ],
          )
        else
          Center(
            child: SizedBox(
              width: mainWidth,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [child, const SizedBox(height: 20), sidebar],
              ),
            ),
          ),
      ],
    );
  }
}

class _ProjectSidebar extends StatelessWidget {
  const _ProjectSidebar({required this.project, required this.ref});

  final models.Project project;
  final String ref;

  Future<void> _forkProject(BuildContext context) async {
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
                        if (context.mounted) {
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

  @override
  Widget build(BuildContext context) {
    final api = context.read<ApiClient>();
    return Column(
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
              onPressed: () => _forkProject(context),
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
  }
}
