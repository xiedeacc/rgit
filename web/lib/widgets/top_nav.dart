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
    final width = MediaQuery.sizeOf(context).width;
    final compact = width < 700;
    final showSearch =
        width >= 520 && GoRouterState.of(context).uri.path != '/login';
    final currentQuery =
        GoRouterState.of(context).uri.queryParameters['q'] ?? '';
    return AppBar(
      automaticallyImplyLeading: false,
      titleSpacing: 16,
      title: Row(
        children: [
          InkWell(
            onTap: () => context.go('/'),
            child: const Text(
              'rgit',
              style: TextStyle(fontSize: 18, fontWeight: FontWeight.w700),
            ),
          ),
          if (showSearch)
            Expanded(
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 520),
                  child: SizedBox(
                    height: 34,
                    child: TextFormField(
                      key: ValueKey(currentQuery),
                      initialValue: currentQuery,
                      decoration: const InputDecoration(
                        hintText: 'Search projects',
                        prefixIcon: Icon(Icons.search, size: 18),
                        contentPadding: EdgeInsets.symmetric(vertical: 4),
                      ),
                      style: const TextStyle(fontSize: 14),
                      onFieldSubmitted: (q) {
                        final query = q.trim();
                        context.go(
                          Uri(
                            path: '/',
                            queryParameters: query.isEmpty
                                ? null
                                : {'q': query},
                          ).toString(),
                        );
                      },
                    ),
                  ),
                ),
              ),
            ),
          if (!showSearch) const Spacer(),
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
