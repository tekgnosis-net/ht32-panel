# WS5 Milestone 2 — Web Template Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A browser drag-drop editor that lets a non-technical user compose, preview (matching the real panel), save, and activate a custom LCD face, persisted as a JSON `TemplateSpec` the daemon already renders.

**Architecture:** A new `/editor` page (separate from the existing `/` HTMX control panel) backed by a JSON `/api/templates*` CRUD surface, a `/api/template-schema` endpoint that generates the property-panel dropdowns from the Rust enums, and a pure off-screen `preview_render` that reuses the Milestone-1 `TemplateFace` + `render_layout` so previews are pixel-identical to hardware. A D-Bus CRUD mirror gives scriptable parity. The front end is no-build (HTMX + Alpine.js from CDN; editor JS embedded via `include_str!`).

**Tech Stack:** Rust, Axum, Askama, zbus (D-Bus), serde/serde_json, the `png` crate, tiny-skia (via the existing `Canvas`); HTMX + Alpine.js (CDN) + vanilla JS on the front end; Playwright (MCP) for browser smoke tests.

## Global Constraints

- No author email anywhere (Cargo.toml, nfpm, headers) — maintainer is `tekgnosis-net`, no email. (Project-wide rule.)
- No new heavy/runtime dependencies and **no third-party CDN loads for the editor**: Alpine.js is **vendored** into `assets/alpine.min.js` and embedded with `include_str!` (served from `/editor/alpine.js`), exactly like the editor's own JS. This removes the Subresource-Integrity / CDN-compromise risk entirely (nothing is fetched from a third party) and lets the editor work on an air-gapped LAN. No Node/bundler; packaging (deb/rpm/nix) stays a single self-contained binary. (The existing `/` control panel's HTMX-from-CDN is a pre-existing pattern, out of scope here.)
- The renderer and USB transmission path are **untouched**: full-redraw only (`partial_updates` stays default-false). M2 adds rendering *inputs* (templates), never changes how pixels reach the panel.
- Template name allowlist is the single validation gate everywhere: `name` must be non-empty and match `[A-Za-z0-9_-]+` (no `.`/`/`/`\`). Reuse the M1 rule; never construct a path from an unvalidated name.
- Templates persist as JSON at `<state_dir>/templates/<name>.json` (unchanged from M1).
- Activation reuses the existing `AppState::set_face(name)` (already template-aware via `resolve_face`). Do NOT add a second activation path.
- The JSON editor API lives under `/api/...`; do NOT mix it with the existing HTML-partial routes (the editor's JS owns its DOM; the control panel is server-rendered HTMX).
- Hardware acceptance is non-negotiable: each phase deploys to pve3 (`portrait-upside-down`, 170×320, theme `tokyonight`) and is eyeballed on the physical panel. Deploy with `cp -f target/release/ht32paneld /usr/local/sbin/ht32paneld` — NEVER `ln`.
- Every changed Rust crate must pass `cargo test -p ht32-panel-daemon`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all -- --check` before a task is complete.

## Exact existing interfaces (verified — use these signatures verbatim)

```rust
// crates/ht32-panel-daemon/src/faces/template/spec.rs
pub struct TemplateSpec { pub name: String, pub orientation: Option<TemplateOrientation>,
                          pub theme: Option<String>, pub widgets: Vec<TemplateWidget> }
pub struct TemplateWidget { pub id: String, pub rect: Rect, /* #[serde(flatten)] */ pub content: TemplateContent }
pub enum TemplateContent { Text{value,size,color,align}, Bar{value,fill,bg},
    Gauge{value,min,max,color,track}, Sparkline{a,b,wrap_around,color_a,color_b,bg,scale}, Clock{mode,color} }
pub enum NumberSource { CpuPercent, RamPercent, CpuTemp, DiskReadRate, DiskWriteRate, NetRxRate, NetTxRate }
pub enum HistorySource { DiskHistory, DiskReadHistory, DiskWriteHistory, NetHistory, NetRxHistory, NetTxHistory }
pub enum TextSource { Literal(String), Hostname, Uptime, Ip, NetInterface, Time(TimeFmt), Date(DateFmt), Number(NumberBinding) }
pub enum TimeFmt { Hhmm, Hhmmss, Hhmm12h }   pub enum DateFmt { Iso, Eu, Us, Short }   pub enum NumberFmt { Percent, Rate, Raw }
pub enum ThemeSlot { Primary, Secondary, Text, Background }   pub enum ColorRef { Theme(ThemeSlot), Hex(u32) }
pub enum Align { Left, Center, Right }   pub enum ClockMode { Analog, Digital }   pub enum ScaleMode { Auto, Fixed(f64) }
pub enum TemplateOrientation { Landscape, Portrait, LandscapeUpsideDown, PortraitUpsideDown }  // -> ht32_panel_hw::Orientation via From

// crates/ht32-panel-daemon/src/faces/layout.rs
pub struct Rect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }   // has serde derive
pub struct Widget { pub id: Cow<'static,str>, pub rect: Rect, pub kind: ZoneKind, pub cadence: Cadence, pub content: WidgetContent }
pub struct Layout { pub widgets: Vec<Widget> }
pub fn render_layout(canvas: &mut Canvas, layout: &Layout);
pub enum WidgetContent { Text{ text, size, color, align }, TextScaled{..}, Bar{..}, /* … */ }

// crates/ht32-panel-daemon/src/faces/mod.rs
pub use template::{list_templates, load_template, TemplateFace};
pub struct Theme { pub primary: u32, pub secondary: u32, pub text: u32, pub background: u32 }
pub fn Theme::from_preset(name: &str) -> Theme;
pub fn create_face(name: &str) -> Option<Box<dyn Face>>;
pub fn available_faces() -> Vec<FaceInfo>;          // FaceInfo { id: &str, display_name: &str }
// Face::layout(&self, &Canvas, &SystemData, &Theme, &EnabledComplications) -> Layout

// crates/ht32-panel-daemon/src/faces/template/face.rs
pub fn load_template(state_dir: &Path, name: &str) -> Option<TemplateSpec>;   // validates name allowlist
pub fn list_templates(state_dir: &Path) -> Vec<String>;
pub struct TemplateFace; impl TemplateFace { pub fn new(spec: TemplateSpec) -> Self }

// crates/ht32-panel-daemon/src/rendering/canvas.rs
impl Canvas { pub fn new(w:u32,h:u32)->Self; pub fn dimensions(&self)->(u32,u32);
    pub fn set_background(&mut self,color:u32); pub fn clear(&mut self); pub fn pixels(&self)->&[u8];
    pub fn text_width(&self, text:&str, size:f32)->i32; pub fn line_height(&self, size:f32)->i32 }

// crates/ht32-panel-daemon/src/state.rs   (AppState — Arc<AppState> shared by web + dbus)
state_dir: PathBuf                                  // field, line ~234
pub fn set_face(&self, name:&str) -> anyhow::Result<()>;     // resolve_face: built-in else template
pub fn face_name(&self) -> String;
pub fn list_all_faces(&self) -> Vec<String>;        // built-in ids + list_templates(state_dir)
pub fn get_screen_png(&self) -> anyhow::Result<Vec<u8>>;     // uses png::Encoder (Rgba, 8-bit)
pub fn available_themes(&self) -> Vec<faces::ThemeInfo>;

// crates/ht32-panel-daemon/src/web/mod.rs
pub struct WebState { pub app: Arc<AppState>, pub signal_tx: broadcast::Sender<DaemonSignals> }
pub fn create_router(state: Arc<AppState>, signal_tx: broadcast::Sender<DaemonSignals>) -> Router;
// handler shape: async fn h(State(state): State<WebState>, ...) -> impl IntoResponse / Response

// crates/ht32-panel-daemon/src/dbus/interface.rs
pub enum DaemonSignals { OrientationChanged, LedChanged, DisplaySettingsChanged, ComplicationOptionChanged }
// #[interface(name="org.ht32panel.Daemon1")] impl Daemon1Interface { async fn x(&self,..) -> zbus::fdo::Result<()> { … self.signal_tx.send(..) } }
```

## File Structure

| File | Responsibility | New? |
|------|----------------|------|
| `crates/ht32-panel-daemon/src/state.rs` | + `save_template`/`update_template`/`delete_template`/`clone_template`; expose `pub fn state_dir(&self)->&Path`, `pub fn current_theme(&self)->Theme`, `pub fn orientation()->Orientation` (exists) | modify |
| `crates/ht32-panel-daemon/src/faces/template/preview.rs` | `Warning`, `check_bounds`, `sample_data`, `preview_render` — the pure server-truth render | **new** |
| `crates/ht32-panel-daemon/src/faces/template/schema.rs` | `template_schema_json() -> serde_json::Value` + exhaustive-match contract test | **new** |
| `crates/ht32-panel-daemon/src/faces/template/mod.rs` | declare `preview`, `schema` submodules; re-export | modify |
| `crates/ht32-panel-daemon/src/web/api.rs` | JSON CRUD + schema + preview handlers; `api_router()` | **new** |
| `crates/ht32-panel-daemon/src/web/editor.rs` | `/editor` page + embedded asset routes; `editor_router()` | **new** |
| `crates/ht32-panel-daemon/src/web/mod.rs` | declare `api`/`editor`; `.merge(api_router()).merge(editor_router())`; add `TemplatesChanged` to SSE map | modify |
| `crates/ht32-panel-daemon/src/dbus/interface.rs` | + `TemplatesChanged` signal; `ListTemplates`/`GetTemplate`/`SaveTemplate`/`DeleteTemplate`/`CloneTemplate` | modify |
| `crates/ht32-panel-daemon/templates/editor.html` | Askama three-column editor shell | **new** |
| `crates/ht32-panel-daemon/assets/editor.js` | Alpine editor component (state, CRUD, drag, preview) | **new** |
| `crates/ht32-panel-daemon/assets/widgets.js` | pure client-side approximate widget renderer | **new** |
| `crates/ht32-panel-daemon/assets/editor.css` | editor layout + widget styles | **new** |
| `crates/ht32-panel-daemon/assets/alpine.min.js` | vendored Alpine.js 3.14.1 (embedded, no CDN) | **new** |

---

## PHASE M2.1 — CRUD backend

### Task 1: AppState template CRUD methods

**Files:**
- Modify: `crates/ht32-panel-daemon/src/state.rs` (add methods to `impl AppState`)
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `self.state_dir: PathBuf`; `faces::list_templates(&Path)`, `faces::load_template(&Path,&str)`; `self.face_name() -> String`.
- Produces:
  - `pub fn save_template(&self, spec: &TemplateSpec) -> anyhow::Result<()>` (create or overwrite; validates name)
  - `pub fn delete_template(&self, name: &str) -> anyhow::Result<()>` (refuses if `name == self.face_name()`)
  - `pub fn clone_template(&self, src: &str, dst: &str) -> anyhow::Result<()>`
  - `pub fn template_names(&self) -> Vec<String>` (= `list_templates(&self.state_dir)`)
  - `pub fn load_template_spec(&self, name: &str) -> Option<TemplateSpec>` (= `load_template`)
  - `pub fn state_dir(&self) -> &std::path::Path`

- [ ] **Step 1: Write the failing test**

Add to `state.rs` tests. Helper builds an `AppState` rooted at a tempdir is heavy (it opens the LCD); instead test the free functions through a thin path-based core. Put the core logic in free functions and have the methods delegate, so tests need no `AppState`:

```rust
#[cfg(test)]
mod template_crud_tests {
    use super::*;
    use crate::faces::template::spec::{TemplateSpec};
    use tempfile::tempdir;

    fn spec(name: &str) -> TemplateSpec {
        TemplateSpec { name: name.to_string(), orientation: None, theme: None, widgets: vec![] }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        save_template_at(dir.path(), &spec("my_face")).unwrap();
        let loaded = crate::faces::load_template(dir.path(), "my_face").unwrap();
        assert_eq!(loaded, spec("my_face"));
    }

    #[test]
    fn save_rejects_bad_name() {
        let dir = tempdir().unwrap();
        assert!(save_template_at(dir.path(), &spec("../evil")).is_err());
        assert!(save_template_at(dir.path(), &spec("")).is_err());
    }

    #[test]
    fn delete_refuses_active() {
        let dir = tempdir().unwrap();
        save_template_at(dir.path(), &spec("live")).unwrap();
        assert!(delete_template_at(dir.path(), "live", "live").is_err()); // active == name
        assert!(delete_template_at(dir.path(), "live", "other").is_ok());
    }

    #[test]
    fn clone_copies_spec_under_new_name() {
        let dir = tempdir().unwrap();
        save_template_at(dir.path(), &spec("base")).unwrap();
        clone_template_at(dir.path(), "base", "copy").unwrap();
        let copy = crate::faces::load_template(dir.path(), "copy").unwrap();
        assert_eq!(copy.name, "copy"); // name field rewritten to match the file
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ht32-panel-daemon template_crud_tests 2>&1 | tail -20`
Expected: FAIL — `save_template_at`, `delete_template_at`, `clone_template_at` not found.

- [ ] **Step 3: Write the free functions + name validator (minimal)**

Add near the top of `state.rs` (module-level), reusing the M1 allowlist rule:

```rust
use crate::faces::template::spec::TemplateSpec;

/// Validates a template name (single gate): non-empty, `[A-Za-z0-9_-]+`.
fn valid_template_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn template_path(state_dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    state_dir.join("templates").join(format!("{name}.json"))
}

/// Writes `spec` to `<state_dir>/templates/<spec.name>.json` (create or overwrite).
fn save_template_at(state_dir: &std::path::Path, spec: &TemplateSpec) -> anyhow::Result<()> {
    if !valid_template_name(&spec.name) {
        anyhow::bail!("invalid template name {:?}", spec.name);
    }
    let dir = state_dir.join("templates");
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(spec)?;
    std::fs::write(template_path(state_dir, &spec.name), json)?;
    Ok(())
}

/// Deletes a template file. Refuses if `name` is the currently active face.
fn delete_template_at(state_dir: &std::path::Path, name: &str, active_face: &str) -> anyhow::Result<()> {
    if !valid_template_name(name) {
        anyhow::bail!("invalid template name {:?}", name);
    }
    if name == active_face {
        anyhow::bail!("cannot delete the active template '{name}'; switch face first");
    }
    std::fs::remove_file(template_path(state_dir, name))?;
    Ok(())
}

/// Loads `src`, rewrites its `name` to `dst`, writes `<dst>.json`.
fn clone_template_at(state_dir: &std::path::Path, src: &str, dst: &str) -> anyhow::Result<()> {
    if !valid_template_name(dst) {
        anyhow::bail!("invalid template name {:?}", dst);
    }
    let mut spec = crate::faces::load_template(state_dir, src)
        .ok_or_else(|| anyhow::anyhow!("source template '{src}' not found"))?;
    spec.name = dst.to_string();
    save_template_at(state_dir, &spec)
}
```

- [ ] **Step 4: Add the thin `AppState` methods that delegate**

Inside `impl AppState`:

```rust
/// The daemon state directory (templates live under `<state_dir>/templates/`).
pub fn state_dir(&self) -> &std::path::Path { &self.state_dir }

/// Saves (creates or overwrites) a template. Validates the name.
pub fn save_template(&self, spec: &TemplateSpec) -> anyhow::Result<()> {
    save_template_at(&self.state_dir, spec)
}

/// Deletes a template. Refuses to delete the active face.
pub fn delete_template(&self, name: &str) -> anyhow::Result<()> {
    delete_template_at(&self.state_dir, name, &self.face_name())
}

/// Duplicates `src` under `dst`.
pub fn clone_template(&self, src: &str, dst: &str) -> anyhow::Result<()> {
    clone_template_at(&self.state_dir, src, dst)
}

/// Lists saved template names.
pub fn template_names(&self) -> Vec<String> {
    crate::faces::list_templates(&self.state_dir)
}

/// Loads a template spec for editing.
pub fn load_template_spec(&self, name: &str) -> Option<TemplateSpec> {
    crate::faces::load_template(&self.state_dir, name)
}
```

- [ ] **Step 5: Run tests + gates**

Run: `cargo test -p ht32-panel-daemon template_crud_tests 2>&1 | tail -10`
Expected: PASS (4 tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3 && cargo fmt --all -- --check && echo OK`
Expected: clean + `OK`.

- [ ] **Step 6: Commit**

```bash
git add crates/ht32-panel-daemon/src/state.rs
git commit -m "feat(template): AppState CRUD (save/delete/clone) with name-allowlist gate"
```

---

### Task 2: JSON CRUD API routes

**Files:**
- Create: `crates/ht32-panel-daemon/src/web/api.rs`
- Modify: `crates/ht32-panel-daemon/src/web/mod.rs` (declare module, merge router)
- Test: `crates/ht32-panel-daemon/src/web/api.rs` `#[cfg(test)] mod tests` (pure request→response via `tower::ServiceExt::oneshot`)

**Interfaces:**
- Consumes: `WebState { app: Arc<AppState>, signal_tx }`; AppState `template_names`, `load_template_spec`, `save_template`, `delete_template`, `clone_template`; `DaemonSignals::TemplatesChanged` (added in Task 3 — for now emit `DisplaySettingsChanged` and switch in Task 3).
- Produces: `pub fn api_router() -> Router<WebState>` with the routes below; reused by `mod.rs`.

Routes (this task: CRUD only; schema + preview are Tasks 5/6 and add to this same router):
`GET /api/templates`, `POST /api/templates`, `GET/PUT/DELETE /api/templates/{name}`, `POST /api/templates/{name}/clone`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // oneshot
    // Build a WebState over a tempdir-backed AppState test double.
    // AppState is heavy; expose a test constructor `AppState::for_tests(state_dir)`
    // that builds state WITHOUT opening the LCD (face=professional, no device).

    fn test_state() -> (tempfile::TempDir, WebState) {
        let dir = tempfile::tempdir().unwrap();
        let app = std::sync::Arc::new(crate::state::AppState::for_tests(dir.path()));
        let (tx, _rx) = tokio::sync::broadcast::channel(16);
        (dir, WebState { app, signal_tx: tx })
    }

    #[tokio::test]
    async fn post_then_get_template() {
        let (_d, st) = test_state();
        let router = super::api_router().with_state(st);
        let body = r#"{"name":"web_made","widgets":[]}"#;
        let resp = router.clone().oneshot(
            Request::post("/api/templates").header("content-type","application/json")
                .body(Body::from(body)).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = router.oneshot(
            Request::get("/api/templates/web_made").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_bad_name_is_400() {
        let (_d, st) = test_state();
        let router = super::api_router().with_state(st);
        let resp = router.oneshot(
            Request::post("/api/templates").header("content-type","application/json")
                .body(Body::from(r#"{"name":"../evil","widgets":[]}"#)).unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
```

> NOTE for the implementer: this task requires a non-LCD `AppState::for_tests(state_dir: &Path) -> AppState` constructor. If one does not exist, add it in this task: construct the struct with `face = create_face("professional").unwrap()`, a `Framebuffer`/`Canvas` at 320×170, `lcd = Mutex::new(None)` (no device), and `state_dir` set to the arg. Gate it `#[cfg(any(test, feature = "test-support"))]` or make it `pub` with a doc note "test/headless construction; no hardware". Keep it minimal — it only needs the fields the API handlers read (`state_dir`, `face_name`).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ht32-panel-daemon web::api 2>&1 | tail -20`
Expected: FAIL — `api_router` / `AppState::for_tests` not found.

- [ ] **Step 3: Implement `api.rs` CRUD handlers**

```rust
//! JSON template-editor API (`/api/...`). Distinct from the HTML-partial routes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::dbus::DaemonSignals;
use crate::faces::template::spec::TemplateSpec;
use crate::web::WebState;

/// Maps an `anyhow::Error` from AppState into a 400 with its message.
fn bad_request(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, e.to_string()).into_response()
}

/// GET /api/templates -> ["name", ...]
async fn list(State(st): State<WebState>) -> Json<Vec<String>> {
    Json(st.app.template_names())
}

/// POST /api/templates  (body: TemplateSpec) -> 200 {"name":...} | 400
async fn create(State(st): State<WebState>, Json(spec): Json<TemplateSpec>) -> Response {
    match st.app.save_template(&spec) {
        Ok(()) => {
            let _ = st.signal_tx.send(DaemonSignals::TemplatesChanged);
            Json(serde_json::json!({ "name": spec.name })).into_response()
        }
        Err(e) => bad_request(e),
    }
}

/// GET /api/templates/{name} -> TemplateSpec | 404
async fn get_one(State(st): State<WebState>, Path(name): Path<String>) -> Response {
    match st.app.load_template_spec(&name) {
        Some(spec) => Json(spec).into_response(),
        None => (StatusCode::NOT_FOUND, format!("template '{name}' not found")).into_response(),
    }
}

/// PUT /api/templates/{name}  (body: TemplateSpec) -> 200 | 400
async fn update(State(st): State<WebState>, Path(name): Path<String>, Json(mut spec): Json<TemplateSpec>) -> Response {
    spec.name = name; // the URL is authoritative for the file name
    match st.app.save_template(&spec) {
        Ok(()) => { let _ = st.signal_tx.send(DaemonSignals::TemplatesChanged);
                    Json(serde_json::json!({"ok": true})).into_response() }
        Err(e) => bad_request(e),
    }
}

/// DELETE /api/templates/{name} -> 204 | 400 (refused if active)
async fn delete(State(st): State<WebState>, Path(name): Path<String>) -> Response {
    match st.app.delete_template(&name) {
        Ok(()) => { let _ = st.signal_tx.send(DaemonSignals::TemplatesChanged);
                    StatusCode::NO_CONTENT.into_response() }
        Err(e) => bad_request(e),
    }
}

#[derive(Deserialize)]
struct CloneBody { new_name: String }

/// POST /api/templates/{name}/clone  (body: {"new_name":...}) -> 200 {"name":...} | 400
async fn clone(State(st): State<WebState>, Path(name): Path<String>, Json(b): Json<CloneBody>) -> Response {
    match st.app.clone_template(&name, &b.new_name) {
        Ok(()) => { let _ = st.signal_tx.send(DaemonSignals::TemplatesChanged);
                    Json(serde_json::json!({"name": b.new_name})).into_response() }
        Err(e) => bad_request(e),
    }
}

/// Router for the JSON API. Schema + preview routes are added by later tasks.
pub fn api_router() -> Router<WebState> {
    Router::new()
        .route("/api/templates", get(list).post(create))
        .route("/api/templates/{name}", get(get_one).put(update).delete(delete))
        .route("/api/templates/{name}/clone", post(clone))
}
```

- [ ] **Step 4: Wire into `web/mod.rs`**

Add `mod api;` near the top, add the `TemplatesChanged` arm placeholder is NOT needed yet (Task 3 adds the enum variant; until then `create` etc. must compile — so DO Task 3's enum addition first OR temporarily emit `DisplaySettingsChanged`). To keep tasks independent, in this task emit `DisplaySettingsChanged` everywhere `TemplatesChanged` appears above, and Task 3 will switch them. Update the merge in `create_router`:

```rust
Router::new()
    .route("/", get(index))
    // … existing routes …
    .route("/preview", get(preview_get))
    .merge(api::api_router())
    .with_state(web_state)
```

> Implementer: replace the four `DaemonSignals::TemplatesChanged` in `api.rs` with `DaemonSignals::DisplaySettingsChanged` for THIS task; Task 3 introduces the variant and switches them back. (Recorded so the plan stays compile-correct if executed strictly in order.)

- [ ] **Step 5: Run tests + gates**

Run: `cargo test -p ht32-panel-daemon web::api 2>&1 | tail -10`  → PASS (2 tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3 && cargo fmt --all -- --check && echo OK` → clean + OK.

- [ ] **Step 6: Commit**

```bash
git add crates/ht32-panel-daemon/src/web/api.rs crates/ht32-panel-daemon/src/web/mod.rs crates/ht32-panel-daemon/src/state.rs
git commit -m "feat(web): JSON /api/templates CRUD routes"
```

---

### Task 3: D-Bus CRUD mirror + TemplatesChanged signal

**Files:**
- Modify: `crates/ht32-panel-daemon/src/dbus/interface.rs` (add signal variant + 5 methods)
- Modify: `crates/ht32-panel-daemon/src/web/mod.rs` (`events_stream` match arm) and `crates/ht32-panel-daemon/src/web/api.rs` (switch to `TemplatesChanged`)
- Test: `interface.rs` unit test for the JSON (de)serialization helper used by `SaveTemplate`

**Interfaces:**
- Consumes: `AppState::{template_names, load_template_spec, save_template, delete_template, clone_template}`.
- Produces: `DaemonSignals::TemplatesChanged`; D-Bus methods `ListTemplates() -> Vec<String>`, `GetTemplate(name) -> String` (JSON), `SaveTemplate(json) -> ()`, `DeleteTemplate(name) -> ()`, `CloneTemplate(src, dst) -> ()`.

- [ ] **Step 1: Add the signal variant**

In `DaemonSignals`:
```rust
pub enum DaemonSignals {
    OrientationChanged, LedChanged, DisplaySettingsChanged, ComplicationOptionChanged,
    /// A template was created, updated, deleted, or cloned.
    TemplatesChanged,
}
```

- [ ] **Step 2: Add SSE map arm in `web/mod.rs` `events_stream`**

```rust
DaemonSignals::TemplatesChanged => "templates",
```
And switch `web/api.rs`'s four signal emissions from `DisplaySettingsChanged` back to `TemplatesChanged`.

- [ ] **Step 3: Write the failing test (JSON round-trip used by SaveTemplate)**

```rust
#[cfg(test)]
mod template_dbus_tests {
    use crate::faces::template::spec::TemplateSpec;
    #[test]
    fn save_template_json_parses() {
        let json = r#"{"name":"viadbus","widgets":[]}"#;
        let spec: TemplateSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.name, "viadbus");
    }
}
```

- [ ] **Step 4: Run to verify it fails / compiles**

Run: `cargo test -p ht32-panel-daemon template_dbus_tests 2>&1 | tail -10`
Expected: PASS for the helper test (it only checks serde), but the build must include the new methods (Step 5) — run after Step 5.

- [ ] **Step 5: Add the 5 D-Bus methods (mirror the `set_face` pattern)**

Inside `#[interface(...)] impl Daemon1Interface`:
```rust
/// Lists saved template names.
fn list_templates(&self) -> Vec<String> { self.state.template_names() }

/// Returns a template as a JSON string, or an error if missing.
fn get_template(&self, name: &str) -> zbus::fdo::Result<String> {
    let spec = self.state.load_template_spec(name)
        .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("template '{name}' not found")))?;
    serde_json::to_string(&spec).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
}

/// Saves a template from a JSON string.
fn save_template(&self, json: &str) -> zbus::fdo::Result<()> {
    let spec: crate::faces::template::spec::TemplateSpec = serde_json::from_str(json)
        .map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
    self.state.save_template(&spec).map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
    let _ = self.signal_tx.send(DaemonSignals::TemplatesChanged);
    debug!("D-Bus: SaveTemplate({})", spec.name);
    Ok(())
}

/// Deletes a template (refused if active).
fn delete_template(&self, name: &str) -> zbus::fdo::Result<()> {
    self.state.delete_template(name).map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
    let _ = self.signal_tx.send(DaemonSignals::TemplatesChanged);
    debug!("D-Bus: DeleteTemplate({})", name);
    Ok(())
}

/// Duplicates `src` under `dst`.
fn clone_template(&self, src: &str, dst: &str) -> zbus::fdo::Result<()> {
    self.state.clone_template(src, dst).map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
    let _ = self.signal_tx.send(DaemonSignals::TemplatesChanged);
    debug!("D-Bus: CloneTemplate({} -> {})", src, dst);
    Ok(())
}
```

- [ ] **Step 6: Run tests + gates**

Run: `cargo test -p ht32-panel-daemon 2>&1 | tail -6` → all pass.
Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3 && cargo fmt --all -- --check && echo OK` → clean + OK.

- [ ] **Step 7: Commit**

```bash
git add crates/ht32-panel-daemon/src/dbus/interface.rs crates/ht32-panel-daemon/src/web/mod.rs crates/ht32-panel-daemon/src/web/api.rs
git commit -m "feat(dbus): template CRUD mirror + TemplatesChanged signal"
```

### Phase M2.1 hardware checkpoint (controller, not a subagent step)

Deploy to pve3 (push origin/main; on pve3 `git reset --hard fork/main`, rebuild, `cp -f` binary, restart). Then:
```bash
curl -s -X POST http://192.168.1.53:8686/api/templates -H 'content-type: application/json' \
  -d @- <<'JSON'
{"name":"crudtest","orientation":"portrait","widgets":[
  {"id":"h","rect":{"x":6,"y":6,"w":158,"h":18},"kind":"text","value":{"src":"hostname"},"size":14.0,"color":"primary","align":"center"}]}
JSON
ssh root@192.168.1.53 'ht32panelctl lcd face crudtest'
```
**Eyes on the physical panel:** the hostname renders. Confirms CRUD→storage→activation works end-to-end on hardware before any UI exists.

---

## PHASE M2.2 — preview + schema

### Task 4: `sample_data` + `check_bounds` + `Warning`

**Files:**
- Create: `crates/ht32-panel-daemon/src/faces/template/preview.rs`
- Modify: `crates/ht32-panel-daemon/src/faces/template/mod.rs` (add `pub mod preview;`)
- Test: `preview.rs` tests

**Interfaces:**
- Consumes: `SystemData` (struct, fields per `sensors/data.rs`); `Layout`, `Widget`, `WidgetContent`, `Rect` (`faces/layout.rs`); `Canvas::{new, text_width, line_height}`.
- Produces:
  - `pub struct Warning { pub widget_id: String, pub message: String }`
  - `pub fn sample_data() -> SystemData`
  - `pub fn check_bounds(layout: &Layout, canvas: &Canvas) -> Vec<Warning>`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::layout::{Layout, Widget, WidgetContent, Rect, ZoneKind, Cadence};
    use crate::faces::Align;
    use crate::rendering::Canvas;
    use std::borrow::Cow;

    fn text_widget(id: &str, rect: Rect, text: &str, size: f32) -> Widget {
        Widget { id: Cow::Owned(id.into()), rect, kind: ZoneKind::Dynamic, cadence: Cadence::EveryFrame,
            content: WidgetContent::Text { text: text.into(), size, color: 0xFFFFFF, align: Align::Left } }
    }

    #[test]
    fn rect_off_canvas_warns() {
        let canvas = Canvas::new(170, 320);
        let layout = Layout { widgets: vec![
            text_widget("over", Rect { x: 160, y: 4, w: 40, h: 16 }, "hi", 12.0) ] };
        let warns = check_bounds(&layout, &canvas);
        assert!(warns.iter().any(|w| w.widget_id == "over"));
    }

    #[test]
    fn fitting_widget_no_warn() {
        let canvas = Canvas::new(170, 320);
        let layout = Layout { widgets: vec![
            text_widget("ok", Rect { x: 4, y: 4, w: 80, h: 16 }, "hi", 12.0) ] };
        assert!(check_bounds(&layout, &canvas).is_empty());
    }

    #[test]
    fn text_wider_than_rect_warns() {
        let canvas = Canvas::new(170, 320);
        // A long string in a narrow rect: measured width should exceed rect.w.
        let layout = Layout { widgets: vec![
            text_widget("wide", Rect { x: 4, y: 4, w: 20, h: 16 },
                        "a very long label that will not fit", 14.0) ] };
        assert!(check_bounds(&layout, &canvas).iter().any(|w| w.widget_id == "wide"));
    }

    #[test]
    fn sample_data_has_full_histories() {
        let d = sample_data();
        assert!(d.disk_history.len() >= 60);
        assert!(d.net_rx_history.len() >= 60);
        assert!(!d.hostname.is_empty());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ht32-panel-daemon faces::template::preview 2>&1 | tail -20`
Expected: FAIL — module/functions not found.

- [ ] **Step 3: Implement `preview.rs` (part 1: sample_data + check_bounds)**

```rust
//! Pure server-truth preview: render a draft `TemplateSpec` off-screen through the
//! real `render_layout`, and report layout warnings — without touching the live device.

use crate::faces::layout::{Layout, WidgetContent};
use crate::rendering::Canvas;
use crate::sensors::data::SystemData;

/// A non-blocking layout warning surfaced in the editor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Warning {
    pub widget_id: String,
    pub message: String,
}

/// A fixed, representative `SystemData` so previews are deterministic and populated.
#[allow(clippy::field_reassign_with_default)]
pub fn sample_data() -> SystemData {
    let mut d = SystemData::default();
    d.hostname = "preview-host".into();
    d.cpu_percent = 65.0;
    d.ram_percent = 80.0;
    d.cpu_temp = Some(72.0);
    d.hour = 14; d.minute = 30; d.day = 21; d.month = 6; d.year = 2026;
    d.disk_read_rate = 5_000_000.0; d.disk_write_rate = 1_000_000.0;
    d.net_rx_rate = 2_000_000.0; d.net_tx_rate = 500_000.0;
    d.disk_sample_count = 60; d.net_sample_count = 60;
    for i in 0..60u64 {
        let v = (i as f64) * 100_000.0;
        d.disk_history.push_back(v);
        d.disk_read_history.push_back(v);
        d.disk_write_history.push_back(v / 2.0);
        d.net_history.push_back(v);
        d.net_rx_history.push_back(v);
        d.net_tx_history.push_back(v / 4.0);
    }
    d
}

/// Reports widgets whose rect leaves the canvas, or whose resolved text is wider
/// than its rect. Computed in the same units the renderer draws in, so a warning
/// can never disagree with the rendered pixels.
pub fn check_bounds(layout: &Layout, canvas: &Canvas) -> Vec<Warning> {
    let (cw, ch) = canvas.dimensions();
    let (cw, ch) = (cw as i32, ch as i32);
    let mut out = Vec::new();
    for w in &layout.widgets {
        let r = &w.rect;
        if r.x < 0 || r.y < 0 || r.x + r.w as i32 > cw || r.y + r.h as i32 > ch {
            out.push(Warning { widget_id: w.id.to_string(),
                message: format!("widget '{}' extends outside the {}×{} screen", w.id, cw, ch) });
            continue; // one warning per widget is enough
        }
        if let WidgetContent::Text { text, size, .. } = &w.content {
            if canvas.text_width(text, *size) > r.w as i32 {
                out.push(Warning { widget_id: w.id.to_string(),
                    message: format!("text in '{}' is wider than its box", w.id) });
            }
        }
    }
    out
}
```

- [ ] **Step 4: Run tests + gates**

Run: `cargo test -p ht32-panel-daemon faces::template::preview 2>&1 | tail -10` → PASS (4 tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3 && cargo fmt --all -- --check && echo OK` → clean + OK.

- [ ] **Step 5: Commit**

```bash
git add crates/ht32-panel-daemon/src/faces/template/preview.rs crates/ht32-panel-daemon/src/faces/template/mod.rs
git commit -m "feat(template): sample_data + check_bounds layout warnings"
```

---

### Task 5: `preview_render` + `/api/templates/preview`

**Files:**
- Modify: `crates/ht32-panel-daemon/src/faces/template/preview.rs` (add `preview_render`)
- Modify: `crates/ht32-panel-daemon/src/web/api.rs` (add `/api/templates/preview` route + handler; need `AppState::current_theme` + `AppState::orientation`)
- Modify: `crates/ht32-panel-daemon/src/state.rs` (add `pub fn current_theme(&self) -> Theme`)
- Test: `preview.rs` (pixel + warning), `web/api.rs` (route returns base64 + warnings)

**Interfaces:**
- Consumes: `TemplateFace::new(spec).layout(&canvas, &sample_data(), &theme, &EnabledComplications::new())`; `render_layout`; `png::Encoder`; `Theme`; `ht32_panel_hw::Orientation::dimensions()`.
- Produces:
  - `pub fn preview_render(spec: &TemplateSpec, theme: &Theme, orientation: Orientation) -> (Vec<u8>, Vec<Warning>)`
  - `pub fn current_theme(&self) -> Theme` on AppState
  - JSON route `POST /api/templates/preview` → `{ png_base64: String, warnings: Vec<Warning> }`

- [ ] **Step 1: Write the failing test (pixel + warning)**

```rust
// in preview.rs tests
#[test]
fn preview_render_paints_and_reports() {
    use crate::faces::Theme;
    use ht32_panel_hw::Orientation;
    use crate::faces::template::spec::*;
    // One in-bounds bar (paints) + one off-canvas text (warns).
    let spec = TemplateSpec {
        name: "p".into(), orientation: None, theme: None,
        widgets: vec![
            TemplateWidget { id: "bar".into(), rect: crate::faces::layout::Rect{x:0,y:0,w:100,h:10},
                content: TemplateContent::Bar { value: NumberSource::CpuPercent,
                    fill: ColorRef::Hex(0xFFFFFF), bg: ColorRef::Hex(0x000000) } },
            TemplateWidget { id: "off".into(), rect: crate::faces::layout::Rect{x:300,y:0,w:80,h:16},
                content: TemplateContent::Text { value: TextSource::Hostname, size: 12.0,
                    color: ColorRef::Hex(0xFFFFFF), align: Align::Left } },
        ],
    };
    let theme = Theme::from_preset("nord");
    let (png, warns) = preview_render(&spec, &theme, Orientation::Landscape);
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "valid PNG header");
    assert!(warns.iter().any(|w| w.widget_id == "off"), "off-canvas widget warns");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ht32-panel-daemon preview_render_paints_and_reports 2>&1 | tail -20`
Expected: FAIL — `preview_render` not found.

- [ ] **Step 3: Implement `preview_render`**

```rust
use crate::faces::layout::render_layout;
use crate::faces::template::spec::TemplateSpec;
use crate::faces::{EnabledComplications, Theme, TemplateFace};
use ht32_panel_hw::Orientation;

/// Renders a draft spec off-screen through the REAL renderer and returns
/// `(png_bytes, warnings)`. Never touches the live device or active face.
pub fn preview_render(spec: &TemplateSpec, theme: &Theme, orientation: Orientation) -> (Vec<u8>, Vec<Warning>) {
    let (w, h) = orientation.dimensions();           // portrait swaps to 170×320
    let (w, h) = (w as u32, h as u32);
    let reference = Canvas::new(w, h);               // measurement canvas for layout()
    let comps = EnabledComplications::new();
    let layout = TemplateFace::new(spec.clone())
        .layout(&reference, &sample_data(), theme, &comps);
    let warnings = check_bounds(&layout, &reference);

    let mut canvas = Canvas::new(w, h);
    canvas.set_background(theme.background);
    canvas.clear();
    render_layout(&mut canvas, &layout);

    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().expect("png header");
        writer.write_image_data(canvas.pixels()).expect("png data");
    }
    (png, warnings)
}
```
> Note `orientation.dimensions()` is the same helper `AppState::new` uses (`state.rs:305`). If it is not `pub`, make it `pub` on `Orientation` in the hw crate (it already exists for internal use).

- [ ] **Step 4: Add `AppState::current_theme` + the route**

In `state.rs` `impl AppState`:
```rust
/// The active `Theme` (resolved from the current theme name).
pub fn current_theme(&self) -> crate::faces::Theme {
    crate::faces::Theme::from_preset(&self.theme_name())
}
```
In `web/api.rs`:
```rust
use crate::faces::template::preview::preview_render;

/// POST /api/templates/preview (body: TemplateSpec) -> {png_base64, warnings}
async fn preview(State(st): State<WebState>, Json(spec): Json<TemplateSpec>) -> Response {
    // spec.orientation/theme override the daemon's current settings if set.
    let orientation = match spec.orientation {
        Some(o) => o.into(),
        None => st.app.orientation(),
    };
    let theme = match &spec.theme {
        Some(name) => crate::faces::Theme::from_preset(name),
        None => st.app.current_theme(),
    };
    let (png, warnings) = preview_render(&spec, &theme, orientation);
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    Json(serde_json::json!({ "png_base64": b64, "warnings": warnings })).into_response()
}
```
Add the route to `api_router()`: `.route("/api/templates/preview", post(preview))`.
> `base64` is already a transitive dep via several crates; if it is not a direct dependency, add `base64 = "0.22"` to `crates/ht32-panel-daemon/Cargo.toml`. (Alternatively, return the PNG as a raw `image/png` body from a second GET that caches the last preview — but base64-in-JSON keeps the warnings and image in one response, matching the spec.)

- [ ] **Step 5: Write the route test, run, gates**

```rust
// web/api.rs tests
#[tokio::test]
async fn preview_returns_png_and_warnings() {
    let (_d, st) = test_state();
    let router = super::api_router().with_state(st);
    let body = r#"{"name":"x","widgets":[
      {"id":"off","rect":{"x":300,"y":0,"w":80,"h":16},"kind":"text",
       "value":{"src":"hostname"},"size":12.0,"color":"primary","align":"left"}]}"#;
    let resp = router.oneshot(axum::http::Request::post("/api/templates/preview")
        .header("content-type","application/json")
        .body(axum::body::Body::from(body)).unwrap()).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v["png_base64"].as_str().unwrap().len() > 100);
    assert!(!v["warnings"].as_array().unwrap().is_empty());
}
```
Run: `cargo test -p ht32-panel-daemon 2>&1 | tail -6` → pass.
Run clippy + fmt gates → clean + OK.

- [ ] **Step 6: Commit**

```bash
git add crates/ht32-panel-daemon/src/faces/template/preview.rs crates/ht32-panel-daemon/src/web/api.rs crates/ht32-panel-daemon/src/state.rs crates/ht32-panel-daemon/Cargo.toml
git commit -m "feat(web): pure preview_render + /api/templates/preview (png + warnings)"
```

---

### Task 6: `/api/template-schema` + compiler-enforced contract test

**Files:**
- Create: `crates/ht32-panel-daemon/src/faces/template/schema.rs`
- Modify: `crates/ht32-panel-daemon/src/faces/template/mod.rs` (`pub mod schema;`)
- Modify: `crates/ht32-panel-daemon/src/web/api.rs` (add `GET /api/template-schema`)
- Test: `schema.rs` exhaustive-match contract test

**Interfaces:**
- Produces: `pub fn template_schema_json() -> serde_json::Value`; route `GET /api/template-schema`.

- [ ] **Step 1: Write the failing contract test (exhaustive match = compiler guard)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::template::spec::*;

    /// Maps each NumberSource variant to its wire string. The `match` is
    /// exhaustive (no `_`), so adding a variant breaks compilation until both
    /// this map AND the schema are updated — the dropdown can never drift.
    fn number_source_wire(s: NumberSource) -> &'static str {
        match s {
            NumberSource::CpuPercent => "cpu_percent",
            NumberSource::RamPercent => "ram_percent",
            NumberSource::CpuTemp => "cpu_temp",
            NumberSource::DiskReadRate => "disk_read_rate",
            NumberSource::DiskWriteRate => "disk_write_rate",
            NumberSource::NetRxRate => "net_rx_rate",
            NumberSource::NetTxRate => "net_tx_rate",
        }
    }

    #[test]
    fn schema_contains_every_number_source() {
        let schema = template_schema_json();
        let listed: Vec<String> = serde_json::from_value(schema["number_sources"].clone()).unwrap();
        for s in [NumberSource::CpuPercent, NumberSource::RamPercent, NumberSource::CpuTemp,
                  NumberSource::DiskReadRate, NumberSource::DiskWriteRate,
                  NumberSource::NetRxRate, NumberSource::NetTxRate] {
            assert!(listed.iter().any(|x| x == number_source_wire(s)),
                "schema missing number source {:?}", s);
        }
    }

    #[test]
    fn schema_has_all_top_level_keys() {
        let s = template_schema_json();
        for k in ["kinds","number_sources","history_sources","text_sources","theme_slots",
                  "aligns","time_fmts","date_fmts","number_fmts","clock_modes","orientations"] {
            assert!(s.get(k).is_some(), "schema missing key {k}");
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ht32-panel-daemon faces::template::schema 2>&1 | tail -20`
Expected: FAIL — `template_schema_json` not found.

- [ ] **Step 3: Implement `schema.rs`**

```rust
//! The editor's vocabulary, generated as JSON for the property-panel dropdowns.
//! Hand-listed here but guarded by an exhaustive-match contract test, so a new
//! enum variant cannot silently desync the editor from the renderer.

use serde_json::{json, Value};

/// All closed enum sets the editor offers, as JSON arrays of wire strings.
pub fn template_schema_json() -> Value {
    json!({
        "kinds": ["text","bar","gauge","sparkline","clock"],
        "number_sources": ["cpu_percent","ram_percent","cpu_temp","disk_read_rate",
                            "disk_write_rate","net_rx_rate","net_tx_rate"],
        "history_sources": ["disk_history","disk_read_history","disk_write_history",
                            "net_history","net_rx_history","net_tx_history"],
        "text_sources": ["literal","hostname","uptime","ip","net_interface","time","date","number"],
        "theme_slots": ["primary","secondary","text","background"],
        "aligns": ["left","center","right"],
        "time_fmts": ["hhmm","hhmmss","hhmm12h"],
        "date_fmts": ["iso","eu","us","short"],
        "number_fmts": ["percent","rate","raw"],
        "clock_modes": ["analog","digital"],
        "scale_modes": ["auto","fixed"],
        "orientations": ["landscape","portrait","landscape_upside_down","portrait_upside_down"]
    })
}
```

- [ ] **Step 4: Add the route**

In `web/api.rs`:
```rust
use crate::faces::template::schema::template_schema_json;
/// GET /api/template-schema -> the dropdown vocabulary
async fn schema() -> axum::Json<serde_json::Value> { axum::Json(template_schema_json()) }
```
Add `.route("/api/template-schema", get(schema))` to `api_router()`.

- [ ] **Step 5: Run tests + gates**

Run: `cargo test -p ht32-panel-daemon faces::template::schema 2>&1 | tail -10` → PASS.
Run clippy + fmt gates → clean + OK.

- [ ] **Step 6: Commit**

```bash
git add crates/ht32-panel-daemon/src/faces/template/schema.rs crates/ht32-panel-daemon/src/faces/template/mod.rs crates/ht32-panel-daemon/src/web/api.rs
git commit -m "feat(web): /api/template-schema with compiler-enforced variant contract"
```

### Phase M2.2 hardware checkpoint (controller)

After deploy, `curl -s -X POST http://192.168.1.53:8686/api/templates/preview -H 'content-type: application/json' -d @templates/example-portrait.json | jq -r .png_base64 | base64 -d > /tmp/preview.png`, copy locally, **view the PNG** — confirm it matches what the panel shows for the same template. Confirms the server-truth path is real.

---

## PHASE M2.3 — editor shell + property panel (no drag yet)

### Task 7: `/editor` page + embedded assets + Alpine bootstrap

**Files:**
- Create: `crates/ht32-panel-daemon/templates/editor.html`, `assets/editor.js`, `assets/editor.css`
- Create: `crates/ht32-panel-daemon/src/web/editor.rs`
- Modify: `crates/ht32-panel-daemon/src/web/mod.rs` (`mod editor; .merge(editor::editor_router())`)
- Test: `web/editor.rs` (route returns 200 + expected asset bytes)

**Interfaces:**
- Produces: `pub fn editor_router() -> Router<WebState>` serving `GET /editor`, `GET /editor/editor.js`, `GET /editor/editor.css`, `GET /editor/widgets.js` (widgets.js created in Task 9 — for this task create an empty stub file so `include_str!` compiles).

- [ ] **Step 1: Create the stub + vendored asset files (so include_str! compiles)**

`assets/widgets.js`:
```js
// Client-side approximate widget renderer (implemented in Task 9).
export function renderWidget() {}
```

Vendor Alpine.js (download the pinned release into the repo so it ships embedded — no CDN at runtime):
```bash
curl -fsSL https://unpkg.com/alpinejs@3.14.1/dist/cdn.min.js \
  -o crates/ht32-panel-daemon/assets/alpine.min.js
# record the version for provenance
echo "alpinejs 3.14.1 (vendored $(date +%F))" > crates/ht32-panel-daemon/assets/alpine.min.js.PROVENANCE
```
`assets/editor.css` (minimal three-column grid; full styling acceptable here as it's the shell):
```css
:root { --bg:#11131a; --panel:#1a1d27; --line:#2a2f3d; --ink:#e6e9ef; --accent:#7aa2f7; }
* { box-sizing: border-box; }
body { margin:0; font-family:system-ui,sans-serif; background:var(--bg); color:var(--ink); }
.editor { display:grid; grid-template-columns:160px 1fr 280px; grid-template-rows:48px 1fr;
          grid-template-areas:"top top top" "palette canvas props"; height:100vh; }
.topbar { grid-area:top; display:flex; align-items:center; gap:12px; padding:0 12px;
          background:var(--panel); border-bottom:1px solid var(--line); }
.palette { grid-area:palette; background:var(--panel); border-right:1px solid var(--line); padding:8px; }
.canvaswrap { grid-area:canvas; display:flex; align-items:center; justify-content:center; gap:24px; }
.props { grid-area:props; background:var(--panel); border-left:1px solid var(--line); padding:8px; overflow:auto; }
.device { position:relative; background:#000; outline:1px solid var(--line); }
.widget { position:absolute; overflow:hidden; outline:1px dashed transparent; }
.widget.selected { outline:1px solid var(--accent); }
.widget.warn { outline:2px solid #e0af68; }
.palette button, .topbar button { background:#222838; color:var(--ink); border:1px solid var(--line);
          border-radius:6px; padding:6px 10px; cursor:pointer; width:100%; margin-bottom:6px; }
```

- [ ] **Step 2: Create `editor.html` (Askama shell)**

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
  <title>HT32 Template Editor</title>
  <link rel="stylesheet" href="/editor/editor.css">
  <!-- Alpine.js is vendored + embedded (no CDN, no SRI risk, works offline) -->
  <script src="/editor/alpine.js" defer></script>
</head>
<body>
  <div class="editor" x-data="editor()" x-init="init()">
    <div class="topbar">
      <strong>Template</strong>
      <select x-model="name" @change="load()">
        <option value="">— new —</option>
        <template x-for="t in templates" :key="t"><option :value="t" x-text="t"></option></template>
      </select>
      <input x-model="name" placeholder="name" size="14">
      <button @click="save()">Save</button>
      <button @click="activate()">Activate</button>
      <button @click="refreshTruth()">Refresh preview</button>
      <span x-text="status"></span>
    </div>

    <div class="palette">
      <template x-for="k in schema.kinds" :key="k">
        <button @click="addWidget(k)" x-text="'+ ' + k"></button>
      </template>
    </div>

    <div class="canvaswrap">
      <div class="device" :style="deviceStyle()">
        <template x-for="(w,i) in spec.widgets" :key="w.id">
          <div class="widget" :class="{selected: i===sel}" @mousedown="select(i)"
               :style="widgetStyle(w)" x-text="w.id"></div>
        </template>
      </div>
      <img class="device" :src="truthSrc" :style="deviceStyle()" alt="true preview">
    </div>

    <div class="props" x-show="sel!==null">
      <!-- property panel fields injected in Task 8 -->
      <em x-show="sel===null">Select a widget</em>
    </div>
  </div>
  <script type="module" src="/editor/editor.js"></script>
</body>
</html>
```

- [ ] **Step 3: Create `editor.js` (Alpine bootstrap — state + load schema/list; no add/save yet)**

```js
window.editor = function () {
  return {
    schema: { kinds: [] }, templates: [], name: "",
    spec: { name: "", orientation: "portrait", widgets: [] },
    sel: null, truthSrc: "", status: "", scale: 1.5,
    async init() {
      this.schema = await (await fetch("/api/template-schema")).json();
      this.templates = await (await fetch("/api/templates")).json();
      this.dims = this.spec.orientation.startsWith("portrait") ? [170,320] : [320,170];
    },
    deviceStyle() {
      const [w,h] = this.dims; const s = this.scale;
      return `width:${w*s}px;height:${h*s}px`;
    },
    widgetStyle(w) {
      const s = this.scale; const r = w.rect;
      return `left:${r.x*s}px;top:${r.y*s}px;width:${r.w*s}px;height:${r.h*s}px;`+
             `font-size:10px;color:#888;background:#222`;
    },
    select(i){ this.sel = i; },
    addWidget(){}, save(){}, load(){}, activate(){}, refreshTruth(){}, // Tasks 8/9/10/11
  };
};
```

- [ ] **Step 4: Create `web/editor.rs` (page + asset routes via include_str!)**

```rust
//! The `/editor` page and its embedded static assets (no build step).

use askama::Template;
use axum::{http::header, response::{Html, IntoResponse}, routing::get, Router};
use crate::web::WebState;

#[derive(Template)]
#[template(path = "editor.html")]
struct EditorTemplate;

async fn page() -> impl IntoResponse { Html(EditorTemplate.render().unwrap()) }

const EDITOR_JS: &str = include_str!("../../assets/editor.js");
const WIDGETS_JS: &str = include_str!("../../assets/widgets.js");
const EDITOR_CSS: &str = include_str!("../../assets/editor.css");
const ALPINE_JS: &str = include_str!("../../assets/alpine.min.js");

async fn editor_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript")], EDITOR_JS)
}
async fn widgets_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript")], WIDGETS_JS)
}
async fn editor_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css")], EDITOR_CSS)
}
async fn alpine_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript")], ALPINE_JS)
}

/// Router for the editor page + assets.
pub fn editor_router() -> Router<WebState> {
    Router::new()
        .route("/editor", get(page))
        .route("/editor/editor.js", get(editor_js))
        .route("/editor/widgets.js", get(widgets_js))
        .route("/editor/editor.css", get(editor_css))
        .route("/editor/alpine.js", get(alpine_js))
}
```
Wire into `mod.rs`: add `mod editor;` and `.merge(editor::editor_router())` in `create_router`.

- [ ] **Step 5: Write the route test, run, gates**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;
    fn st() -> WebState {
        let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        let app = std::sync::Arc::new(crate::state::AppState::for_tests(dir.path()));
        let (tx,_rx) = tokio::sync::broadcast::channel(8);
        WebState { app, signal_tx: tx }
    }
    #[tokio::test]
    async fn editor_page_and_assets_serve() {
        let r = editor_router().with_state(st());
        for path in ["/editor","/editor/editor.js","/editor/editor.css","/editor/widgets.js","/editor/alpine.js"] {
            let resp = r.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "{path} should 200");
        }
    }
}
```
Run: `cargo test -p ht32-panel-daemon web::editor 2>&1 | tail -10` → PASS.
Run clippy + fmt gates → clean + OK.

- [ ] **Step 6: Commit**

```bash
git add crates/ht32-panel-daemon/templates/editor.html crates/ht32-panel-daemon/assets/ crates/ht32-panel-daemon/src/web/editor.rs crates/ht32-panel-daemon/src/web/mod.rs
git commit -m "feat(web): /editor page shell + embedded (vendored) Alpine assets"
```
> The vendored `assets/alpine.min.js` is committed to the repo (embedded at build time). No CDN is contacted at runtime — the editor works offline and is not exposed to CDN compromise.

---

### Task 8: Palette add + property panel + Save/Activate

**Files:**
- Modify: `crates/ht32-panel-daemon/assets/editor.js` (implement `addWidget`, property fields, `save`, `load`, `activate`)
- Modify: `crates/ht32-panel-daemon/templates/editor.html` (property-panel field markup bound to schema)
- Test: Playwright smoke (added in Task 12); this task is verified manually + by the existing route tests.

**Interfaces:**
- Consumes: `/api/template-schema`, `/api/templates` (GET/POST/PUT), `POST /face`.
- Produces: a working forms-only editor (add widget with default content per kind; edit id/rect/binding/color/align; Save → POST or PUT; Activate → POST /face).

- [ ] **Step 1: Implement widget defaults + add/save/load/activate in `editor.js`**

```js
// default TemplateContent per kind (matches the serde wire format)
function defaultContent(kind, schema) {
  switch (kind) {
    case "text": return { kind:"text", value:{src:"hostname"}, size:12.0, color:"primary", align:"left" };
    case "bar": return { kind:"bar", value:"cpu_percent", fill:"primary", bg:2105376 };
    case "gauge": return { kind:"gauge", value:"cpu_temp", min:0.0, max:100.0, color:"primary", track:"background" };
    case "sparkline": return { kind:"sparkline", a:"disk_history", b:null, wrap_around:true,
                               color_a:"primary", color_b:"secondary", bg:0, scale:"auto" };
    case "clock": return { kind:"clock", mode:"digital", color:"text" };
  }
}
// merged into the editor() object returned in Task 7:
addWidget(kind) {
  const id = kind + "_" + (this.spec.widgets.length + 1);
  this.spec.widgets.push({ id, rect:{x:8,y:8,w:80,h:20}, ...defaultContent(kind, this.schema) });
  this.sel = this.spec.widgets.length - 1;
  this.refreshTruth();
},
async load() {
  if (!this.name) { this.spec = { name:"", orientation:"portrait", widgets:[] }; return; }
  this.spec = await (await fetch(`/api/templates/${this.name}`)).json();
  if (!this.spec.orientation) this.spec.orientation = "portrait";
  this.dims = this.spec.orientation.startsWith("portrait") ? [170,320] : [320,170];
  this.sel = null; this.refreshTruth();
},
async save() {
  this.spec.name = this.name;
  const exists = this.templates.includes(this.name);
  const url = exists ? `/api/templates/${this.name}` : "/api/templates";
  const method = exists ? "PUT" : "POST";
  const resp = await fetch(url, { method, headers:{ "content-type":"application/json" },
                                  body: JSON.stringify(this.spec) });
  if (resp.ok) { this.status = "saved"; this.templates = await (await fetch("/api/templates")).json(); }
  else { this.status = "error: " + (await resp.text()); }
},
async activate() {
  await this.save();
  const body = new URLSearchParams({ face: this.name });
  await fetch("/face", { method:"POST", body });
  this.status = "activated on panel";
},
```

- [ ] **Step 2: Add property-panel fields in `editor.html`** (bound to the selected widget; dropdowns from `schema`)

```html
<div class="props" x-show="sel!==null" x-data>
  <template x-if="sel!==null">
    <div>
      <label>id <input x-model="spec.widgets[sel].id"></label>
      <label>x <input type="number" x-model.number="spec.widgets[sel].rect.x" @input="refreshTruth()"></label>
      <label>y <input type="number" x-model.number="spec.widgets[sel].rect.y" @input="refreshTruth()"></label>
      <label>w <input type="number" x-model.number="spec.widgets[sel].rect.w" @input="refreshTruth()"></label>
      <label>h <input type="number" x-model.number="spec.widgets[sel].rect.h" @input="refreshTruth()"></label>

      <!-- bar/gauge numeric binding -->
      <template x-if="['bar','gauge'].includes(spec.widgets[sel].kind)">
        <label>source
          <select x-model="spec.widgets[sel].value" @change="refreshTruth()">
            <template x-for="s in schema.number_sources" :key="s"><option :value="s" x-text="s"></option></template>
          </select>
        </label>
      </template>

      <!-- text source -->
      <template x-if="spec.widgets[sel].kind==='text'">
        <label>text source
          <select x-model="spec.widgets[sel].value.src" @change="refreshTruth()">
            <template x-for="s in schema.text_sources" :key="s"><option :value="s" x-text="s"></option></template>
          </select>
        </label>
      </template>

      <button @click="spec.widgets.splice(sel,1); sel=null; refreshTruth()">Delete widget</button>
    </div>
  </template>
</div>
```

- [ ] **Step 3: Implement `refreshTruth()` (debounced POST to preview)**

```js
refreshTruth() {
  clearTimeout(this._t);
  this._t = setTimeout(async () => {
    const resp = await fetch("/api/templates/preview",
      { method:"POST", headers:{ "content-type":"application/json" }, body: JSON.stringify(this.spec) });
    if (!resp.ok) { this.status = "preview error"; return; }
    const j = await resp.json();
    this.truthSrc = "data:image/png;base64," + j.png_base64;
    this.warnings = j.warnings;
  }, 400);
},
```

- [ ] **Step 4: Manual verification (no automated test this step; Playwright in Task 12)**

Run the daemon locally (`cargo run -p ht32-panel-daemon -- <config>` with web enabled), open `http://localhost:8686/editor`, add a text + bar + gauge, watch the truth preview update, Save as `t8test`, verify the file:
`cat <state_dir>/templates/t8test.json` parses and contains the widgets.

- [ ] **Step 5: Gates + commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3 && cargo fmt --all -- --check && echo OK`
```bash
git add crates/ht32-panel-daemon/assets/editor.js crates/ht32-panel-daemon/templates/editor.html
git commit -m "feat(editor): palette add, schema-driven property panel, save/activate"
```

### Phase M2.3 hardware checkpoint (controller)

In a browser against pve3 (`http://192.168.1.53:8686/editor`): build the portrait dashboard via forms, Save, Activate. **Eyes on the physical panel** — it shows the face you just built in the browser.

---

## PHASE M2.4 — drag/resize + client render + live validation

### Task 9: `widgets.js` approximate client renderer

**Files:**
- Modify: `crates/ht32-panel-daemon/assets/widgets.js`
- Modify: `crates/ht32-panel-daemon/assets/editor.js` (call `renderWidget` per widget), `editor.html` (mount points)

**Interfaces:**
- Produces: `export function renderWidget(el, widget)` — paints an approximate representation of the widget kind into `el` (text string / bar fill / gauge arc on a `<canvas>` / sparkline polyline), using a module-level `SAMPLE` constant for illustrative values. Pure: same input → same DOM.

- [ ] **Step 1: Implement `widgets.js`**

```js
// Approximate, smooth client render of each widget kind for the editing canvas.
// NOT authoritative — the server PNG is. Values are illustrative sample data.
const SAMPLE = { cpu_percent:65, ram_percent:80, cpu_temp:72 };

export function renderWidget(el, w) {
  el.innerHTML = "";
  if (w.kind === "text") { el.textContent = previewText(w.value); el.style.color = "#cdd6f4"; }
  else if (w.kind === "bar") {
    const pct = (SAMPLE[w.value] ?? 50);
    el.style.background = "#202632";
    const fill = document.createElement("div");
    fill.style.cssText = `height:100%;width:${pct}%;background:#7aa2f7`;
    el.appendChild(fill);
  }
  else if (w.kind === "gauge" || w.kind === "sparkline" || w.kind === "clock") {
    const c = document.createElement("canvas");
    c.width = el.clientWidth; c.height = el.clientHeight; el.appendChild(c);
    const g = c.getContext("2d"); g.strokeStyle = "#7aa2f7"; g.lineWidth = 2;
    if (w.kind === "gauge") { g.beginPath(); g.arc(c.width/2, c.height/2, Math.min(c.width,c.height)/2-3, Math.PI*0.75, Math.PI*1.9); g.stroke(); }
    else if (w.kind === "sparkline") { g.beginPath(); for (let x=0;x<c.width;x++){ const y=c.height-(x/c.width)*c.height; x?g.lineTo(x,y):g.moveTo(x,y);} g.stroke(); }
    else { g.fillStyle="#cdd6f4"; g.font=`${Math.floor(c.height*0.6)}px monospace`; g.fillText("14:30", 2, c.height*0.7); }
  }
}
function previewText(value) {
  if (typeof value === "object" && value.src === "literal") return value.fmt || "text";
  if (typeof value === "object") return "{" + value.src + "}";
  return String(value);
}
```

- [ ] **Step 2: Call it from `editor.js`** — after the `x-for` renders widget boxes, render content into each. Use an Alpine `x-effect` or a `renderAll()` invoked on changes:

```js
import { renderWidget } from "/editor/widgets.js";
// in editor():
renderAll() {
  this.$nextTick(() => {
    document.querySelectorAll(".device .widget").forEach((el, i) => renderWidget(el, this.spec.widgets[i]));
  });
},
```
Call `this.renderAll()` at the end of `addWidget`, `load`, and after any property change (add `@input="renderAll()"` alongside the existing `refreshTruth()` calls). Remove the `x-text="w.id"` from the widget div so `renderWidget` owns its content.

- [ ] **Step 3: Manual verification + gates + commit**

Open `/editor`, confirm bars/gauges/sparklines render as smooth approximations on the editing canvas (left) while the truth PNG (right) matches after the debounce.
```bash
git add crates/ht32-panel-daemon/assets/widgets.js crates/ht32-panel-daemon/assets/editor.js crates/ht32-panel-daemon/templates/editor.html
git commit -m "feat(editor): client-side approximate widget renderer"
```

---

### Task 10: Pointer drag/resize + grid snap + bounds clamp

**Files:**
- Modify: `crates/ht32-panel-daemon/assets/editor.js`, `editor.html` (resize handle element)

**Interfaces:**
- Produces: drag a widget to move; drag a corner handle to resize; positions snap to a 2px device-grid; a widget cannot be dragged fully off the device.

- [ ] **Step 1: Implement drag/resize in `editor.js`**

First, add a module-level helper at the top of `editor.js` (before `window.editor`):
```js
function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }
```

Then add the handler to the `editor()` object. Note `mouse` (pointer coords) and `rect`
(the widget's original box) are kept as **separate** fields to avoid any `x`/`y` name
collision:
```js
// Drag to move or resize the selected widget. Screen px -> device px via this.scale;
// positions snap to a 2px device grid and clamp so the widget can't leave the screen.
startDrag(i, ev, mode /* 'move' | 'resize' */) {
  ev.preventDefault();
  this.sel = i;
  const w = this.spec.widgets[i];
  const mouse = { x: ev.clientX, y: ev.clientY };          // pointer origin (screen px)
  const rect = { x: w.rect.x, y: w.rect.y, w: w.rect.w, h: w.rect.h }; // original box (device px)
  const [dw, dh] = this.dims;
  const onMove = (e) => {
    const dx = Math.round((e.clientX - mouse.x) / this.scale / 2) * 2;
    const dy = Math.round((e.clientY - mouse.y) / this.scale / 2) * 2;
    if (mode === "move") {
      w.rect.x = clamp(rect.x + dx, 0, dw - w.rect.w);
      w.rect.y = clamp(rect.y + dy, 0, dh - w.rect.h);
    } else {
      w.rect.w = clamp(rect.w + dx, 4, dw - w.rect.x);
      w.rect.h = clamp(rect.h + dy, 4, dh - w.rect.y);
    }
    this.renderAll();
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    this.refreshTruth();
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
},
```

- [ ] **Step 2: Wire handlers in `editor.html`**

Replace the widget div's `@mousedown="select(i)"` with `@mousedown="startDrag(i,$event,'move')"`, and add a resize handle:
```html
<div class="widget" :class="{selected:i===sel, warn: warnIds.includes(w.id)}"
     @mousedown="startDrag(i,$event,'move')" :style="widgetStyle(w)">
  <span class="handle" @mousedown.stop="startDrag(i,$event,'resize')"></span>
</div>
```
Add to `editor.css`: `.handle{position:absolute;right:0;bottom:0;width:10px;height:10px;background:var(--accent);cursor:se-resize}`.

- [ ] **Step 3: Manual verification + gates + commit**

Open `/editor`, drag and resize widgets; confirm they snap to 2px and can't leave the device; the truth preview updates on release.
```bash
git add crates/ht32-panel-daemon/assets/editor.js crates/ht32-panel-daemon/templates/editor.html crates/ht32-panel-daemon/assets/editor.css
git commit -m "feat(editor): pointer drag/resize with grid snap and bounds clamp"
```

---

### Task 11: Inline overflow warnings on the editing canvas

**Files:**
- Modify: `crates/ht32-panel-daemon/assets/editor.js` (consume `warnings` from preview response)

**Interfaces:**
- Consumes: `warnings: [{widget_id, message}]` from `/api/templates/preview`.
- Produces: `warnIds` computed array; warned widgets get the `.warn` outline; a plain-language list shows under the property panel.

- [ ] **Step 1: Track warnings in `editor.js`**

```js
// add to state: warnings: [],
// computed: derive ids whenever warnings change
get warnIds() { return (this.warnings || []).map(w => w.widget_id); },
```
(`refreshTruth` already sets `this.warnings = j.warnings;` from Task 8 Step 3.)

- [ ] **Step 2: Show the warning list in `editor.html`** (under the canvas)

```html
<div class="warnings" x-show="warnings && warnings.length">
  <template x-for="wn in warnings" :key="wn.widget_id">
    <div class="warnrow" x-text="wn.message"></div>
  </template>
</div>
```
CSS: `.warnings{position:absolute;bottom:8px;left:8px;color:#e0af68;font-size:12px}` and ensure `.canvaswrap` is `position:relative`.

- [ ] **Step 3: Manual verification + gates + commit**

Drag a text widget partly off-screen or shrink its box below the text width; confirm the yellow outline + a readable message appear, and editing is not blocked.
```bash
git add crates/ht32-panel-daemon/assets/editor.js crates/ht32-panel-daemon/templates/editor.html crates/ht32-panel-daemon/assets/editor.css
git commit -m "feat(editor): inline non-blocking overflow warnings"
```

---

### Task 12: Playwright smoke tests

**Files:**
- Create: `crates/ht32-panel-daemon/tests/editor_smoke.md` (a documented manual+MCP Playwright checklist) OR a scripted check. Since the repo has no JS test runner, use the MCP Playwright tools driven by the controller, and record the steps as a checklist committed to the repo.

**Interfaces:** none (end-to-end browser verification).

- [ ] **Step 1: Write the smoke checklist** `crates/ht32-panel-daemon/tests/editor_smoke.md`:

```md
# Editor smoke (MCP Playwright, run against a local daemon with web enabled)
1. navigate http://localhost:8686/editor → page has title "HT32 Template Editor"
2. click "+ text" → one .widget appears on .device
3. set property id="hi", text source="hostname"
4. wait for img.device[src^="data:image/png"] (truth preview rendered)
5. set name="smoke", click Save → POST /api/templates returns 200
6. GET /api/templates includes "smoke"
7. click Activate → POST /face 200
8. DELETE cleanup: the controller removes the smoke template
```

- [ ] **Step 2: Execute via MCP Playwright (controller)** — drive the 8 steps; capture a screenshot; assert the network calls returned 200. Record pass/fail in the task report.

- [ ] **Step 3: Commit the checklist**

```bash
git add crates/ht32-panel-daemon/tests/editor_smoke.md
git commit -m "test(editor): browser smoke checklist for the template editor"
```

### Phase M2.4 hardware checkpoint (controller — the milestone acceptance)

In a browser against pve3: design a BRAND-NEW face by dragging widgets (not one of the example templates), Save, Activate. **Eyes on the physical panel** — it shows the face you dragged together, and it matches the editor's truth preview. This is the end-goal demonstrated end-to-end: a user designing a custom LCD face in the browser and seeing it on hardware.

---

## Final whole-branch review

After Task 12, dispatch the final code-reviewer over the whole M2 branch (`scripts/review-package <merge-base> HEAD`). Then use superpowers:finishing-a-development-branch.

## Notes for the executor

- **Test double:** Tasks 2, 5, 7 need `AppState::for_tests(state_dir)` (no LCD). It is introduced in Task 2; later tasks consume it. If building strictly in order, Task 2 must add it.
- **Signal ordering:** Task 2 temporarily emits `DisplaySettingsChanged`; Task 3 introduces `TemplatesChanged` and switches the four call sites + the SSE map arm. Don't skip the switch.
- **`orientation.dimensions()`** must be `pub` on `ht32_panel_hw::Orientation` (used by `preview_render`). It already exists internally (used at `state.rs:305`); expose it if private.
- **base64:** `/api/templates/preview` uses base64; add `base64 = "0.22"` to the daemon crate if not already a direct dep.
- **Front-end tasks (8–11) are browser-verified**, not unit-tested — the heavy logic (CRUD, validation, schema, preview) is all server-side and unit-tested in Tasks 1–6. Keep it that way; do not move validation into JS.
