import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../api/models.dart' as models;
import '../theme.dart';
import '../widgets/account_shell.dart';
import '../widgets/error_view.dart';
import '../widgets/loading.dart';

const List<String> _allScopes = <String>[
  'api',
  'read_api',
  'read_repository',
  'write_repository',
];

/// Personal access token management (route: /settings/tokens).
class SettingsTokensPage extends StatefulWidget {
  const SettingsTokensPage({super.key});

  @override
  State<SettingsTokensPage> createState() => _SettingsTokensPageState();
}

class _SettingsTokensPageState extends State<SettingsTokensPage> {
  List<models.PersonalAccessToken>? _tokens;
  Object? _error;
  bool _loading = true;

  final _name = TextEditingController();
  final _expiresAt = TextEditingController();
  final Set<String> _scopes = {'read_api'};
  bool _busy = false;

  /// Plaintext of the most recently created token (shown once).
  String? _newTokenPlaintext;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _name.dispose();
    _expiresAt.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final tokens = await context.read<ApiClient>().listTokens();
      if (mounted) setState(() => _tokens = tokens);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _snack(String message) {
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _create() async {
    if (_scopes.isEmpty) {
      _snack('Select at least one scope.');
      return;
    }
    setState(() => _busy = true);
    try {
      final token = await context.read<ApiClient>().createToken(
        name: _name.text,
        scopes: _scopes.toList(),
        expiresAt: _expiresAt.text.isEmpty ? null : _expiresAt.text,
      );
      _name.clear();
      _expiresAt.clear();
      setState(() => _newTokenPlaintext = token.plaintext);
      await _load();
    } catch (e) {
      if (mounted) _snack('$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _revoke(models.PersonalAccessToken token) async {
    try {
      await context.read<ApiClient>().revokeToken(token.id);
      await _load();
      if (mounted) _snack('Token revoked.');
    } catch (e) {
      if (mounted) _snack('$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final border = theme.dividerColor;
    return AccountShell(
      selected: AccountSection.tokens,
      maxContentWidth: 720,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text(
            'Personal access tokens',
            style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 16),
          if (_newTokenPlaintext != null)
            Container(
              margin: const EdgeInsets.only(bottom: 16),
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                border: Border.all(color: theme.colorScheme.primary),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text(
                    'Your new token — copy it now, it will not be shown '
                    'again:',
                    style: TextStyle(fontWeight: FontWeight.w600),
                  ),
                  const SizedBox(height: 6),
                  SelectableText(_newTokenPlaintext!, style: RgitTheme.mono),
                  Align(
                    alignment: Alignment.centerRight,
                    child: TextButton(
                      onPressed: () =>
                          setState(() => _newTokenPlaintext = null),
                      child: const Text('Dismiss'),
                    ),
                  ),
                ],
              ),
            ),
          TextField(
            controller: _name,
            decoration: const InputDecoration(labelText: 'Token name'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _expiresAt,
            decoration: const InputDecoration(
              labelText: 'Expires at (optional)',
              hintText: 'YYYY-MM-DD',
            ),
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            children: [
              for (final scope in _allScopes)
                FilterChip(
                  label: Text(scope),
                  selected: _scopes.contains(scope),
                  onSelected: (v) => setState(() {
                    v ? _scopes.add(scope) : _scopes.remove(scope);
                  }),
                ),
            ],
          ),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton(
              onPressed: _busy ? null : _create,
              child: const Text('Create token'),
            ),
          ),
          const SizedBox(height: 24),
          const Divider(),
          const SizedBox(height: 12),
          if (_loading)
            const Loading()
          else if (_error != null)
            ErrorView(error: _error!, onRetry: _load)
          else if ((_tokens ?? const []).isEmpty)
            const Padding(
              padding: EdgeInsets.all(24),
              child: Center(child: Text('No active tokens.')),
            )
          else
            for (final t in _tokens!)
              Container(
                margin: const EdgeInsets.only(bottom: 8),
                decoration: BoxDecoration(
                  border: Border.all(color: border),
                  borderRadius: BorderRadius.circular(6),
                ),
                child: ListTile(
                  leading: const Icon(Icons.token_outlined),
                  title: Row(
                    children: [
                      Text(t.name),
                      if (t.revoked) ...[
                        const SizedBox(width: 8),
                        Text(
                          'revoked',
                          style: TextStyle(
                            fontSize: 12,
                            color: theme.colorScheme.error,
                          ),
                        ),
                      ],
                    ],
                  ),
                  subtitle: Text(
                    'Scopes: ${t.scopes.join(', ')}'
                    '${t.expiresAt != null ? ' · expires ${t.expiresAt}' : ''}',
                    style: theme.textTheme.bodySmall,
                  ),
                  trailing: t.revoked
                      ? null
                      : IconButton(
                          tooltip: 'Revoke token',
                          icon: const Icon(Icons.delete_outline),
                          onPressed: () => _revoke(t),
                        ),
                ),
              ),
        ],
      ),
    );
  }
}
