import 'dart:convert';

import 'package:http/http.dart' as http;

import 'models.dart';

/// Error shape from the backend: {"error": "...", "message": "..."}.
class ApiException implements Exception {
  const ApiException(this.status, this.error, this.message);

  final int status;
  final String error;
  final String message;

  bool get isUnauthorized => status == 401;
  bool get isNotFound => status == 404;

  @override
  String toString() => message.isNotEmpty ? message : 'HTTP $status ($error)';
}

/// HTTP client for the rgit REST API (/api/v1, DESIGN.md §10).
///
/// - Base URL derives from the current origin (same-origin, so the browser
///   attaches the session cookie automatically).
/// - Mutating requests carry `X-Rgit-Csrf: 1` and a JSON content type.
/// - An optional PAT can be sent as `Authorization: Bearer <token>`.
class ApiClient {
  ApiClient({http.Client? httpClient, Uri? origin})
    : _http = httpClient ?? http.Client(),
      _base = (origin ?? Uri.base).resolve('/api/v1');

  final http.Client _http;
  final Uri _base;

  /// Optional personal access token for API auth.
  String? bearerToken;

  /// Invoked on any 401 response (e.g. session expired).
  void Function()? onUnauthorized;

  Uri _uri(String path, [Map<String, String>? query]) {
    final q = query == null || query.isEmpty ? null : query;
    return _base.replace(path: '${_base.path}$path', queryParameters: q);
  }

  Map<String, String> _headers({bool mutating = false}) => <String, String>{
    'Accept': 'application/json',
    if (mutating) 'Content-Type': 'application/json',
    if (mutating) 'X-Rgit-Csrf': '1',
    if (bearerToken != null) 'Authorization': 'Bearer $bearerToken',
  };

  Never _throw(http.Response res) {
    String error = 'error';
    String message = 'Request failed with status ${res.statusCode}';
    try {
      final body = jsonDecode(utf8.decode(res.bodyBytes));
      if (body is Map<String, dynamic>) {
        error = '${body['error'] ?? error}';
        message = '${body['message'] ?? message}';
      }
    } catch (_) {
      // Non-JSON error body: keep defaults.
    }
    if (res.statusCode == 401) onUnauthorized?.call();
    throw ApiException(res.statusCode, error, message);
  }

  dynamic _decode(http.Response res) {
    if (res.statusCode < 200 || res.statusCode >= 300) _throw(res);
    if (res.bodyBytes.isEmpty) return null;
    try {
      return jsonDecode(utf8.decode(res.bodyBytes));
    } on FormatException {
      throw ApiException(
        res.statusCode,
        'bad_response',
        'Server returned an invalid JSON response',
      );
    }
  }

  Future<dynamic> _get(String path, [Map<String, String>? query]) async =>
      _decode(await _http.get(_uri(path, query), headers: _headers()));

  Future<http.Response> _getRaw(
    String path, [
    Map<String, String>? query,
  ]) async {
    final res = await _http.get(
      _uri(path, query),
      headers: bearerToken != null
          ? {'Authorization': 'Bearer $bearerToken'}
          : const {},
    );
    if (res.statusCode < 200 || res.statusCode >= 300) _throw(res);
    return res;
  }

  Future<dynamic> _post(String path, [Object? body]) async => _decode(
    await _http.post(
      _uri(path),
      headers: _headers(mutating: true),
      body: jsonEncode(body ?? const <String, dynamic>{}),
    ),
  );

  Future<dynamic> _patch(String path, Object body) async => _decode(
    await _http.patch(
      _uri(path),
      headers: _headers(mutating: true),
      body: jsonEncode(body),
    ),
  );

  Future<dynamic> _delete(String path) async => _decode(
    await _http.delete(_uri(path), headers: _headers(mutating: true)),
  );

  Future<Paged<T>> _paged<T>(
    String path,
    Map<String, String> query,
    int page,
    T Function(Map<String, dynamic>) fromJson,
  ) async {
    final res = await _http.get(_uri(path, query), headers: _headers());
    final body = _decode(res);
    final items = (body as List? ?? const [])
        .whereType<Map<String, dynamic>>()
        .map(fromJson)
        .toList();
    final total = int.tryParse(res.headers['x-total'] ?? '');
    if (total == null || total < 0) {
      throw const ApiException(
        502,
        'bad_response',
        'Server returned an invalid pagination total',
      );
    }
    return Paged<T>(items: items, total: total, page: page);
  }

  static Map<String, dynamic> _map(dynamic v) =>
      v is Map<String, dynamic> ? v : <String, dynamic>{};

  static List<Map<String, dynamic>> _list(dynamic v) =>
      (v as List? ?? const []).whereType<Map<String, dynamic>>().toList();

  /// Encodes a project/group locator: numeric id or URL-encoded ns/path.
  static String encodeId(Object idOrPath) =>
      idOrPath is int ? '$idOrPath' : Uri.encodeComponent('$idOrPath');

  // ---- Instance status -----------------------------------------------------

  Future<int> uptimeSeconds() async {
    final body = _map(await _get('/status'));
    final value = body['uptime_seconds'];
    return switch (value) {
      int seconds => seconds,
      num seconds => seconds.toInt(),
      _ => int.tryParse('$value') ?? 0,
    };
  }

  // ---- Session -------------------------------------------------------------

  Future<User?> login(String login, String password) async {
    final body = await _post('/session', {
      'login': login,
      'password': password,
    });
    final map = _map(body);
    // The server may return the user directly or nested under "user".
    if (map.containsKey('username')) return User.fromJson(map);
    if (map['user'] is Map<String, dynamic>) {
      return User.fromJson(map['user'] as Map<String, dynamic>);
    }
    return null;
  }

  Future<void> logout() => _delete('/session');

  Future<User> currentUser() async => User.fromJson(_map(await _get('/user')));

  Future<User> updateProfile({String? name, String? email}) async =>
      User.fromJson(
        _map(await _patch('/user', {'name': ?name, 'email': ?email})),
      );

  Future<void> changePassword(String current, String next) => _post(
    '/user/password',
    {'current_password': current, 'new_password': next},
  );

  // ---- SSH keys ------------------------------------------------------------

  Future<List<SshKey>> listKeys() async =>
      _list(await _get('/user/keys')).map(SshKey.fromJson).toList();

  Future<SshKey> addKey({required String title, required String key}) async =>
      SshKey.fromJson(
        _map(await _post('/user/keys', {'title': title, 'key': key})),
      );

  Future<void> deleteKey(int id) => _delete('/user/keys/$id');

  // ---- Personal access tokens ----------------------------------------------

  Future<List<PersonalAccessToken>> listTokens() async => _list(
    await _get('/user/tokens'),
  ).map(PersonalAccessToken.fromJson).toList();

  /// Creates a token. The response is {"token": {row}, "plaintext": "rgit_…"};
  /// the returned model carries the one-time secret in `.plaintext`.
  Future<PersonalAccessToken> createToken({
    required String name,
    required List<String> scopes,
    String? expiresAt,
  }) async {
    final body = _map(
      await _post('/user/tokens', {
        'name': name,
        'scopes': scopes,
        'expires_at': ?expiresAt,
      }),
    );
    final row = _map(body['token']);
    return PersonalAccessToken.fromJson({
      ...row,
      'plaintext': body['plaintext'],
    });
  }

  Future<void> revokeToken(int id) => _delete('/user/tokens/$id');

  // ---- Projects ------------------------------------------------------------

  Future<Paged<Project>> listProjects({
    String? search,
    String? namespace,
    int? visibility,
    int page = 1,
    int perPage = 20,
  }) => _paged(
    '/projects',
    {
      if (search != null && search.isNotEmpty) 'search': search,
      if (namespace != null && namespace.isNotEmpty) 'namespace': namespace,
      if (visibility != null) 'visibility': '$visibility',
      'page': '$page',
      'per_page': '$perPage',
    },
    page,
    Project.fromJson,
  );

  Future<Paged<Project>> searchProjects(
    String query, {
    int page = 1,
    int perPage = 20,
  }) => listProjects(search: query, page: page, perPage: perPage);

  Future<Project> getProject(Object idOrPath) async =>
      Project.fromJson(_map(await _get('/projects/${encodeId(idOrPath)}')));

  Future<Project> createProject({
    required String name,
    required String path,
    int? namespaceId,
    int visibility = Visibility.private,
    String? description,
  }) async => Project.fromJson(
    _map(
      await _post('/projects', {
        'name': name,
        'path': path,
        'namespace_id': ?namespaceId,
        'visibility': visibility,
        'description': ?description,
      }),
    ),
  );

  Future<Project> updateProject(int id, Map<String, dynamic> fields) async =>
      Project.fromJson(_map(await _patch('/projects/$id', fields)));

  Future<void> deleteProject(int id) => _delete('/projects/$id');

  Future<Project> archiveProject(int id) async =>
      Project.fromJson(_map(await _post('/projects/$id/archive')));

  Future<Project> unarchiveProject(int id) async =>
      Project.fromJson(_map(await _post('/projects/$id/unarchive')));

  Future<Project> forkProject(
    int id, {
    int? namespaceId,
    String? path,
    String? name,
  }) async => Project.fromJson(
    _map(
      await _post('/projects/$id/fork', {
        'namespace_id': ?namespaceId,
        'path': ?path,
        'name': ?name,
      }),
    ),
  );

  Future<Project> transferProject(int id, int namespaceId) async =>
      Project.fromJson(
        _map(
          await _post('/projects/$id/transfer', {'namespace_id': namespaceId}),
        ),
      );

  // ---- Project members -----------------------------------------------------

  Future<List<Member>> listProjectMembers(int id) async =>
      _list(await _get('/projects/$id/members')).map(Member.fromJson).toList();

  /// Adds or updates a member (the backend POST is an upsert; 201, no body).
  Future<void> addProjectMember(
    int id, {
    required int userId,
    required int accessLevel,
  }) => _post('/projects/$id/members', {
    'user_id': userId,
    'access_level': accessLevel,
  });

  Future<void> removeProjectMember(int id, int userId) =>
      _delete('/projects/$id/members/$userId');

  // ---- Repository browsing -------------------------------------------------

  Future<Paged<TreeEntry>> tree(
    Object idOrPath, {
    String? ref,
    String? path,
    int page = 1,
    int perPage = 100,
  }) => _paged(
    '/projects/${encodeId(idOrPath)}/repository/tree',
    {
      'ref': ?ref,
      if (path != null && path.isNotEmpty) 'path': path,
      'page': '$page',
      'per_page': '$perPage',
    },
    page,
    TreeEntry.fromJson,
  );

  /// Blob metadata + content. The endpoint returns
  /// {path, ref, size, binary, content_base64}; text blobs are decoded here.
  Future<BlobFile> blob(
    Object idOrPath, {
    required String ref,
    required String path,
  }) async {
    final file = BlobFile.fromJson(
      _map(
        await _get('/projects/${encodeId(idOrPath)}/repository/blob', {
          'ref': ref,
          'path': path,
        }),
      ),
    );
    if (file.binary) return file;
    try {
      return file.withText(
        utf8.decode(
          base64Decode(file.contentBase64.trim()),
          allowMalformed: true,
        ),
      );
    } catch (_) {
      return file;
    }
  }

  Future<String> raw(
    Object idOrPath, {
    required String ref,
    required String path,
  }) async {
    final res = await _getRaw(
      '/projects/${encodeId(idOrPath)}/repository/raw',
      {'ref': ref, 'path': path},
    );
    return utf8.decode(res.bodyBytes, allowMalformed: true);
  }

  /// Absolute URL of the raw endpoint (for links/downloads).
  Uri rawUrl(Object idOrPath, {required String ref, required String path}) =>
      _uri('/projects/${encodeId(idOrPath)}/repository/raw', {
        'ref': ref,
        'path': path,
      });

  Future<Paged<CommitInfo>> commits(
    Object idOrPath, {
    String? ref,
    String? path,
    int page = 1,
    int perPage = 20,
  }) => _paged(
    '/projects/${encodeId(idOrPath)}/repository/commits',
    {
      'ref': ?ref,
      if (path != null && path.isNotEmpty) 'path': path,
      'page': '$page',
      'per_page': '$perPage',
    },
    page,
    CommitInfo.fromJson,
  );

  Future<CommitInfo> commit(Object idOrPath, String sha) async =>
      CommitInfo.fromJson(
        _map(
          await _get('/projects/${encodeId(idOrPath)}/repository/commits/$sha'),
        ),
      );

  /// Raw patch text for a commit.
  Future<String> diff(Object idOrPath, String sha) async {
    final res = await _getRaw(
      '/projects/${encodeId(idOrPath)}/repository/diff/$sha',
    );
    return utf8.decode(res.bodyBytes, allowMalformed: true);
  }

  Future<List<Branch>> branches(Object idOrPath) async => _list(
    await _get('/projects/${encodeId(idOrPath)}/repository/branches'),
  ).map(Branch.fromJson).toList();

  Future<List<Tag>> tags(Object idOrPath) async => _list(
    await _get('/projects/${encodeId(idOrPath)}/repository/tags'),
  ).map(Tag.fromJson).toList();

  /// Absolute URL of a tar.gz/zip archive download.
  Uri archiveUrl(
    Object idOrPath, {
    required String ref,
    String format = 'tar.gz',
  }) => _uri('/projects/${encodeId(idOrPath)}/repository/archive', {
    'ref': ref,
    'format': format,
  });

  Future<ReadmeFile?> readme(Object idOrPath, {String? ref}) async {
    try {
      final body = await _get(
        '/projects/${encodeId(idOrPath)}/repository/readme',
        {'ref': ?ref},
      );
      if (body == null) return null;
      return ReadmeFile.fromJson(_map(body));
    } on ApiException catch (e) {
      if (e.isNotFound) return null;
      rethrow;
    }
  }

  // ---- Groups ----------------------------------------------------------

  Future<List<Namespace>> listGroups() async =>
      _list(await _get('/groups')).map(Namespace.fromJson).toList();

  Future<Namespace> createGroup({
    required String name,
    required String path,
    String? description,
  }) async => Namespace.fromJson(
    _map(
      await _post('/groups', {
        'name': name,
        'path': path,
        'description': ?description,
      }),
    ),
  );

  Future<Namespace> getGroup(Object idOrPath) async =>
      Namespace.fromJson(_map(await _get('/groups/${encodeId(idOrPath)}')));

  Future<Namespace> updateGroup(
    Object idOrPath,
    Map<String, dynamic> fields,
  ) async => Namespace.fromJson(
    _map(await _patch('/groups/${encodeId(idOrPath)}', fields)),
  );

  Future<List<Member>> listGroupMembers(int id) async =>
      _list(await _get('/groups/$id/members')).map(Member.fromJson).toList();

  Future<void> deleteGroup(int id) => _delete('/groups/$id');

  /// Adds or updates a group member (the backend POST is an upsert).
  Future<void> addGroupMember(
    int id, {
    required int userId,
    required int accessLevel,
  }) => _post('/groups/$id/members', {
    'user_id': userId,
    'access_level': accessLevel,
  });

  Future<void> removeGroupMember(int id, int userId) =>
      _delete('/groups/$id/members/$userId');

  // ---- Admin ----------------------------------------------------------

  Future<Paged<User>> adminListUsers({int page = 1, int perPage = 20}) =>
      _paged(
        '/admin/users',
        {'page': '$page', 'per_page': '$perPage'},
        page,
        User.fromJson,
      );

  /// Creates a user. The backend requires an initial password and returns
  /// the created User JSON.
  Future<User> adminCreateUser({
    required String username,
    required String email,
    required String name,
    required String password,
    bool isAdmin = false,
  }) async => User.fromJson(
    _map(
      await _post('/admin/users', {
        'username': username,
        'email': email,
        'name': name,
        'password': password,
        'is_admin': isAdmin,
      }),
    ),
  );

  /// Fields: name/email/is_admin/state ("active"|"blocked")/password (reset).
  Future<User> adminUpdateUser(int id, Map<String, dynamic> fields) async =>
      User.fromJson(_map(await _patch('/admin/users/$id', fields)));

  Future<void> adminDeleteUser(int id) => _delete('/admin/users/$id');

  Future<Paged<Project>> adminListProjects({int page = 1, int perPage = 20}) =>
      _paged(
        '/admin/projects',
        {'page': '$page', 'per_page': '$perPage'},
        page,
        Project.fromJson,
      );

  Future<AdminStats> adminStats() async =>
      AdminStats.fromJson(_map(await _get('/admin/stats')));

  // ---- Clone URLs ------------------------------------------------------

  /// HTTPS clone URL for a project full path, from the current origin.
  String httpCloneUrl(String fullPath) {
    final origin = _base.replace(path: '', queryParameters: null);
    return '${origin.origin}/$fullPath.git';
  }

  /// Fallback SSH clone URL when older API responses omit the configured URL.
  String sshCloneUrl(String fullPath, {int port = 10022}) =>
      'ssh://git@${_base.host}:$port/$fullPath.git';
}
