import 'package:flutter/material.dart';

/// Centered progress indicator used while a page loads.
class Loading extends StatelessWidget {
  const Loading({super.key});

  @override
  Widget build(BuildContext context) => const Center(
        child: Padding(
          padding: EdgeInsets.all(48),
          child: SizedBox(
            width: 28,
            height: 28,
            child: CircularProgressIndicator(strokeWidth: 2.5),
          ),
        ),
      );
}
