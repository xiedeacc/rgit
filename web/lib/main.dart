import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'api/client.dart';
import 'app.dart';
import 'state/session.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  final api = ApiClient();
  final session = SessionState(api);
  session.bootstrap();
  runApp(
    MultiProvider(
      providers: [
        Provider<ApiClient>.value(value: api),
        ChangeNotifierProvider<SessionState>.value(value: session),
      ],
      child: const RgitApp(),
    ),
  );
}
