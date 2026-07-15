import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart';

/// Rendered markdown (README, .md blobs) in a bordered panel.
class MarkdownView extends StatelessWidget {
  const MarkdownView({super.key, required this.data, this.title});

  final String data;
  final String? title;

  @override
  Widget build(BuildContext context) {
    final border = Theme.of(context).dividerColor;
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
              child: Text(title!,
                  style: const TextStyle(fontWeight: FontWeight.w600)),
            ),
          Padding(
            padding: const EdgeInsets.all(24),
            child: MarkdownBody(data: data, selectable: true),
          ),
        ],
      ),
    );
  }
}
