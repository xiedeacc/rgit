import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../build_info.dart';
import '../state/session.dart';
import '../pages/project_new_dialog.dart';

/// GitHub-style top navigation bar: logo, search, avatar menu.
class TopNav extends StatelessWidget implements PreferredSizeWidget {
  const TopNav({super.key});

  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionState>();
    final user = session.user;
    final compact = MediaQuery.sizeOf(context).width < 700;
    return AppBar(
      titleSpacing: 16,
      title: Row(
        children: [
          InkWell(
            onTap: () => context.go('/'),
            child: Row(
              children: const [
                Text(
                  '</>',
                  style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 18,
                    fontWeight: FontWeight.w700,
                  ),
                ),
                SizedBox(width: 7),
                Text(
                  'rgit',
                  style: TextStyle(fontSize: 18, fontWeight: FontWeight.w700),
                ),
              ],
            ),
          ),
          if (!compact) ...[
            const SizedBox(width: 16),
            SizedBox(
              width: 280,
              height: 34,
              child: TextField(
                decoration: const InputDecoration(
                  hintText: 'Search projects',
                  prefixIcon: Icon(Icons.search, size: 18),
                  contentPadding: EdgeInsets.symmetric(vertical: 4),
                ),
                style: const TextStyle(fontSize: 14),
                onSubmitted: (q) => context.go(
                  Uri(path: '/', queryParameters: {'q': q}).toString(),
                ),
              ),
            ),
          ],
        ],
      ),
      actions: [
        if (!compact && BuildInfo.label.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(right: 8),
            child: Center(
              child: SelectableText(
                BuildInfo.label,
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                  fontFamily: 'RobotoMono',
                  fontSize: 13,
                  letterSpacing: 0,
                ),
              ),
            ),
          ),
        if (user != null)
          PopupMenuButton<String>(
            tooltip: 'Create new',
            icon: const Icon(Icons.add),
            onSelected: (v) {
              if (v == 'new-project') {
                showNewProjectDialog(context);
              } else {
                context.go(v);
              }
            },
            itemBuilder: (_) => const [
              PopupMenuItem(value: 'new-project', child: Text('New project')),
              PopupMenuItem(value: '/groups/new', child: Text('New group')),
            ],
          ),
        if (user != null)
          PopupMenuButton<String>(
            tooltip: user.username,
            icon: CircleAvatar(
              radius: 14,
              child: Text(
                user.username.isNotEmpty ? user.username[0].toUpperCase() : '?',
                style: const TextStyle(fontSize: 13),
              ),
            ),
            onSelected: (v) async {
              if (v == 'signout') {
                await session.logout();
                if (context.mounted) context.go('/login');
              } else {
                context.go(v);
              }
            },
            itemBuilder: (_) => [
              PopupMenuItem(
                value: '/${user.username}',
                child: Text('Signed in as ${user.username}'),
              ),
              const PopupMenuDivider(),
              const PopupMenuItem(
                value: '/settings/profile',
                child: Text('Settings'),
              ),
              const PopupMenuItem(
                value: '/settings/keys',
                child: Text('SSH keys'),
              ),
              const PopupMenuItem(
                value: '/settings/tokens',
                child: Text('Access tokens'),
              ),
              if (user.isAdmin) ...const [
                PopupMenuDivider(),
                PopupMenuItem(value: '/admin', child: Text('Admin area')),
              ],
              const PopupMenuDivider(),
              const PopupMenuItem(value: 'signout', child: Text('Sign out')),
            ],
          )
        else
          Padding(
            padding: const EdgeInsets.only(right: 8),
            child: TextButton(
              onPressed: () => context.go('/login'),
              child: const Text('Sign in'),
            ),
          ),
        const SizedBox(width: 8),
      ],
    );
  }
}

/// Standard page scaffold: top nav + centered max-width content.
class PageShell extends StatelessWidget {
  const PageShell({super.key, required this.child, this.maxWidth = 1012});

  final Widget child;
  final double maxWidth;

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: const TopNav(),
    body: SingleChildScrollView(
      child: Center(
        child: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: maxWidth),
          child: Padding(padding: const EdgeInsets.all(24), child: child),
        ),
      ),
    ),
  );
}
