import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../theme.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/project_tabs.dart';
import '../widgets/top_nav.dart';

/// Commit detail with raw patch (route: /:ns/:proj/commit/:sha).
class CommitPage extends StatefulWidget {
  const CommitPage({
    super.key,
    required this.ns,
    required this.proj,
    required this.sha,
  });

  final String ns;
  final String proj;
  final String sha;

  @override
  State<CommitPage> createState() => _CommitPageState();
}

class _CommitPageState extends State<CommitPage> {
  models.Project? _project;
  models.CommitInfo? _commit;
  String? _diff;
  Object? _error;
  bool _loading = true;

  String get _fullPath => '${widget.ns}/${widget.proj}';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(CommitPage old) {
    super.didUpdateWidget(old);
    if (old.ns != widget.ns ||
        old.proj != widget.proj ||
        old.sha != widget.sha) {
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
      final commit = await api.commit(_fullPath, widget.sha);
      final diff = await api
          .diff(_fullPath, widget.sha)
          .catchError((Object _) => '');
      if (mounted) {
        setState(() {
          _commit = commit;
          _diff = diff;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = theme.dividerColor;
    return PageShell(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (_project != null)
            ProjectHeader(project: _project!, selected: ProjectTab.commits),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else ...[
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(16),
              decoration: BoxDecoration(
                border: Border.all(color: border),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(_commit!.title,
                      style: const TextStyle(
                          fontSize: 16, fontWeight: FontWeight.w600)),
                  const SizedBox(height: 6),
                  Text(
                    '${_commit!.authorName} <${_commit!.authorEmail}> · '
                    '${_commit!.authoredAt ?? ''}',
                    style: theme.textTheme.bodySmall,
                  ),
                  const SizedBox(height: 6),
                  SelectableText('commit ${_commit!.sha}',
                      style: RgitTheme.mono),
                  if (_commit!.message.contains('\n')) ...[
                    const SizedBox(height: 10),
                    SelectableText(
                      _commit!.message
                          .split('\n')
                          .skip(1)
                          .join('\n')
                          .trim(),
                      style: theme.textTheme.bodyMedium,
                    ),
                  ],
                  if (_commit!.parents.isNotEmpty) ...[
                    const SizedBox(height: 6),
                    Text(
                      'Parents: '
                      '${_commit!.parents.map((p) => p.length > 8 ? p.substring(0, 8) : p).join(', ')}',
                      style: theme.textTheme.bodySmall,
                    ),
                  ],
                ],
              ),
            ),
            const SizedBox(height: 16),
            if (_diff != null && _diff!.isNotEmpty)
              Container(
                width: double.infinity,
                decoration: BoxDecoration(
                  border: Border.all(color: border),
                  borderRadius: BorderRadius.circular(6),
                ),
                child: SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Padding(
                    padding: const EdgeInsets.all(12),
                    child: SelectableText(_diff!, style: RgitTheme.mono),
                  ),
                ),
              )
            else
              const Text('No diff available.'),
          ],
        ],
      ),
    );
  }
}
