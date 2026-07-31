class ResolvedRefPath {
  const ResolvedRefPath({required this.ref, required this.path});

  final String ref;
  final String path;
}

ResolvedRefPath resolveRefPath(String routeTail, Iterable<String> refs) {
  final tail = routeTail.replaceFirst(RegExp(r'^/+'), '');
  if (tail.isEmpty) return const ResolvedRefPath(ref: '', path: '');

  final sortedRefs = refs.where((ref) => ref.isNotEmpty).toList()
    ..sort((a, b) => b.length.compareTo(a.length));
  for (final ref in sortedRefs) {
    if (tail == ref) return ResolvedRefPath(ref: ref, path: '');
    if (tail.startsWith('$ref/')) {
      return ResolvedRefPath(ref: ref, path: tail.substring(ref.length + 1));
    }
  }

  final slash = tail.indexOf('/');
  if (slash < 0) return ResolvedRefPath(ref: tail, path: '');
  return ResolvedRefPath(
    ref: tail.substring(0, slash),
    path: tail.substring(slash + 1),
  );
}
