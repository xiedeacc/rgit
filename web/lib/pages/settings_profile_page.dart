import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../state/session.dart';
import '../widgets/top_nav.dart';

/// User profile settings (route: /settings/profile).
class SettingsProfilePage extends StatefulWidget {
  const SettingsProfilePage({super.key});

  @override
  State<SettingsProfilePage> createState() => _SettingsProfilePageState();
}

class _SettingsProfilePageState extends State<SettingsProfilePage> {
  final _name = TextEditingController();
  final _email = TextEditingController();
  final _currentPassword = TextEditingController();
  final _newPassword = TextEditingController();
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    final user = context.read<SessionState>().user;
    _name.text = user?.name ?? '';
    _email.text = user?.email ?? '';
  }

  @override
  void dispose() {
    _name.dispose();
    _email.dispose();
    _currentPassword.dispose();
    _newPassword.dispose();
    super.dispose();
  }

  void _snack(String message) {
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _saveProfile() async {
    final session = context.read<SessionState>();
    setState(() => _busy = true);
    try {
      await session.api
          .updateProfile(name: _name.text, email: _email.text);
      await session.refresh();
      if (mounted) _snack('Profile updated.');
    } catch (e) {
      if (mounted) _snack('$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _changePassword() async {
    final session = context.read<SessionState>();
    setState(() => _busy = true);
    try {
      await session.api
          .changePassword(_currentPassword.text, _newPassword.text);
      _currentPassword.clear();
      _newPassword.clear();
      if (mounted) _snack('Password changed.');
    } catch (e) {
      if (mounted) _snack('$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final user = context.watch<SessionState>().user;
    return PageShell(
      maxWidth: 640,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('Profile',
              style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600)),
          const SizedBox(height: 4),
          if (user != null)
            Text('Signed in as ${user.username}',
                style: Theme.of(context).textTheme.bodySmall),
          const SizedBox(height: 16),
          TextField(
            controller: _name,
            decoration: const InputDecoration(labelText: 'Display name'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _email,
            decoration: const InputDecoration(labelText: 'Email'),
          ),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton(
              onPressed: _busy ? null : _saveProfile,
              child: const Text('Update profile'),
            ),
          ),
          const SizedBox(height: 32),
          const Divider(),
          const SizedBox(height: 16),
          const Text('Change password',
              style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
          const SizedBox(height: 12),
          TextField(
            controller: _currentPassword,
            obscureText: true,
            decoration:
                const InputDecoration(labelText: 'Current password'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _newPassword,
            obscureText: true,
            decoration: const InputDecoration(labelText: 'New password'),
          ),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton(
              onPressed: _busy ? null : _changePassword,
              child: const Text('Change password'),
            ),
          ),
        ],
      ),
    );
  }
}
