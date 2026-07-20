class BuildInfo {
  static const revision = String.fromEnvironment('RGIT_BUILD_REV');
  static const committedAt = String.fromEnvironment('RGIT_BUILD_TIME');

  static String get label {
    if (revision.isEmpty || committedAt.isEmpty) {
      return '';
    }
    return '$revision · $committedAt';
  }
}
