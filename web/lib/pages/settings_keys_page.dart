import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../theme.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';
import '../widgets/top_nav.dart';

/// SSH key management (route: /settings/keys).
class SettingsKeysPage extends StatefulWidget {
  const SettingsKeysPage({super.key});

  @override
  State<SettingsKeysPage> createState() => _SettingsKeysPageState();
}

class _SettingsKeysPageState extends State<SettingsKeysPage> {
  List<models.SshKey>? _keys;
  Object? _error;
  bool _loading = true;

  final _title = TextEditingController();
  final _key = TextEditingController();
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _title.dispose();
    _key.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final keys = await context.read<ApiClient>().listKeys();
      if (mounted) setState(() => _keys = keys);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _snack(String message) {
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _add() async {
    setState(() => _busy = true);
    try {
      await context
          .read<ApiClient>()
          .addKey(title: _title.text, key: _key.text);
      _title.clear();
      _key.clear();
      await _load();
      if (mounted) _snack('SSH key added.');
    } catch (e) {
      if (mounted) _snack('$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _delete(models.SshKey key) async {
    try {
      await context.read<ApiClient>().deleteKey(key.id);
      await _load();
      if (mounted) _snack('SSH key removed.');
    } catch (e) {
      if (mounted) _snack('$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final border = Theme.of(context).dividerColor;
    return PageShell(
      maxWidth: 720,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('SSH keys',
              style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600)),
          const SizedBox(height: 16),
          TextField(
            controller: _title,
            decoration: const InputDecoration(
                labelText: 'Title', hintText: 'e.g. work laptop'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _key,
            maxLines: 4,
            style: RgitTheme.mono,
            decoration: const InputDecoration(
              labelText: 'Public key',
              hintText: 'Begins with ssh-ed25519, ssh-rsa, …',
            ),
          ),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton(
              onPressed: _busy ? null : _add,
              child: const Text('Add key'),
            ),
          ),
          const SizedBox(height: 24),
          const Divider(),
          const SizedBox(height: 12),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else if ((_keys ?? const []).isEmpty)
            const Padding(
              padding: EdgeInsets.all(24),
              child: Center(child: Text('No SSH keys yet.')),
            )
          else
            for (final k in _keys!)
              Container(
                margin: const EdgeInsets.only(bottom: 8),
                decoration: BoxDecoration(
                  border: Border.all(color: border),
                  borderRadius: BorderRadius.circular(6),
                ),
                child: ListTile(
                  leading: const Icon(Icons.vpn_key_outlined),
                  title: Text(k.title),
                  subtitle: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('SHA256:${k.fingerprintSha256}',
                          style: RgitTheme.mono.copyWith(fontSize: 11)),
                      Text(
                        'Added ${k.createdAt ?? '-'}'
                        '${k.lastUsedAt != null ? ' · last used ${k.lastUsedAt}' : ''}',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ],
                  ),
                  trailing: IconButton(
                    tooltip: 'Delete key',
                    icon: const Icon(Icons.delete_outline),
                    onPressed: () => _delete(k),
                  ),
                ),
              ),
        ],
      ),
    );
  }
}
