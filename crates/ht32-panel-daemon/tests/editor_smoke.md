# Template-editor browser smoke checklist

The `/editor` page is a client-side Alpine app (no Rust unit tests cover its
runtime behaviour). Verify it manually, or with the MCP Playwright tools, against
a running daemon with the web server enabled (`[web] enable = true`).

Run a daemon locally without hardware (it serves the web UI in headless mode):

```
cargo run -p ht32-panel-daemon -- <config-with-web-enabled-on-127.0.0.1:PORT>
```

Then exercise these scenarios (all verified during M2 development against a local
headless daemon; console must stay clean — only a `/favicon.ico` 404 is expected):

1. **Load** — `GET /editor` returns 200; the palette renders `+ text / + bar /
   + gauge / + sparkline / + clock` (proves Alpine init + `/api/template-schema`
   fetch); the template dropdown lists saved templates (`/api/templates`).
2. **Add + select** — clicking a palette button adds a widget to the canvas and
   selects it; the property panel shows id / x / y / w / h and the kind-specific
   binding + colour controls.
3. **Client render** — bar/gauge/sparkline/clock render as their approximate
   shapes on the editing canvas (left); the server-truth PNG renders beside it.
4. **Property selects reflect the model** — for a fresh text widget the source
   dropdown shows `hostname` and the colour dropdown shows `primary` (not the
   first/`custom` option). (Regression: `:selected`-per-option, not `:value`.)
5. **Colour controls** — choosing `custom…` shows a colour picker; the chosen
   hex renders on BOTH the editing canvas and the server-truth preview. The wire
   value is an integer (e.g. `#ff8800` → `16746496`), never a `"#rrggbb"` string.
6. **Free text + text formats** — text source `literal` shows a text input; `time`
   / `date` show a format dropdown; `number` shows source + style dropdowns. Each
   produces a valid spec (Save returns 200 — the daemon's serde validates it).
7. **Custom background** — the top-bar background control (`inherit` / slot /
   `custom`) paints the whole canvas; the server preview renders it; `inherit`
   clears it back to the theme background.
8. **Drag / resize** — dragging a widget moves it (snapped to a 2px grid, clamped
   on-canvas); the corner handle resizes it.
9. **Overflow warnings** — a widget extending past the canvas, or text wider than
   its box, shows an amber warning under the canvas (non-blocking).
10. **Delete / new (regression)** — clicking *Delete widget*, or selecting
    `— new —`, sets `sel = null` and must log ZERO console errors (the property
    panel binds through the null-safe `cur` getter).
11. **Save + Activate** — Save writes the template (POST/PUT `/api/templates`);
    Activate POSTs the form-encoded `/face`; the panel shows the new face.

The authoritative check for any rendering question is the server-truth PNG
(`POST /api/templates/preview`) and, ultimately, the physical LCD — the client
canvas is an approximation only.
