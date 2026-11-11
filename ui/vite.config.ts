import { defineConfig, type Plugin } from "vite";

/**
 * KaTeX's stylesheet lists every font as woff2, woff and ttf. WKWebView,
 * WebView2 and WebKitGTK all read woff2, so the other two copies would only
 * add about 1.2 MB of files that nothing loads.
 */
function katexWoff2Only(): Plugin {
  return {
    name: "katex-woff2-only",
    enforce: "pre",
    transform(code, id) {
      if (!id.endsWith("katex/dist/katex.min.css")) return null;
      return code.replace(/,url\([^)]*\.(?:woff|ttf)\)\s*format\("[^"]*"\)/g, "");
    },
  };
}

// Tauri needs a fixed dev port. Production assets bundle into ui/dist and are
// served by the Tauri custom protocol, never file:// or a runtime HTTP server.
export default defineConfig({
  clearScreen: false,
  plugins: [katexWoff2Only()],
  // Visual artifacts come from the visual: scheme in dev too, never from Vite.
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    outDir: "dist",
    // The CSP's font-src falls back to default-src 'self', which has no data:.
    // Vite would inline a font under 4 KB (KaTeX_Size3 is 3.6 KB) as a data:
    // URI and the webview would refuse it, so a font stays a file.
    assetsInlineLimit: (file) => (/\.(?:woff2?|ttf|otf)$/.test(file) ? false : undefined),
  },
});
