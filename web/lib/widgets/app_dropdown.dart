import 'dart:math' as math;

import 'package:flutter/material.dart';

class AppDropdownOption<T> {
  const AppDropdownOption({
    required this.value,
    required this.label,
    this.icon,
  });

  final T value;
  final String label;
  final IconData? icon;
}

class AppMenuSurface extends StatelessWidget {
  const AppMenuSurface({
    super.key,
    required this.child,
    this.borderRadius = 10,
    this.shadowAlpha = 0.10,
  });

  final Widget child;
  final double borderRadius;
  final double shadowAlpha;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      color: Colors.transparent,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: theme.colorScheme.surface,
          border: Border.all(color: theme.dividerColor),
          borderRadius: BorderRadius.circular(borderRadius),
          boxShadow: [
            BoxShadow(
              color: Colors.black.withValues(alpha: shadowAlpha),
              blurRadius: borderRadius >= 10 ? 18 : 14,
              offset: Offset(0, borderRadius >= 10 ? 8 : 6),
            ),
          ],
        ),
        child: ClipRRect(
          borderRadius: BorderRadius.circular(borderRadius),
          child: child,
        ),
      ),
    );
  }
}

class AppMenuTile extends StatelessWidget {
  const AppMenuTile({
    super.key,
    required this.icon,
    required this.label,
    required this.onTap,
    this.trailing,
  });

  final IconData? icon;
  final String label;
  final VoidCallback onTap;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        child: SizedBox(
          height: 40,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14),
            child: Row(
              children: [
                if (icon != null) ...[
                  Icon(
                    icon,
                    size: 18,
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                  const SizedBox(width: 12),
                ],
                Expanded(
                  child: Text(
                    label,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 14, height: 1.2),
                  ),
                ),
                if (trailing != null) ...[const SizedBox(width: 12), trailing!],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class AppDropdown<T> extends StatefulWidget {
  const AppDropdown({
    super.key,
    required this.value,
    required this.options,
    required this.onChanged,
    this.label,
    this.leadingIcon,
    this.width,
    this.menuWidth,
    this.maxMenuHeight = 320,
  });

  final T value;
  final List<AppDropdownOption<T>> options;
  final ValueChanged<T> onChanged;
  final String? label;
  final IconData? leadingIcon;
  final double? width;
  final double? menuWidth;
  final double maxMenuHeight;

  @override
  State<AppDropdown<T>> createState() => _AppDropdownState<T>();
}

class _AppDropdownState<T> extends State<AppDropdown<T>> {
  OverlayEntry? _entry;

  @override
  void dispose() {
    _hide();
    super.dispose();
  }

  void _hide() {
    _entry?.remove();
    _entry = null;
  }

  void _toggle(BuildContext anchorContext) {
    if (_entry != null) {
      _hide();
      return;
    }
    final overlay = Overlay.of(context);
    final renderBox = anchorContext.findRenderObject() as RenderBox;
    final anchor = renderBox.localToGlobal(Offset.zero);
    final size = renderBox.size;
    final screen = MediaQuery.sizeOf(context);
    final requestedMenuWidth = widget.menuWidth ?? math.max(size.width, 180);
    final menuWidth = math.min(
      requestedMenuWidth,
      math.max(0.0, screen.width - 16),
    );
    final menuHeight = math.min(
      widget.maxMenuHeight,
      math.max(40.0, widget.options.length * 40.0),
    );
    final maxLeft = math.max(8.0, screen.width - menuWidth - 8);
    final left = anchor.dx.clamp(8.0, maxLeft);
    final belowTop = anchor.dy + size.height + 8;
    final top = belowTop + menuHeight <= screen.height - 8
        ? belowTop
        : math.max(8.0, anchor.dy - menuHeight - 8);

    _entry = OverlayEntry(
      builder: (overlayContext) => Stack(
        children: [
          Positioned.fill(
            child: GestureDetector(
              behavior: HitTestBehavior.translucent,
              onTap: _hide,
              child: const SizedBox.expand(),
            ),
          ),
          Positioned(
            left: left,
            top: top,
            width: menuWidth,
            child: AppMenuSurface(
              child: ConstrainedBox(
                constraints: BoxConstraints(maxHeight: widget.maxMenuHeight),
                child: SingleChildScrollView(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      for (var i = 0; i < widget.options.length; i++) ...[
                        _optionTile(widget.options[i]),
                        if (i != widget.options.length - 1)
                          Divider(
                            height: 1,
                            color: Theme.of(context).dividerColor,
                          ),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
    overlay.insert(_entry!);
  }

  Widget _optionTile(AppDropdownOption<T> option) {
    final selected = option.value == widget.value;
    return AppMenuTile(
      icon: option.icon,
      label: option.label,
      trailing: selected
          ? Icon(
              Icons.check,
              size: 16,
              color: Theme.of(context).colorScheme.primary,
            )
          : null,
      onTap: () {
        _hide();
        if (!selected) widget.onChanged(option.value);
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final selected = widget.options.firstWhere(
      (option) => option.value == widget.value,
      orElse: () =>
          AppDropdownOption(value: widget.value, label: '${widget.value}'),
    );
    final field = Builder(
      builder: (anchorContext) => MouseRegion(
        cursor: SystemMouseCursors.click,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => _toggle(anchorContext),
          child: DecoratedBox(
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surface,
              border: Border.all(color: Theme.of(context).dividerColor),
              borderRadius: BorderRadius.circular(8),
            ),
            child: SizedBox(
              width: widget.width ?? 220,
              height: 40,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 14),
                child: Row(
                  children: [
                    if (widget.leadingIcon != null) ...[
                      Icon(widget.leadingIcon, size: 18),
                      const SizedBox(width: 12),
                    ],
                    Expanded(
                      child: Text(
                        selected.label,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontSize: 14, height: 1.2),
                      ),
                    ),
                    const SizedBox(width: 12),
                    const Icon(Icons.arrow_drop_down, size: 18),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );

    if (widget.label == null) return field;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          widget.label!,
          style: TextStyle(
            color: Theme.of(context).colorScheme.onSurfaceVariant,
            fontSize: 12,
            height: 1.2,
          ),
        ),
        const SizedBox(height: 6),
        field,
      ],
    );
  }
}
