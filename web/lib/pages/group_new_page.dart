import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../api/client.dart';
import '../widgets/top_nav.dart';

/// New group form (route: /groups/new).
class GroupNewPage extends StatefulWidget {
  const GroupNewPage({super.key});

  @override
  State<GroupNewPage> createState() => _GroupNewPageState();
}

class _GroupNewPageState extends State<GroupNewPage> {
  final _name = TextEditingController();
  final _path = TextEditingController();
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _name.dispose();
    _path.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final group = await context.read<ApiClient>().createGroup(
            name: _name.text,
            path: _path.text.isNotEmpty ? _path.text : _name.text,
          );
      if (mounted) context.go('/${group.path}');
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return PageShell(
      maxWidth: 560,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('New group',
              style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600)),
          const SizedBox(height: 6),
          Text(
            'Groups are shared namespaces: projects under a group are '
            'accessible to all group members.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 16),
          if (_error != null) ...[
            Text(_error!,
                style: TextStyle(
                    color: Theme.of(context).colorScheme.error)),
            const SizedBox(height: 10),
          ],
          TextField(
            controller: _name,
            autofocus: true,
            decoration: const InputDecoration(labelText: 'Group name'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _path,
            decoration: const InputDecoration(
              labelText: 'Group path (URL, defaults to name)',
            ),
          ),
          const SizedBox(height: 16),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton(
              onPressed: _busy ? null : _submit,
              child: Text(_busy ? 'Creating…' : 'Create group'),
            ),
          ),
        ],
      ),
    );
  }
}
