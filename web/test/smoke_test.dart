import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:provider/provider.dart';
import 'package:rgit_web/api/client.dart';
import 'package:rgit_web/api/models.dart';
import 'package:rgit_web/app.dart';
import 'package:rgit_web/pages/tree_page.dart';
import 'package:rgit_web/state/session.dart';
import 'package:rgit_web/widgets/clone_url_box.dart';
import 'package:rgit_web/widgets/top_nav.dart';

Widget _app(String location) {
  final api = ApiClient(origin: Uri.parse('http://localhost:8000/'));
  return _appWithApi(location, api);
}

Widget _appWithApi(String location, ApiClient api) {
  final session = SessionState(api);
  return MultiProvider(
    providers: [
      Provider<ApiClient>.value(value: api),
      ChangeNotifierProvider<SessionState>.value(value: session),
    ],
    child: RgitApp(initialLocation: location),
  );
}

void main() {
  testWidgets('app shell builds (login page)', (tester) async {
    await tester.pumpWidget(_app('/login'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in to rgit'), findsOneWidget);

    final fields = tester
        .widgetList<TextField>(find.byType(TextField))
        .toList(growable: false);
    expect(fields, hasLength(2));
    expect(fields[0].autofillHints, const [AutofillHints.username]);
    expect(fields[0].textInputAction, TextInputAction.next);
    expect(fields[1].autofillHints, const [AutofillHints.password]);
    expect(fields[1].textInputAction, TextInputAction.done);
    expect(
      tester.widget<AutofillGroup>(find.byType(AutofillGroup)).onDisposeAction,
      AutofillContextAction.cancel,
    );
  });

  testWidgets('app shell fits a mobile viewport', (tester) async {
    await tester.binding.setSurfaceSize(const Size(390, 844));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_app('/login'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in to rgit'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('successful login asks the browser to save credentials', (
    tester,
  ) async {
    final client = MockClient((request) async {
      if (request.url.path.endsWith('/session')) {
        return http.Response(
          '{"id":1,"username":"root","email":"root@example.test",'
          '"name":"Root","is_admin":true,"state":"active"}',
          200,
          headers: {'content-type': 'application/json'},
        );
      }
      if (request.url.path.endsWith('/projects')) {
        return http.Response(
          '[]',
          200,
          headers: {'content-type': 'application/json', 'x-total': '0'},
        );
      }
      return http.Response('{}', 404);
    });
    final api = ApiClient(
      httpClient: client,
      origin: Uri.parse('https://rgit.example.test/'),
    );

    await tester.pumpWidget(_appWithApi('/login', api));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).at(0), 'root');
    await tester.enterText(find.byType(TextField).at(1), 'correct-password');
    tester.testTextInput.log.clear();

    await tester.tap(find.widgetWithText(FilledButton, 'Sign in'));
    await tester.pumpAndSettle();

    expect(
      tester.testTextInput.log,
      contains(
        isMethodCall('TextInput.finishAutofillContext', arguments: true),
      ),
    );
  });

  testWidgets('wildcard tree route matches nested paths', (tester) async {
    await tester.pumpWidget(_app('/ns/proj/tree/main/src/lib'));
    await tester.pumpAndSettle();
    final page = tester.widget<TreePage>(find.byType(TreePage));
    expect(page.ns, 'ns');
    expect(page.proj, 'proj');
    expect(page.gitRef, 'main');
    expect(page.path, 'src/lib');
  });

  testWidgets('new group route exposes the complete form', (tester) async {
    await tester.pumpWidget(_app('/groups/new'));
    await tester.pump();

    expect(find.text('New group'), findsOneWidget);
    expect(find.widgetWithText(TextField, 'Group name'), findsOneWidget);
    expect(find.widgetWithText(TextField, 'Group path'), findsOneWidget);
    expect(find.widgetWithText(TextField, 'Description'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Create group'), findsOneWidget);
  });

  testWidgets('explore keeps one centered top search field', (tester) async {
    await tester.binding.setSurfaceSize(const Size(1280, 720));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    final api = ApiClient(
      httpClient: MockClient(
        (_) async => http.Response(
          '[]',
          200,
          headers: {'content-type': 'application/json', 'x-total': '0'},
        ),
      ),
      origin: Uri.parse('https://rgit.example.test/'),
    );

    await tester.pumpWidget(_appWithApi('/', api));
    await tester.pumpAndSettle();

    final searchFields = tester
        .widgetList<TextField>(find.byType(TextField))
        .where((field) => field.decoration?.hintText == 'Search projects')
        .toList(growable: false);
    expect(searchFields, hasLength(1));
    expect(find.text('Search projects…'), findsNothing);
    expect(find.text('</>'), findsNothing);
    expect(find.text('rgit'), findsOneWidget);
  });

  testWidgets('project tab routes do not animate', (tester) async {
    await tester.pumpWidget(_app('/ns/proj/commits/main'));
    await tester.pump();

    final pages = tester
        .widgetList<Navigator>(find.byType(Navigator))
        .expand((navigator) => navigator.pages)
        .whereType<NoTransitionPage<void>>()
        .toList(growable: false);
    expect(pages, hasLength(2));
  });

  testWidgets('explore route does not animate', (tester) async {
    await tester.pumpWidget(_app('/'));
    await tester.pump();

    final pages = tester
        .widgetList<Navigator>(find.byType(Navigator))
        .expand((navigator) => navigator.pages)
        .whereType<NoTransitionPage<void>>()
        .toList(growable: false);
    expect(pages, hasLength(1));
  });

  testWidgets('project child routes do not show an app bar back button', (
    tester,
  ) async {
    await tester.pumpWidget(_app('/ns/proj/commits/main'));
    await tester.pump();

    expect(find.byType(BackButton), findsNothing);
  });

  testWidgets('project code page shows latest commit summary', (tester) async {
    final api = ApiClient(
      httpClient: MockClient((request) async {
        final path = request.url.path;
        if (path.endsWith('/user')) {
          return http.Response('{}', 401);
        }
        if (path.endsWith('/projects/ns%2Fproj')) {
          return http.Response(
            '{"id":1,"namespace_id":1,"name":"Proj","path":"proj",'
            '"full_path":"ns/proj","namespace_path":"ns","visibility":0,'
            '"archived":false,"default_branch":"main"}',
            200,
            headers: {'content-type': 'application/json'},
          );
        }
        if (path.endsWith('/repository/branches')) {
          return http.Response(
            '[{"name":"main","sha":"0123456789abcdef"}]',
            200,
            headers: {'content-type': 'application/json'},
          );
        }
        if (path.endsWith('/repository/tree')) {
          return http.Response(
            '[{"name":"README.md","path":"README.md","kind":"blob",'
            '"latest_commit":{"sha":"fedcba9876543210",'
            '"message":"document install flow\\n",'
            '"author_name":"dev","author_email":"dev@example.test",'
            '"authored_at":"2026-07-21T12:00:00+00:00","parents":[]}}]',
            200,
            headers: {'content-type': 'application/json', 'x-total': '1'},
          );
        }
        if (path.endsWith('/repository/commits')) {
          return http.Response(
            '[{"sha":"0123456789abcdef","message":"fix ui\\n",'
            '"author_name":"dev","author_email":"dev@example.test",'
            '"authored_at":"2026-07-21T12:00:00+00:00","parents":[]}]',
            200,
            headers: {'content-type': 'application/json', 'x-total': '5'},
          );
        }
        if (path.endsWith('/repository/readme')) {
          return http.Response('{}', 404);
        }
        return http.Response('{}', 404);
      }),
      origin: Uri.parse('https://rgit.example.test/'),
    );

    await tester.pumpWidget(_appWithApi('/ns/proj', api));
    await tester.pumpAndSettle();

    expect(find.text('dev'), findsOneWidget);
    expect(find.text('fix ui'), findsOneWidget);
    expect(find.text('01234567'), findsOneWidget);
    expect(find.text('2026-07-21 20:00:00'), findsOneWidget);
    expect(find.text('01234567 · 2026-07-21 20:00:00'), findsNothing);
    expect(find.text('5 Commits'), findsOneWidget);
    expect(find.text('document install flow'), findsOneWidget);
  });

  testWidgets('tree branch pages keep the latest commit header', (
    tester,
  ) async {
    final api = ApiClient(
      httpClient: MockClient((request) async {
        final path = request.url.path;
        if (path.endsWith('/user')) {
          return http.Response('{}', 401);
        }
        if (path.endsWith('/projects/ns%2Fproj')) {
          return http.Response(
            '{"id":1,"namespace_id":1,"name":"Proj","path":"proj",'
            '"full_path":"ns/proj","namespace_path":"ns","visibility":0,'
            '"archived":false,"default_branch":"main"}',
            200,
            headers: {'content-type': 'application/json'},
          );
        }
        if (path.endsWith('/repository/tree')) {
          expect(request.url.queryParameters['ref'], 'config_dns');
          return http.Response(
            '[{"name":"config.toml","path":"config.toml","kind":"blob"}]',
            200,
            headers: {'content-type': 'application/json', 'x-total': '1'},
          );
        }
        if (path.endsWith('/repository/commits')) {
          expect(request.url.queryParameters['ref'], 'config_dns');
          return http.Response(
            '[{"sha":"9d3a1415abcdef00","message":"branch config\\n",'
            '"author_name":"ops","author_email":"ops@example.test",'
            '"authored_at":"2026-07-21T03:04:05+00:00","parents":[]}]',
            200,
            headers: {'content-type': 'application/json', 'x-total': '34'},
          );
        }
        return http.Response('{}', 404);
      }),
      origin: Uri.parse('https://rgit.example.test/'),
    );

    await tester.pumpWidget(_appWithApi('/ns/proj/tree/config_dns', api));
    await tester.pumpAndSettle();

    expect(find.text('ops'), findsOneWidget);
    expect(find.text('branch config'), findsOneWidget);
    expect(find.text('9d3a1415'), findsOneWidget);
    expect(find.text('2026-07-21 11:04:05'), findsOneWidget);
    expect(find.text('34 Commits'), findsOneWidget);
    expect(find.text('config.toml'), findsOneWidget);
  });

  test('project preserves server-configured clone URLs', () {
    final project = Project.fromJson({
      'id': 1,
      'namespace_id': 2,
      'name': 'Demo',
      'path': 'demo',
      'full_path': 'team/demo',
      'visibility': 0,
      'archived': false,
      'http_clone_url': 'https://code.example.test/team/demo.git',
      'ssh_clone_url': 'ssh://git@code.example.test:10022/team/demo.git',
    });

    expect(project.httpCloneUrl, 'https://code.example.test/team/demo.git');
    expect(
      project.sshCloneUrl,
      'ssh://git@code.example.test:10022/team/demo.git',
    );
  });

  testWidgets('clone URL box defaults to SSH', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: CloneUrlBox(
            httpsUrl: 'https://code.example.test/team/demo.git',
            sshUrl: 'ssh://git@code.example.test:10022/team/demo.git',
          ),
        ),
      ),
    );

    expect(
      find.text('ssh://git@code.example.test:10022/team/demo.git'),
      findsOneWidget,
    );
    expect(find.text('https://code.example.test/team/demo.git'), findsNothing);
  });

  test('paged API uses the server total without changing it by page', () async {
    final client = MockClient((request) async {
      expect(request.url.queryParameters['page'], '4');
      expect(request.url.queryParameters['per_page'], '20');
      return http.Response(
        '[{"id":1,"namespace_id":1,"name":"Demo","path":"demo",'
        '"full_path":"team/demo","visibility":20,"archived":false}]',
        200,
        headers: {'x-total': '95', 'content-type': 'application/json'},
      );
    });
    final api = ApiClient(
      httpClient: client,
      origin: Uri.parse('https://rgit.example.test/'),
    );

    final page = await api.listProjects(page: 4, perPage: 20);
    expect(page.page, 4);
    expect(page.total, 95);
    expect(page.items, hasLength(1));
  });

  test('paged API rejects a missing total header', () async {
    final api = ApiClient(
      httpClient: MockClient((_) async => http.Response('[]', 200)),
      origin: Uri.parse('https://rgit.example.test/'),
    );

    await expectLater(
      api.listProjects(),
      throwsA(
        isA<ApiException>().having(
          (error) => error.error,
          'error',
          'bad_response',
        ),
      ),
    );
  });

  test('status API returns uptime seconds', () async {
    final api = ApiClient(
      httpClient: MockClient((request) async {
        expect(request.url.path, '/api/v1/status');
        return http.Response(
          '{"uptime_seconds":3661}',
          200,
          headers: {'content-type': 'application/json'},
        );
      }),
      origin: Uri.parse('https://rgit.example.test/'),
    );

    expect(await api.uptimeSeconds(), 3661);
  });

  test('uptime display elides larger units until needed', () {
    expect(formatUptimeSeconds(3661), '01:01:01');
    expect(formatUptimeSeconds(86401), '01 00:00:01');
    expect(formatUptimeSeconds(30 * Duration.secondsPerDay), '01-00 00:00:00');
    expect(
      formatUptimeSeconds(365 * Duration.secondsPerDay),
      '01-00-00 00:00:00',
    );
  });
}
