import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart';

/// Rendered markdown (README, .md blobs) in a bordered panel.
class MarkdownView extends StatelessWidget {
  const MarkdownView({super.key, required this.data, this.title});

  final String data;
  final String? title;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = Theme.of(context).dividerColor;
    const sans = ['Droid Sans Fallback'];
    const mono = ['Roboto Mono', 'Droid Sans Fallback'];
    final body = theme.textTheme.bodyMedium?.copyWith(
      fontFamily: 'Roboto',
      fontFamilyFallback: sans,
      fontSize: 16,
      height: 1.5,
      letterSpacing: 0,
    );
    final titleLarge = theme.textTheme.headlineSmall?.copyWith(
      fontFamily: 'Roboto',
      fontFamilyFallback: sans,
      letterSpacing: 0,
    );
    final titleMedium = theme.textTheme.titleLarge?.copyWith(
      fontFamily: 'Roboto',
      fontFamilyFallback: sans,
      letterSpacing: 0,
    );
    final code = theme.textTheme.bodyMedium?.copyWith(
      fontFamily: 'Roboto Mono',
      fontFamilyFallback: mono,
      fontSize: 13,
      height: 1.5,
      letterSpacing: 0,
    );
    final markdownStyle = MarkdownStyleSheet.fromTheme(theme).copyWith(
      p: body,
      listBullet: body,
      h1: titleLarge?.copyWith(fontSize: 32, fontWeight: FontWeight.w600),
      h2: titleMedium?.copyWith(fontSize: 24, fontWeight: FontWeight.w600),
      h3: titleMedium?.copyWith(fontSize: 20, fontWeight: FontWeight.w600),
      h4: body?.copyWith(fontSize: 16, fontWeight: FontWeight.w600),
      h5: body?.copyWith(fontSize: 14, fontWeight: FontWeight.w600),
      h6: body?.copyWith(fontSize: 13, fontWeight: FontWeight.w600),
      code: code,
      codeblockDecoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(6),
      ),
      codeblockPadding: const EdgeInsets.all(12),
      blockquote: body?.copyWith(color: theme.colorScheme.onSurfaceVariant),
      blockquoteDecoration: BoxDecoration(
        border: Border(
          left: BorderSide(color: theme.colorScheme.outline, width: 4),
        ),
      ),
      blockquotePadding: const EdgeInsets.fromLTRB(16, 4, 0, 4),
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
          if (title != null)
            Container(
              width: double.infinity,
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
              decoration: BoxDecoration(
                color: Theme.of(context).colorScheme.surfaceContainerHighest,
                border: Border(bottom: BorderSide(color: border)),
              ),
              child: Text(
                title!,
                style: const TextStyle(fontWeight: FontWeight.w600),
              ),
            ),
          Padding(
            padding: const EdgeInsets.all(24),
            child: MarkdownBody(
              data: data,
              selectable: true,
              styleSheet: markdownStyle,
            ),
          ),
        ],
      ),
    );
  }
}
