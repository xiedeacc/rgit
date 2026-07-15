import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../state/session.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/project_card.dart';
import '../widgets/top_nav.dart';

/// User/group home: project list under a namespace (route: /:ns).
class NamespacePage extends StatefulWidget {
  const NamespacePage({super.key, required this.ns});

  final String ns;

  @override
  State<NamespacePage> createState() => _NamespacePageState();
}

class _NamespacePageState extends State<NamespacePage> {
  List<models.Project>? _projects;
  Object? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(NamespacePage old) {
    super.didUpdateWidget(old);
    if (old.ns != widget.ns) _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      // No dedicated namespace listing endpoint yet: search by namespace
      // path and keep only projects under it.
      final page = await context
          .read<ApiClient>()
          .listProjects(search: widget.ns, perPage: 100);
      if (mounted) {
        setState(() => _projects = page.items
            .where((p) => p.namespacePath == widget.ns)
            .toList());
      }
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionState>();
    return PageShell(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              CircleAvatar(child: Text(widget.ns[0].toUpperCase())),
              const SizedBox(width: 12),
              Text(widget.ns,
                  style: const TextStyle(
                      fontSize: 22, fontWeight: FontWeight.w600)),
              const Spacer(),
              if (session.isSignedIn)
                OutlinedButton.icon(
                  icon: const Icon(Icons.settings_outlined, size: 18),
                  label: const Text('Group settings'),
                  onPressed: () => context.go('/${widget.ns}/settings'),
                ),
            ],
          ),
          const SizedBox(height: 20),
          const Text('Projects',
              style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else ...[
            for (final p in _projects ?? const <models.Project>[])
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: ProjectCard(project: p),
              ),
            if ((_projects ?? const []).isEmpty)
              const Padding(
                padding: EdgeInsets.all(32),
                child: Center(child: Text('No projects in this namespace.')),
              ),
          ],
        ],
      ),
    );
  }
}
