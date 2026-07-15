import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/project_card.dart';
import '../widgets/top_nav.dart';
import 'project_new_dialog.dart';

/// Explore: visible project list + search (route: /).
class ExplorePage extends StatefulWidget {
  const ExplorePage({super.key, this.initialQuery});

  final String? initialQuery;

  @override
  State<ExplorePage> createState() => _ExplorePageState();
}

class _ExplorePageState extends State<ExplorePage> {
  late final TextEditingController _search =
      TextEditingController(text: widget.initialQuery ?? '');
  models.Paged<models.Project>? _result;
  Object? _error;
  bool _loading = true;
  int _page = 1;
  static const int _perPage = 20;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(ExplorePage old) {
    super.didUpdateWidget(old);
    if (old.initialQuery != widget.initialQuery) {
      _search.text = widget.initialQuery ?? '';
      _page = 1;
      _load();
    }
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final result = await context.read<ApiClient>().listProjects(
          search: _search.text, page: _page, perPage: _perPage);
      if (mounted) setState(() => _result = result);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return PageShell(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Text('Explore projects',
                  style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600)),
              const Spacer(),
              FilledButton.icon(
                icon: const Icon(Icons.add, size: 18),
                label: const Text('New project'),
                onPressed: () => showNewProjectDialog(context),
              ),
            ],
          ),
          const SizedBox(height: 16),
          TextField(
            controller: _search,
            decoration: const InputDecoration(
              hintText: 'Search projects…',
              prefixIcon: Icon(Icons.search),
            ),
            onSubmitted: (_) {
              _page = 1;
              _load();
            },
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else ...[
            for (final p in _result?.items ?? const <models.Project>[])
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: ProjectCard(project: p),
              ),
            if ((_result?.items ?? const []).isEmpty)
              const Padding(
                padding: EdgeInsets.all(32),
                child: Center(child: Text('No projects found.')),
              ),
            _Pager(
              page: _page,
              perPage: _perPage,
              total: _result?.total ?? 0,
              onPage: (p) {
                _page = p;
                _load();
              },
            ),
          ],
        ],
      ),
    );
  }
}

class _Pager extends StatelessWidget {
  const _Pager({
    required this.page,
    required this.perPage,
    required this.total,
    required this.onPage,
  });

  final int page;
  final int perPage;
  final int total;
  final ValueChanged<int> onPage;

  @override
  Widget build(BuildContext context) {
    final pages = (total + perPage - 1) ~/ perPage;
    if (pages <= 1) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          TextButton(
            onPressed: page > 1 ? () => onPage(page - 1) : null,
            child: const Text('Previous'),
          ),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: Text('Page $page of $pages'),
          ),
          TextButton(
            onPressed: page < pages ? () => onPage(page + 1) : null,
            child: const Text('Next'),
          ),
        ],
      ),
    );
  }
}
