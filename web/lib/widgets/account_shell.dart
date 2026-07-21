import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import '../state/session.dart';
import 'top_nav.dart';

enum AccountSection { settings, keys, tokens, admin }

class AccountShell extends StatelessWidget {
  const AccountShell({
    super.key,
    required this.selected,
    required this.child,
    this.maxContentWidth = 760,
  });

  final AccountSection selected;
  final Widget child;
  final double maxContentWidth;

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionState>();
    final user = session.user;
    return PageShell(
      maxWidth: 1180,
      child: LayoutBuilder(
        builder: (context, constraints) {
          final nav = _AccountSideNav(selected: selected);
          final content = ConstrainedBox(
            constraints: BoxConstraints(maxWidth: maxContentWidth),
            child: child,
          );
          if (constraints.maxWidth < 780) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (user != null) _SignedInSummary(username: user.username),
                nav,
                const SizedBox(height: 24),
                content,
              ],
            );
          }
          return Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                width: 232,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    if (user != null) _SignedInSummary(username: user.username),
                    nav,
                  ],
                ),
              ),
              const SizedBox(width: 32),
              Expanded(child: content),
            ],
          );
        },
      ),
    );
  }
}

class _SignedInSummary extends StatelessWidget {
  const _SignedInSummary({required this.username});

  final String username;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Row(
        children: [
          CircleAvatar(
            radius: 18,
            child: Text(username.isNotEmpty ? username[0].toUpperCase() : '?'),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              username,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
            ),
          ),
        ],
      ),
    );
  }
}

class _AccountSideNav extends StatelessWidget {
  const _AccountSideNav({required this.selected});

  final AccountSection selected;

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionState>();
    final items = <_AccountNavItem>[
      const _AccountNavItem(
        section: AccountSection.settings,
        label: 'Settings',
        icon: Icons.settings_outlined,
        route: '/settings/profile',
      ),
      const _AccountNavItem(
        section: AccountSection.keys,
        label: 'SSH keys',
        icon: Icons.key_outlined,
        route: '/settings/keys',
      ),
      const _AccountNavItem(
        section: AccountSection.tokens,
        label: 'Access tokens',
        icon: Icons.token_outlined,
        route: '/settings/tokens',
      ),
      if (session.isAdmin)
        const _AccountNavItem(
          section: AccountSection.admin,
          label: 'Admin area',
          icon: Icons.admin_panel_settings_outlined,
          route: '/admin',
        ),
    ];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final item in items)
          _AccountNavTile(item: item, selected: selected == item.section),
        const Divider(height: 24),
        const _AccountSignOutTile(),
      ],
    );
  }
}

class _AccountNavItem {
  const _AccountNavItem({
    required this.section,
    required this.label,
    required this.icon,
    required this.route,
  });

  final AccountSection section;
  final String label;
  final IconData icon;
  final String route;
}

class _AccountNavTile extends StatelessWidget {
  const _AccountNavTile({required this.item, required this.selected});

  final _AccountNavItem item;
  final bool selected;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final fg = selected
        ? theme.colorScheme.primary
        : theme.colorScheme.onSurface;
    return InkWell(
      borderRadius: BorderRadius.circular(6),
      onTap: () => context.go(item.route),
      child: Container(
        margin: const EdgeInsets.only(bottom: 2),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
        decoration: BoxDecoration(
          color: selected
              ? theme.colorScheme.primary.withValues(alpha: 0.08)
              : Colors.transparent,
          borderRadius: BorderRadius.circular(6),
          border: Border(
            left: BorderSide(
              width: 3,
              color: selected ? theme.colorScheme.primary : Colors.transparent,
            ),
          ),
        ),
        child: Row(
          children: [
            Icon(item.icon, size: 20, color: fg),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                item.label,
                style: TextStyle(
                  color: fg,
                  fontSize: 14,
                  fontWeight: selected ? FontWeight.w600 : FontWeight.w400,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _AccountSignOutTile extends StatelessWidget {
  const _AccountSignOutTile();

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(6),
      onTap: () async {
        await context.read<SessionState>().logout();
        if (context.mounted) context.go('/login');
      },
      child: const Padding(
        padding: EdgeInsets.symmetric(horizontal: 10, vertical: 9),
        child: Row(
          children: [
            Icon(Icons.logout, size: 20),
            SizedBox(width: 10),
            Text('Sign out', style: TextStyle(fontSize: 14)),
          ],
        ),
      ),
    );
  }
}
