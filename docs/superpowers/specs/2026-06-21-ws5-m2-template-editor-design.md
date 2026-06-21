# WS5 Milestone 2 — Web Template Editor (design)

Status: DESIGN · 2026-06-21 · fork: tekgnosis-net/ht32-panel

## Goal

Let a non-technical user **design their own LCD face in the browser** — add widgets
(text, bar, gauge, sparkline, clock), place and size them on a true-to-device canvas,
bind each to a live data source via dropdowns, pick colours, see a live preview that is
guaranteed to match the physical panel, and save + activate it. No JSON, no code.

This is the end-goal of WS5. Milestone 1 (the rendering pipeline: `TemplateSpec` +
resolvers + `TemplateFace`) is complete and hardware-verified on pve3 (`origin/main`
at `e3c88a2`). M2 adds the editor and the CRUD plumbing **on top of** that pipeline,
without changing the renderer.

## Audience & non-negotiables (decided during brainstorming)

- **Audience:** polished end-user feature. The editor hides the JSON entirely behind
  placement, dropdowns, and forgiving (non-blocking) validation.
- **Preview accuracy:** **client fidelity + server truth.** A smooth client-side
  approximate render for editing, AND a server-rendered PNG (the *real* `render_layout`)
  as the authoritative preview. When they disagree, the PNG wins. This directly answers
  the Phase-4 failure mode ("looked right on screen, garbled on hardware").
- **Layout:** three-column design-tool chrome (palette · device canvas · property panel)
  with a top bar (template selector · Save · Activate · refresh-true-preview).
- **Front-end stack:** no build step. HTMX (already in use) + **Alpine.js** (~15 KB, CDN)
  for the reactive property panel and editor state; vanilla pointer events for drag/resize;
  a small client widget renderer (`widgets.js`). HTMX/`fetch` for CRUD round-trips. Assets
  embedded/CDN like the existing UI — no Node toolchain, no new CI/packaging step.
- **Hardware acceptance is a standing rule:** every phase deploys to pve3 and is eyeballed
  on the physical panel (the M1 / Phase-4 lesson). Web `/lcd.png` is necessary but not
  sufficient.

## Architecture

The editor is a **new full-page route** (`/editor`), separate from the existing `/`
control panel (which stays HTMX-partial). Two render layers; the backend owns all logic.

```
BROWSER  (/editor?name=<name>)
  editor.html (Askama shell) — three-column chrome; loads HTMX + Alpine + editor JS
    editor.js  (Alpine component) — working spec (widgets[]), palette-add, drag/resize
                                     (pointer events, 2px grid), select→property-panel
                                     binding, debounced truth-preview, CRUD/activate calls
    widgets.js (pure client renderer) — widget → DOM/CSS (+ tiny <canvas> for gauge/
                                        sparkline). The smooth, APPROXIMATE editing canvas.

DAEMON  (Axum)
  JSON API  /api/template-schema   GET  → enum sets for dropdowns (generated from Rust)
            /api/templates         GET list · POST create
            /api/templates/{name}  GET load · PUT update · DELETE
            /api/templates/{name}/clone  POST
            /api/templates/preview POST draft spec → { png_base64, warnings[] }  (truth)
  HTML      /editor                GET  → editor page shell
  (reuse)   /face                  POST → activate (already template-aware)

  preview_render(spec, theme, orientation) -> (PNG, warnings)   PURE, off-screen
      reuses TemplateFace + render_layout; never touches live device/active face

  AppState template methods: save/update/delete/clone/list/load + validation
  D-Bus mirror: ListTemplates/GetTemplate/SaveTemplate/DeleteTemplate/CloneTemplate
                + TemplatesChanged signal (web list refreshes via existing SSE bridge)

  storage: <state_dir>/templates/<name>.json   (unchanged from M1)
```

### Component boundaries (each one job, independently testable)

| Unit | Responsibility | Depends on |
|------|----------------|-----------|
| `templates/editor.html` | three-column chrome + bootstrap | HTMX, Alpine |
| `editor.js` (Alpine) | editor state, drag/resize, CRUD/preview calls | the JSON API |
| `widgets.js` | approximate client render of each widget kind | nothing (pure) |

The editor's JS lives in its own files for readability (not inlined in the Askama
template), but is **embedded into the binary at compile time** (`include_str!`, the same
no-build ethos as the inline CSS today) and served from a small `GET /editor/<asset>`
route. No `static/` directory on disk, no bundler, no Node. Alpine.js loads from CDN like
HTMX. This keeps deb/rpm/nix packaging unchanged (single self-contained binary).
| template-CRUD routes (`web/`) | list/load/save/delete/clone/schema/preview marshalling | AppState |
| `preview_render` (pure fn) | `spec + SampleData + Theme → (PNG, warnings)` off-screen | `TemplateFace`, `render_layout` |
| AppState template methods | persist/validate/clone templates | `templates/` dir |
| D-Bus CRUD mirror | scriptable parity with web | AppState |

### Why these two choices matter

1. **`/api/template-schema` is the single source of truth for the editor's vocabulary.**
   The property-panel dropdowns (binding `NumberSource`/`HistorySource`/`TextSource`,
   `ThemeSlot`, `Align`, `TimeFmt`, `DateFmt`, `NumberFmt`, `ClockMode`, `ScaleMode`,
   widget `kind`) are generated from the Rust enums in
   `crates/ht32-panel-daemon/src/faces/template/spec.rs`. The editor can never offer a
   binding the daemon doesn't understand; adding a sensor enum variant grows the dropdown
   for free. A contract test asserts every variant appears in the schema response.
2. **`preview_render` is a pure function, decoupled from the live display.** It renders a
   fresh off-screen `Canvas` through the *real* `render_layout` against a fixed
   `SAMPLE_DATA`. It never touches the active face or the device, so it is safe to call on
   every keystroke, is deterministic, and cannot drift from hardware (it IS the hardware
   render path).

## Editing & preview UX

Two render layers, distinct roles:

- **Layer A — editing canvas (client, smooth, approximate).** A 170×320 / 320×170 `<div>`
  scaled ~2× for comfort. Each widget is an absolutely-positioned child rendered by
  `widgets.js`: Text/Clock → sized `<div>`; Bar → bg+fill divs; Gauge → small `<canvas>`
  arc; Sparkline → small `<canvas>` polyline; all fed by sample data so they look
  populated. Drag moves (pointer events, snapped to a 2px grid); corner handles resize.
  Position/size are bidirectional with the property panel (edit `x` → box moves).
- **Layer B — truth preview (server, exact).** An `<img>` of the real renderer's PNG of
  the current draft, refreshed **debounced ~400 ms** after edits (POST draft →
  `/api/templates/preview`) and on an explicit **"Refresh true preview"** button. The PNG
  is authoritative.

### Validation & forgiveness

- **Live, inline, non-blocking.** `/api/templates/preview` returns `{ png_base64, warnings[] }`.
  A widget whose text overflows its rect, or whose rect leaves the canvas, gets a yellow
  outline on the editing canvas + a plain-language warning ("CPU label is wider than its
  box"). Editing continues regardless.
- **Bounds clamping on drag** so a widget can't be dropped fully off-screen.
- **Warnings are computed in the render pass** (the same code that draws pixels), so a
  warning can never disagree with the rendered PNG. The browser only displays warnings,
  never computes them. This is where the M1-deferred overflow-hardening
  (resolve-time text measurement via `text_renderer.text_width`) lands.

## Backend API & D-Bus contract

JSON API (new `/api/...` namespace, distinct from the HTML-partial routes):

| Route | Method | Body → Response | Purpose |
|-------|--------|-----------------|---------|
| `/api/template-schema` | GET | → enum sets `{kinds, number_sources, history_sources, text_sources, theme_slots, aligns, time_fmts, date_fmts, number_fmts, clock_modes, scale_modes, orientations}` | Drives property-panel dropdowns. |
| `/api/templates` | GET | → `[{name}]` | List saved templates. |
| `/api/templates` | POST | `TemplateSpec` → `{name, warnings[]}` | Create new (name unique + allowlisted). |
| `/api/templates/{name}` | GET | → `TemplateSpec` | Load one for editing. |
| `/api/templates/{name}` | PUT | `TemplateSpec` → `{warnings[]}` | Update existing. |
| `/api/templates/{name}` | DELETE | → `204` | Delete (refused if active face). |
| `/api/templates/{name}/clone` | POST | `{new_name}` → `{name}` | Duplicate as a starting point. |
| `/api/templates/preview` | POST | `TemplateSpec` → `{png_base64, warnings[]}` | Truth render of a draft; no save/activate. |
| `/editor` | GET | → HTML | Editor page shell. |

Activation reuses the existing `POST /face` → `AppState::set_face(name)` (already
template-aware via `resolve_face`). No new activation path.

**`preview_render`** (the unit that makes server-truth safe):
```
preview_render(spec: &TemplateSpec, theme: &Theme, orientation: Orientation)
    -> (Vec<u8> /*PNG*/, Vec<Warning>)
  1. canvas = Canvas::new(orientation.dimensions())   // fresh, off-screen
  2. layout = TemplateFace::new(spec.clone()).layout(&canvas, &SAMPLE_DATA, theme, &none)
  3. warnings = check_bounds(&layout, &canvas)         // resolve-time overflow check
  4. render_layout(&mut canvas, &layout)               // the REAL M1 renderer
  5. (encode_png(canvas.pixels()), warnings)
```
`SAMPLE_DATA` is a fixed representative `SystemData` (full 60-sample histories, plausible
values) so previews are deterministic and populated. The spec's `orientation`/`theme`, if
set, override the daemon's current settings for the preview; otherwise the daemon's current
settings are used (so a preview matches what activation would show on this device).

**AppState template methods** mirror the existing setter pattern (validate → persist → log):
`save_template(spec)`, `update_template(name, spec)`, `delete_template(name)`,
`clone_template(name, new_name)`, plus existing `list_templates` / `load_template`. Name
validation reuses the M1 allowlist (`[A-Za-z0-9_-]+`). Deleting the active template is
refused (would orphan the face).

**D-Bus mirror** (parity, the spec's "D-Bus/web CRUD" requirement): `ListTemplates`,
`GetTemplate(name)`, `SaveTemplate(json)`, `DeleteTemplate(name)`, `CloneTemplate(name,
new_name)` — thin shells over the same `AppState` methods, emitting a new `TemplatesChanged`
signal so the web list refreshes live through the existing SSE bridge.

Both transports are thin shells over the same `AppState` methods, so CRUD validation,
persistence, and signal emission live in one place; the editor's JSON namespace stays
separate from the control panel's HTML-partial namespace (Alpine owns the DOM for the
editor; the server owns it for the control panel).

## Build order (each phase independently shippable + hardware-verified)

| Phase | Deliverable | Acceptance |
|-------|-------------|-----------|
| **M2.1 — CRUD backend** | AppState `save/update/delete/clone_template` + `/api/templates*` routes + D-Bus mirror + `TemplatesChanged` signal | Unit tests (save→load round-trip, name rejection, delete-active refused, clone); `curl` creates a template, `set_face` activates it on pve3 → eyes on panel. |
| **M2.2 — Preview + schema** | pure `preview_render` + `check_bounds` warnings, `/api/templates/preview`, `/api/template-schema` | Unit test: known spec → asserted pixel; overflow spec → expected warning; schema-contract test (every enum variant present). `curl` the preview PNG, view it. |
| **M2.3 — Editor shell + property panel** | `/editor` page, three-column chrome, palette→add, select→Alpine property panel driven by the schema, Save/Activate via API. No drag yet — numeric x/y/w/h fields. | In-browser: build the portrait template from scratch via forms, save, activate, confirm on pve3. |
| **M2.4 — Drag/resize + client render + live validation** | `widgets.js` approximate renderer, pointer drag/resize, debounced truth-preview, inline overflow warnings | Full WYSIWYG loop; design a *new* face by dragging, confirm truth-PNG matches pve3. |

M2.1–2.2 are pure backend (SDD-friendly, fast). M2.3–2.4 are the front-end build. By M2.3
the daemon side is proven, so any later problem is provably a browser problem — collapsing
the Phase-4-style ambiguity between render/transport and presentation.

## Testing strategy

- **Pure/unit (Rust, the bulk, runs in CI):** CRUD round-trips; name-allowlist rejection;
  delete-active refusal; `preview_render` pixel asserts + warning asserts; `/api/template-schema`
  shape.
- **Schema-contract test:** assert every binding-source / style enum variant appears in
  `/api/template-schema`, so adding a sensor can't silently desync the dropdowns.
- **Front-end (kept thin by design):** a couple of Playwright smoke tests (load `/editor`,
  add a widget, save, assert the POSTed JSON). MCP Playwright tools are available.
- **Hardware (non-negotiable):** each phase deploys to pve3 and is eyeballed.

## Error handling

- **Invalid name** (web or D-Bus) → `400` / D-Bus error with a plain-language message; the
  allowlist is the single gate.
- **Malformed spec** (bad JSON / unknown enum) → serde rejects → `400` naming the field.
  The editor can't normally produce this (closed dropdowns); the API is defensive for
  D-Bus/`curl` callers.
- **Preview of a broken draft** → renders what it can + returns warnings; never 500s on a
  user layout mistake.
- **Delete/rename the active template** → refused with a clear reason (would orphan the face).
- **Concurrent edits** (two browsers) → last-write-wins on save; the `TemplatesChanged`
  signal refreshes the other client's list. No locking — YAGNI for a single-user home daemon.

## Out of scope (YAGNI for v1)

- New widget kinds or data sources (M2 is an editor for the existing closed set; extend the
  set separately as sensors land).
- Undo/redo history, multi-select, grouping, alignment guides beyond a simple grid snap.
- Per-widget cadence editing (the renderer is full-redraw; cadence is not user-facing yet).
- Auth on the web server (unchanged from today; the daemon is a single-user LAN service —
  noted as a known posture, not introduced or worsened here).

## Notes / dependencies

- Builds entirely on the M1 pipeline; the renderer and transmission path are untouched
  (full-redraw, the only hardware-validated path).
- Folds in two M1 follow-ups where they're actually needed: overflow-hardening (as the
  `check_bounds` warnings) and wiring templates into the face list (templates already
  resolve via `resolve_face`; M2 surfaces them in the editor and, via the CRUD list, the
  control panel).
- Reference: M1 design `docs/design/2026-06-21-ws5-template-builder.md`; wire format
  `crates/ht32-panel-daemon/src/faces/template/spec.rs`; deploy runbook in agent memory.
