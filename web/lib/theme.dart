import 'package:flutter/material.dart';

/// GitHub-like light/dark themes.
class RgitTheme {
  static const Color _lightAccent = Color(0xFF0969DA);
  static const Color _lightBg = Color(0xFFFFFFFF);
  static const Color _lightSubtle = Color(0xFFF6F8FA);
  static const Color _lightBorder = Color(0xFFD0D7DE);
  static const Color _lightText = Color(0xFF1F2328);

  static const Color _darkAccent = Color(0xFF2F81F7);
  static const Color _darkBg = Color(0xFF0D1117);
  static const Color _darkSubtle = Color(0xFF161B22);
  static const Color _darkBorder = Color(0xFF30363D);
  static const Color _darkText = Color(0xFFE6EDF3);

  static ThemeData light() => _base(
    brightness: Brightness.light,
    accent: _lightAccent,
    background: _lightBg,
    subtle: _lightSubtle,
    border: _lightBorder,
    text: _lightText,
  );

  static ThemeData dark() => _base(
    brightness: Brightness.dark,
    accent: _darkAccent,
    background: _darkBg,
    subtle: _darkSubtle,
    border: _darkBorder,
    text: _darkText,
  );

  static ThemeData _base({
    required Brightness brightness,
    required Color accent,
    required Color background,
    required Color subtle,
    required Color border,
    required Color text,
  }) {
    final scheme = ColorScheme.fromSeed(
      seedColor: accent,
      brightness: brightness,
      primary: accent,
      surface: background,
      onSurface: text,
      surfaceContainerHighest: subtle,
      outline: border,
    );
    final defaultTextTheme =
        ThemeData(
          useMaterial3: true,
          brightness: brightness,
          colorScheme: scheme,
        ).textTheme.apply(
          fontFamily: 'Roboto',
          bodyColor: text,
          displayColor: text,
        );
    final textTheme = defaultTextTheme.copyWith(
      bodyLarge: defaultTextTheme.bodyLarge?.copyWith(
        fontSize: 16,
        height: 1.5,
      ),
      bodyMedium: defaultTextTheme.bodyMedium?.copyWith(
        fontSize: 14,
        height: 1.5,
      ),
      bodySmall: defaultTextTheme.bodySmall?.copyWith(
        fontSize: 12,
        height: 1.5,
      ),
      labelLarge: defaultTextTheme.labelLarge?.copyWith(
        fontSize: 14,
        height: 1.35,
      ),
      labelMedium: defaultTextTheme.labelMedium?.copyWith(
        fontSize: 13,
        height: 1.35,
      ),
      titleSmall: defaultTextTheme.titleSmall?.copyWith(
        fontSize: 14,
        height: 1.35,
        fontWeight: FontWeight.w600,
      ),
    );
    return ThemeData(
      useMaterial3: true,
      fontFamily: 'Roboto',
      fontFamilyFallback: const ['Droid Sans Fallback'],
      brightness: brightness,
      colorScheme: scheme,
      textTheme: textTheme,
      scaffoldBackgroundColor: background,
      dividerColor: border,
      appBarTheme: AppBarTheme(
        backgroundColor: brightness == Brightness.light
            ? _lightSubtle
            : _darkSubtle,
        foregroundColor: text,
        elevation: 0,
        scrolledUnderElevation: 0,
        shape: Border(bottom: BorderSide(color: border)),
      ),
      cardTheme: CardThemeData(
        elevation: 0,
        color: background,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(6),
          side: BorderSide(color: border),
        ),
        margin: EdgeInsets.zero,
      ),
      inputDecorationTheme: InputDecorationTheme(
        isDense: true,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(6),
          borderSide: BorderSide(color: border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(6),
          borderSide: BorderSide(color: border),
        ),
      ),
      dividerTheme: DividerThemeData(color: border, space: 1),
    );
  }

  /// Monospace style for code views.
  static const TextStyle mono = TextStyle(
    fontFamily: 'Roboto Mono',
    fontSize: 13,
    height: 1.5,
  );
}
