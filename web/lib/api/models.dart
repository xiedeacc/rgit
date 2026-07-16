/// Plain data models mirroring the rgit REST API.
///
/// Field names follow the actual backend responses
/// (crates/rgit-http/src/handlers/api/, crates/rgit-core/src/models.rs,
/// crates/rgit-git/src/read.rs); visibility and access_level use the
/// GitLab-compatible integer constants.
library;

import 'dart:convert';

int _asInt(dynamic v, [int fallback = 0]) =>
    v is int ? v : int.tryParse('$v') ?? fallback;

int? _asIntOrNull(dynamic v) =>
    v == null ? null : (v is int ? v : int.tryParse('$v'));

bool _asBool(dynamic v) => v == true || v == 1 || v == '1' || v == 'true';

String _asString(dynamic v, [String fallback = '']) =>
    v == null ? fallback : '$v';

/// Visibility levels (GitLab-compatible).
class Visibility {
  static const int private = 0;
  static const int internal = 10;
  static const int public = 20;

  static String label(int v) => switch (v) {
    public => 'Public',
    internal => 'Internal',
    _ => 'Private',
  };
}

/// Access levels (GitLab-compatible).
class AccessLevel {
  static const int guest = 10;
  static const int reporter = 20;
  static const int developer = 30;
  static const int maintainer = 40;
  static const int owner = 50;

  static const Map<int, String> labels = <int, String>{
    guest: 'Guest',
    reporter: 'Reporter',
    developer: 'Developer',
    maintainer: 'Maintainer',
    owner: 'Owner',
  };

  static String label(int v) => labels[v] ?? '$v';
}

class User {
  const User({
    required this.id,
    required this.username,
    required this.email,
    required this.name,
    required this.isAdmin,
    required this.state,
    this.createdAt,
    this.updatedAt,
  });

  final int id;
  final String username;
  final String email;
  final String name;
  final bool isAdmin;
  final String state;
  final String? createdAt;
  final String? updatedAt;

  bool get isActive => state == 'active';

  factory User.fromJson(Map<String, dynamic> json) => User(
    id: _asInt(json['id']),
    username: _asString(json['username']),
    email: _asString(json['email']),
    name: _asString(json['name']),
    isAdmin: _asBool(json['is_admin']),
    state: _asString(json['state'], 'active'),
    createdAt: json['created_at'] as String?,
    updatedAt: json['updated_at'] as String?,
  );
}

/// Namespace row: {id, path, name, kind, owner_user_id, description, ...}.
class Namespace {
  const Namespace({
    required this.id,
    required this.name,
    required this.path,
    required this.kind,
    this.ownerUserId,
    this.description = '',
    this.createdAt,
  });

  final int id;
  final String name;
  final String path;

  /// 'user' or 'group'.
  final String kind;
  final int? ownerUserId;
  final String description;
  final String? createdAt;

  bool get isGroup => kind == 'group';

  factory Namespace.fromJson(Map<String, dynamic> json) => Namespace(
    id: _asInt(json['id']),
    name: _asString(json['name']),
    path: _asString(json['path']),
    kind: _asString(json['kind'], 'user'),
    ownerUserId: _asIntOrNull(json['owner_user_id']),
    description: _asString(json['description']),
    createdAt: json['created_at'] as String?,
  );
}

/// Project row; rendered project JSON additionally carries full_path,
/// namespace_path, http_clone_url and ssh_clone_url (admin list omits them).
class Project {
  const Project({
    required this.id,
    required this.namespaceId,
    required this.name,
    required this.path,
    required this.fullPath,
    required this.visibility,
    required this.archived,
    this.description = '',
    this.defaultBranch,
    this.lfsEnabled = true,
    this.forkedFromProjectId,
    this.namespacePathField,
    this.httpCloneUrl,
    this.sshCloneUrl,
    this.createdAt,
    this.updatedAt,
  });

  final int id;
  final int namespaceId;
  final String name;
  final String path;

  /// "namespace/path"; falls back to [path] when the server omits it
  /// (e.g. admin project list).
  final String fullPath;
  final String description;

  /// 0 private / 10 internal / 20 public.
  final int visibility;
  final String? defaultBranch;
  final bool archived;
  final bool lfsEnabled;
  final int? forkedFromProjectId;
  final String? namespacePathField;
  final String? httpCloneUrl;
  final String? sshCloneUrl;
  final String? createdAt;
  final String? updatedAt;

  String get namespacePath =>
      namespacePathField ??
      (fullPath.contains('/') ? fullPath.split('/').first : fullPath);

  factory Project.fromJson(Map<String, dynamic> json) {
    final path = _asString(json['path']);
    final nsPath = json['namespace_path'] as String?;
    final full =
        json['full_path'] as String? ??
        (nsPath != null ? '$nsPath/$path' : path);
    return Project(
      id: _asInt(json['id']),
      namespaceId: _asInt(json['namespace_id']),
      name: _asString(json['name'], path),
      path: path,
      fullPath: full,
      description: _asString(json['description']),
      visibility: _asInt(json['visibility']),
      defaultBranch: json['default_branch'] as String?,
      archived: _asBool(json['archived']),
      lfsEnabled: json.containsKey('lfs_enabled')
          ? _asBool(json['lfs_enabled'])
          : true,
      forkedFromProjectId: _asIntOrNull(json['forked_from_project_id']),
      namespacePathField: nsPath,
      httpCloneUrl: json['http_clone_url'] as String?,
      sshCloneUrl: json['ssh_clone_url'] as String?,
      createdAt: json['created_at'] as String?,
      updatedAt: json['updated_at'] as String?,
    );
  }
}

/// Member list item: {user_id, username, name, access_level, created_at}.
class Member {
  const Member({
    required this.userId,
    required this.accessLevel,
    this.username,
    this.name,
    this.createdAt,
  });

  final int userId;
  final int accessLevel;
  final String? username;
  final String? name;
  final String? createdAt;

  factory Member.fromJson(Map<String, dynamic> json) => Member(
    userId: _asInt(json['user_id'], _asInt(json['id'])),
    accessLevel: _asInt(json['access_level']),
    username: json['username'] as String?,
    name: json['name'] as String?,
    createdAt: json['created_at'] as String?,
  );
}

/// Tree entry: {name, path, kind ("blob"|"tree"|"commit"), mode, sha, size?}.
class TreeEntry {
  const TreeEntry({
    required this.name,
    required this.path,
    required this.kind,
    this.sha,
    this.mode,
    this.size,
  });

  final String name;
  final String path;

  /// 'blob' | 'tree' | 'commit' (submodule).
  final String kind;
  final String? sha;
  final String? mode;
  final int? size;

  bool get isDir => kind == 'tree';
  bool get isSubmodule => kind == 'commit';

  factory TreeEntry.fromJson(Map<String, dynamic> json) => TreeEntry(
    name: _asString(json['name']),
    path: _asString(json['path'], _asString(json['name'])),
    kind: _asString(json['kind'], _asString(json['type'], 'blob')),
    sha: json['sha'] as String?,
    mode: json['mode'] as String?,
    size: _asIntOrNull(json['size']),
  );
}

/// Commit: {sha, author_name, author_email, authored_at, message, parents}.
class CommitInfo {
  const CommitInfo({
    required this.sha,
    required this.message,
    required this.authorName,
    required this.authorEmail,
    this.authoredAt,
    this.parents = const <String>[],
  });

  final String sha;
  final String message;
  final String authorName;
  final String authorEmail;
  final String? authoredAt;
  final List<String> parents;

  String get shortSha => sha.length > 8 ? sha.substring(0, 8) : sha;
  String get title => message.split('\n').first;

  factory CommitInfo.fromJson(Map<String, dynamic> json) => CommitInfo(
    sha: _asString(json['sha']),
    message: _asString(json['message']),
    authorName: _asString(json['author_name']),
    authorEmail: _asString(json['author_email']),
    authoredAt: json['authored_at'] as String?,
    parents:
        (json['parents'] as List?)?.map((e) => '$e').toList() ??
        const <String>[],
  );
}

/// Branch ref: {name, sha, target_sha?}.
class Branch {
  const Branch({required this.name, this.sha});

  final String name;
  final String? sha;

  factory Branch.fromJson(Map<String, dynamic> json) =>
      Branch(name: _asString(json['name']), sha: json['sha'] as String?);
}

/// Tag ref: {name, sha, target_sha?} (target_sha = peeled commit for
/// annotated tags).
class Tag {
  const Tag({required this.name, this.sha, this.targetSha});

  final String name;
  final String? sha;
  final String? targetSha;

  /// Commit the tag points at (peeled when annotated).
  String? get commitSha => targetSha ?? sha;

  factory Tag.fromJson(Map<String, dynamic> json) => Tag(
    name: _asString(json['name']),
    sha: json['sha'] as String?,
    targetSha: json['target_sha'] as String?,
  );
}

class SshKey {
  const SshKey({
    required this.id,
    required this.title,
    required this.fingerprintSha256,
    this.key,
    this.createdAt,
    this.lastUsedAt,
  });

  final int id;
  final String title;
  final String fingerprintSha256;
  final String? key;
  final String? createdAt;
  final String? lastUsedAt;

  factory SshKey.fromJson(Map<String, dynamic> json) => SshKey(
    id: _asInt(json['id']),
    title: _asString(json['title']),
    fingerprintSha256: _asString(json['fingerprint_sha256']),
    key: json['key'] as String?,
    createdAt: json['created_at'] as String?,
    lastUsedAt: json['last_used_at'] as String?,
  );
}

class PersonalAccessToken {
  const PersonalAccessToken({
    required this.id,
    required this.name,
    required this.scopes,
    this.expiresAt,
    this.revoked = false,
    this.createdAt,
    this.lastUsedAt,
    this.plaintext,
  });

  final int id;
  final String name;
  final List<String> scopes;
  final String? expiresAt;
  final bool revoked;
  final String? createdAt;
  final String? lastUsedAt;

  /// One-time plaintext ("rgit_..."); only present right after creation.
  final String? plaintext;

  /// The row's `scopes` column is a JSON-encoded string (e.g. '["api"]');
  /// tolerate a plain list too.
  static List<String> _parseScopes(dynamic v) {
    if (v is List) return v.map((e) => '$e').toList();
    if (v is String && v.isNotEmpty) {
      try {
        final decoded = jsonDecode(v);
        if (decoded is List) return decoded.map((e) => '$e').toList();
      } catch (_) {
        // Fall through.
      }
    }
    return const <String>[];
  }

  factory PersonalAccessToken.fromJson(Map<String, dynamic> json) =>
      PersonalAccessToken(
        id: _asInt(json['id']),
        name: _asString(json['name']),
        scopes: _parseScopes(json['scopes']),
        expiresAt: json['expires_at'] as String?,
        revoked: _asBool(json['revoked']),
        createdAt: json['created_at'] as String?,
        lastUsedAt: json['last_used_at'] as String?,
        plaintext: json['plaintext'] as String?,
      );
}

/// GET /api/v1/admin/stats:
/// {users, projects, groups, lfs_objects, lfs_bytes, version}.
class AdminStats {
  const AdminStats({
    required this.users,
    required this.projects,
    required this.groups,
    required this.lfsObjects,
    required this.lfsBytes,
    required this.version,
  });

  final int users;
  final int projects;
  final int groups;
  final int lfsObjects;
  final int lfsBytes;
  final String version;

  factory AdminStats.fromJson(Map<String, dynamic> json) => AdminStats(
    users: _asInt(json['users']),
    projects: _asInt(json['projects']),
    groups: _asInt(json['groups']),
    lfsObjects: _asInt(json['lfs_objects']),
    lfsBytes: _asInt(json['lfs_bytes']),
    version: _asString(json['version']),
  );
}

/// GET .../repository/blob:
/// {path, ref, size, binary, content_base64}.
class BlobFile {
  const BlobFile({
    required this.path,
    required this.ref,
    required this.size,
    required this.binary,
    required this.contentBase64,
    this.text,
  });

  final String path;
  final String ref;
  final int size;
  final bool binary;
  final String contentBase64;

  /// Decoded text content (filled by the API client for non-binary blobs).
  final String? text;

  BlobFile withText(String decoded) => BlobFile(
    path: path,
    ref: ref,
    size: size,
    binary: binary,
    contentBase64: contentBase64,
    text: decoded,
  );

  factory BlobFile.fromJson(Map<String, dynamic> json) => BlobFile(
    path: _asString(json['path']),
    ref: _asString(json['ref']),
    size: _asInt(json['size']),
    binary: _asBool(json['binary']),
    contentBase64: _asString(json['content_base64']),
  );
}

/// GET .../repository/readme: {path, content} (raw markdown).
class ReadmeFile {
  const ReadmeFile({required this.path, required this.content});

  final String path;
  final String content;

  factory ReadmeFile.fromJson(Map<String, dynamic> json) => ReadmeFile(
    path: _asString(json['path'], 'README.md'),
    content: _asString(json['content']),
  );
}

/// One page of results. `total` comes from the required X-Total header.
class Paged<T> {
  const Paged({required this.items, required this.total, required this.page});

  final List<T> items;
  final int total;
  final int page;
}
