import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../theme.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/markdown_view.dart';
import '../widgets/project_scaffold.dart';
import '../widgets/project_tabs.dart';
import '../widgets/top_nav.dart';
import 'tree_page.dart' show PathBreadcrumbs;

/// Blob viewer (route: /:ns/:proj/blob/:ref/*path). Markdown files render;
/// everything else shows as monospace text with line numbers.
class BlobPage extends StatefulWidget {
  const BlobPage({
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
  State<BlobPage> createState() => _BlobPageState();
}

class _BlobPageState extends State<BlobPage> {
  models.Project? _project;
  models.BlobFile? _blob;
  Object? _error;
  bool _loading = true;

  String get _fullPath => '${widget.ns}/${widget.proj}';
  bool get _isMarkdown => widget.path.toLowerCase().endsWith('.md');

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(BlobPage old) {
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
      final blob = await api.blob(
        _fullPath,
        ref: widget.gitRef,
        path: widget.path,
      );
      if (mounted) setState(() => _blob = blob);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final api = context.read<ApiClient>();
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
          Row(
            children: [
              Expanded(
                child: PathBreadcrumbs(
                  fullPath: _fullPath,
                  gitRef: widget.gitRef,
                  path: widget.path,
                ),
              ),
              Text(
                'Raw: ${api.rawUrl(_fullPath, ref: widget.gitRef, path: widget.path)}',
                style: Theme.of(context).textTheme.bodySmall,
                overflow: TextOverflow.ellipsis,
              ),
            ],
          ),
          const SizedBox(height: 10),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else if (_blob!.binary)
            _BinaryPlaceholder(size: _blob!.size)
          else if (_isMarkdown)
            MarkdownView(data: _blob!.text ?? '', title: widget.path)
          else
            _CodeView(content: _blob!.text ?? '', size: _blob!.size),
        ],
      ),
    );
  }
}

class _BinaryPlaceholder extends StatelessWidget {
  const _BinaryPlaceholder({required this.size});

  final int size;

  @override
  Widget build(BuildContext context) {
    final border = Theme.of(context).dividerColor;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(40),
      decoration: BoxDecoration(
        border: Border.all(color: border),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        children: [
          const Icon(Icons.insert_drive_file_outlined, size: 36),
          const SizedBox(height: 10),
          Text('Binary file ($size bytes) — view it via the raw URL above.'),
        ],
      ),
    );
  }
}

class _CodeView extends StatelessWidget {
  const _CodeView({required this.content, this.size});

  final String content;
  final int? size;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = theme.dividerColor;
    var lines = content.split('\n');
    if (lines.isNotEmpty && lines.last.isEmpty) {
      lines = lines.sublist(0, lines.length - 1);
    }
    final gutterStyle = RgitTheme.mono.copyWith(
      color: theme.colorScheme.onSurface.withValues(alpha: 0.45),
    );
    return Container(
      width: double.infinity,
      decoration: BoxDecoration(
        border: Border.all(color: border),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: double.infinity,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            decoration: BoxDecoration(
              color: theme.colorScheme.surfaceContainerHighest,
              border: Border(bottom: BorderSide(color: border)),
            ),
            child: Text(
              '${lines.length} lines${size != null ? ' · $size bytes' : ''}',
              style: theme.textTheme.bodySmall,
            ),
          ),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Column(
                    crossAxisAlignment: CrossAxisAlignment.end,
                    children: [
                      for (var i = 1; i <= lines.length; i++)
                        Text('$i', style: gutterStyle),
                    ],
                  ),
                  const SizedBox(width: 16),
                  SelectableText(lines.join('\n'), style: RgitTheme.mono),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
