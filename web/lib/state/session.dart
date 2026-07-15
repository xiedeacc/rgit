import 'package:flutter/foundation.dart';

import '../api/client.dart';
import '../api/models.dart';

/// Holds the current signed-in user (session cookie based).
class SessionState extends ChangeNotifier {
  SessionState(this.api) {
    api.onUnauthorized = _handleUnauthorized;
  }

  final ApiClient api;

  User? _user;
  bool _ready = false;

  User? get user => _user;
  bool get isSignedIn => _user != null;
  bool get isAdmin => _user?.isAdmin ?? false;

  /// True once the initial GET /user probe has completed.
  bool get ready => _ready;

  /// Probes the session cookie on startup.
  Future<void> bootstrap() async {
    try {
      _user = await api.currentUser();
    } catch (_) {
      _user = null;
    }
    _ready = true;
    notifyListeners();
  }

  Future<void> login(String login, String password) async {
    final fromLogin = await api.login(login, password);
    _user = fromLogin ?? await api.currentUser();
    _ready = true;
    notifyListeners();
  }

  Future<void> logout() async {
    try {
      await api.logout();
    } catch (_) {
      // Best effort; drop the local session either way.
    }
    _user = null;
    notifyListeners();
  }

  /// Re-fetches the current user (e.g. after a profile update).
  Future<void> refresh() async {
    try {
      _user = await api.currentUser();
    } on ApiException catch (e) {
      if (e.isUnauthorized) _user = null;
    }
    notifyListeners();
  }

  void _handleUnauthorized() {
    if (_user != null) {
      _user = null;
      notifyListeners();
    }
  }
}
