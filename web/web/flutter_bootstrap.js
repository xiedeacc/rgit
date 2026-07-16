{{flutter_js}}
{{flutter_build_config}}

_flutter.loader.load({
  config: {
    // Keep the renderer self-contained so the production CSP never needs a
    // third-party script or WebAssembly origin.
    canvasKitBaseUrl: "canvaskit/",
  },
});
