import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';

import 'pages/admin_dashboard_page.dart';
import 'pages/admin_projects_page.dart';
import 'pages/admin_users_page.dart';
import 'pages/blob_page.dart';
import 'pages/branches_page.dart';
import 'pages/commit_page.dart';
import 'pages/commits_page.dart';
import 'pages/explore_page.dart';
import 'pages/group_new_page.dart';
import 'pages/group_settings_page.dart';
import 'pages/login_page.dart';
import 'pages/namespace_page.dart';
import 'pages/project_home_page.dart';
import 'pages/project_settings_page.dart';
import 'pages/settings_keys_page.dart';
import 'pages/settings_profile_page.dart';
import 'pages/settings_tokens_page.dart';
import 'pages/tags_page.dart';
import 'pages/tree_page.dart';
import 'state/session.dart';
import 'theme.dart';

/// Extracts the ":path(.+)" wildcard parameter (go_router keeps a leading
/// slash when the wildcard is a sub-route).
String _restPath(GoRouterState state) {
  final raw = state.pathParameters['path'] ?? '';
  return raw.startsWith('/') ? raw.substring(1) : raw;
}

/// Root widget: MaterialApp.router + go_router route table (DESIGN.md §11).
class RgitApp extends StatefulWidget {
  const RgitApp({super.key, this.initialLocation = '/'});

  final String initialLocation;

  @override
  State<RgitApp> createState() => _RgitAppState();
}

class _RgitAppState extends State<RgitApp> {
  late final GoRouter _router = _buildRouter();

  GoRouter _buildRouter() {
    final session = context.read<SessionState>();
    return GoRouter(
      initialLocation: widget.initialLocation,
      refreshListenable: session,
      redirect: (context, state) {
        final path = state.uri.path;
        final signedIn = session.isSignedIn;
        final needsAuth = path.startsWith('/settings') ||
            path.startsWith('/admin') ||
            path == '/groups/new';
        if (needsAuth && session.ready && !signedIn) return '/login';
        if (path == '/login' && signedIn) return '/';
        return null;
      },
      routes: [
        // Fixed top-level routes first so /:ns cannot swallow them.
        GoRoute(
          path: '/login',
          builder: (context, state) => const LoginPage(),
        ),
        GoRoute(
          path: '/',
          builder: (context, state) =>
              ExplorePage(initialQuery: state.uri.queryParameters['q']),
        ),
        GoRoute(
          path: '/settings/profile',
          builder: (context, state) => const SettingsProfilePage(),
        ),
        GoRoute(
          path: '/settings/keys',
          builder: (context, state) => const SettingsKeysPage(),
        ),
        GoRoute(
          path: '/settings/tokens',
          builder: (context, state) => const SettingsTokensPage(),
        ),
        GoRoute(
          path: '/admin',
          builder: (context, state) => const AdminDashboardPage(),
          routes: [
            GoRoute(
              path: 'users',
              builder: (context, state) => const AdminUsersPage(),
            ),
            GoRoute(
              path: 'projects',
              builder: (context, state) => const AdminProjectsPage(),
            ),
          ],
        ),
        GoRoute(
          path: '/groups/new',
          builder: (context, state) => const GroupNewPage(),
        ),

        // Namespace home + group settings.
        GoRoute(
          path: '/:ns',
          builder: (context, state) =>
              NamespacePage(ns: state.pathParameters['ns']!),
          routes: [
            GoRoute(
              path: 'settings',
              builder: (context, state) =>
                  GroupSettingsPage(ns: state.pathParameters['ns']!),
            ),
          ],
        ),

        // Project routes. ":path(.+)" is go_router's spelling of "*path".
        GoRoute(
          path: '/:ns/:proj',
          builder: (context, state) => ProjectHomePage(
            ns: state.pathParameters['ns']!,
            proj: state.pathParameters['proj']!,
          ),
          routes: [
            GoRoute(
              path: 'tree/:ref',
              builder: (context, state) => TreePage(
                ns: state.pathParameters['ns']!,
                proj: state.pathParameters['proj']!,
                gitRef: state.pathParameters['ref']!,
                path: '',
              ),
              routes: [
                GoRoute(
                  path: ':path(.+)',
                  builder: (context, state) => TreePage(
                    ns: state.pathParameters['ns']!,
                    proj: state.pathParameters['proj']!,
                    gitRef: state.pathParameters['ref']!,
                    path: _restPath(state),
                  ),
                ),
              ],
            ),
            GoRoute(
              path: 'blob/:ref/:path(.+)',
              builder: (context, state) => BlobPage(
                ns: state.pathParameters['ns']!,
                proj: state.pathParameters['proj']!,
                gitRef: state.pathParameters['ref']!,
                path: _restPath(state),
              ),
            ),
            GoRoute(
              path: 'commits/:ref',
              builder: (context, state) => CommitsPage(
                ns: state.pathParameters['ns']!,
                proj: state.pathParameters['proj']!,
                gitRef: state.pathParameters['ref']!,
              ),
            ),
            GoRoute(
              path: 'commit/:sha',
              builder: (context, state) => CommitPage(
                ns: state.pathParameters['ns']!,
                proj: state.pathParameters['proj']!,
                sha: state.pathParameters['sha']!,
              ),
            ),
            GoRoute(
              path: 'branches',
              builder: (context, state) => BranchesPage(
                ns: state.pathParameters['ns']!,
                proj: state.pathParameters['proj']!,
              ),
            ),
            GoRoute(
              path: 'tags',
              builder: (context, state) => TagsPage(
                ns: state.pathParameters['ns']!,
                proj: state.pathParameters['proj']!,
              ),
            ),
            GoRoute(
              path: 'settings',
              builder: (context, state) => ProjectSettingsPage(
                ns: state.pathParameters['ns']!,
                proj: state.pathParameters['proj']!,
              ),
            ),
          ],
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      title: 'rgit',
      debugShowCheckedModeBanner: false,
      theme: RgitTheme.light(),
      darkTheme: RgitTheme.dark(),
      themeMode: ThemeMode.system,
      routerConfig: _router,
    );
  }
}
