import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../theme.dart';

/// HTTPS/SSH clone URL box with copy-to-clipboard.
class CloneUrlBox extends StatefulWidget {
  const CloneUrlBox({super.key, required this.httpsUrl, required this.sshUrl});

  final String httpsUrl;
  final String sshUrl;

  @override
  State<CloneUrlBox> createState() => _CloneUrlBoxState();
}

class _CloneUrlBoxState extends State<CloneUrlBox> {
  bool _ssh = true;
  bool _copied = false;

  String get _url => _ssh ? widget.sshUrl : widget.httpsUrl;

  Future<void> _copy() async {
    await Clipboard.setData(ClipboardData(text: _url));
    if (!mounted) return;
    setState(() => _copied = true);
    await Future<void>.delayed(const Duration(seconds: 2));
    if (mounted) setState(() => _copied = false);
  }

  @override
  Widget build(BuildContext context) {
    final border = Theme.of(context).dividerColor;
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        border: Border.all(color: border),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Text(
                'Clone',
                style: TextStyle(fontWeight: FontWeight.w600),
              ),
              const Spacer(),
              SegmentedButton<bool>(
                showSelectedIcon: false,
                style: const ButtonStyle(visualDensity: VisualDensity.compact),
                segments: const [
                  ButtonSegment(value: false, label: Text('HTTPS')),
                  ButtonSegment(value: true, label: Text('SSH')),
                ],
                selected: {_ssh},
                onSelectionChanged: (s) => setState(() => _ssh = s.first),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 8,
                    vertical: 6,
                  ),
                  decoration: BoxDecoration(
                    color: Theme.of(
                      context,
                    ).colorScheme.surfaceContainerHighest,
                    border: Border.all(color: border),
                    borderRadius: BorderRadius.circular(6),
                  ),
                  child: SelectableText(
                    _url,
                    maxLines: 1,
                    style: RgitTheme.mono,
                  ),
                ),
              ),
              const SizedBox(width: 6),
              IconButton(
                tooltip: _copied ? 'Copied' : 'Copy URL',
                icon: Icon(_copied ? Icons.check : Icons.copy, size: 18),
                onPressed: _copy,
              ),
            ],
          ),
        ],
      ),
    );
  }
}
