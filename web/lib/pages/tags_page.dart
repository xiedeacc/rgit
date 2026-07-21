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

/// Tag list with archive download URLs (route: /:ns/:proj/tags).
class TagsPage extends StatefulWidget {
  const TagsPage({super.key, required this.ns, required this.proj});

  final String ns;
  final String proj;

  @override
  State<TagsPage> createState() => _TagsPageState();
}

class _TagsPageState extends State<TagsPage> {
  models.Project? _project;
  List<models.Tag> _tags = const [];
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
      final tags = await api.tags(_fullPath);
      if (mounted) setState(() => _tags = tags);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final api = context.read<ApiClient>();
    final border = Theme.of(context).dividerColor;
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
      selected: ProjectTab.tags,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            'Tags',
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
                  for (var i = 0; i < _tags.length; i++) ...[
                    if (i > 0) Divider(height: 1, color: border),
                    ListTile(
                      dense: true,
                      leading: const Icon(Icons.sell_outlined, size: 18),
                      title: Text(
                        _tags[i].name,
                        style: const TextStyle(fontWeight: FontWeight.w600),
                      ),
                      subtitle: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            'tar.gz: '
                            '${api.archiveUrl(_fullPath, ref: _tags[i].name)}'
                            '  ·  zip: '
                            '${api.archiveUrl(_fullPath, ref: _tags[i].name, format: 'zip')}',
                            style: RgitTheme.mono.copyWith(fontSize: 11),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ],
                      ),
                      onTap: () =>
                          context.go('/$_fullPath/tree/${_tags[i].name}'),
                    ),
                  ],
                  if (_tags.isEmpty)
                    const Padding(
                      padding: EdgeInsets.all(24),
                      child: Text('No tags.'),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
