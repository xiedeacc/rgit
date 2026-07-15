import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:rgit_web/api/client.dart';
import 'package:rgit_web/app.dart';
import 'package:rgit_web/pages/tree_page.dart';
import 'package:rgit_web/state/session.dart';

Widget _app(String location) {
  final api = ApiClient(origin: Uri.parse('http://localhost:8000/'));
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
}
