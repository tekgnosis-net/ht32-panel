# WS5 M2 GUI Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the control panel (`/`) and the template editor (`/editor`) together so saved templates appear in the face selector, can be activated/edited/deleted from the panel, and the editor can open a named template via query-string.

**Architecture:** Five coordinated changes: (1) a shared Rust helper replaces the duplicated face-list builder; (2) `FaceTemplate` gains a `templates` field and `face.html` renders a "Your templates" section with activate/edit/delete controls; (3) `POST /face/delete` handles deletion; (4) `base.html` gains a nav link to the editor; (5) `editor.js` reads `?name=` on init, and `editor.html` gains a back-link. A new SSE listener in `index.html` ensures live-refresh when templates change.

**Tech Stack:** Rust/Axum/Askama (server), HTMX 2 (partials), Alpine.js (editor), vanilla JS (SSE).

## Global Constraints

- `cargo build -p ht32-panel-daemon`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo test -p ht32-panel-daemon` must all pass clean before commit.
- editor.js MUST remain a classic script (no `import`/`type=module`).
- No author email in any file or comment.
- Do not touch renderer or USB path code.
- Follow existing HTMX patterns (`hx-post`/`hx-target`/`hx-swap`) and Askama style.
- DRY: face-list-building logic lives in exactly one place (`render_face_partial`).
- Commit message: `feat(web): surface template editor + saved templates in the control panel`
- Report path: `/home/kumar/workspace/ht32-panel/.superpowers/sdd/task-15-report.md`

---

### Task 1: Rust — shared helper + `FaceTemplate.templates` + `/face/delete` route

**Files:**
- Modify: `crates/ht32-panel-daemon/src/web/mod.rs`

**Interfaces:**
- Produces: `fn render_face_partial(state: &WebState) -> Html<String>` — builds `faces` (built-ins) + `templates = state.app.template_names()` + `current = state.app.face_name()` and renders `FaceTemplate`.
- Produces: `FaceTemplate { current: String, faces: Vec<FaceOption>, templates: Vec<String> }` — the updated struct.
- Produces: route `POST /face/delete` handled by `face_delete`.

- [ ] **Step 1: Add `templates` field to `FaceTemplate`**

In `mod.rs` around line 68, change:

```rust
/// Face partial template.
#[derive(Template)]
#[template(path = "partials/face.html")]
struct FaceTemplate {
    current: String,
    faces: Vec<FaceOption>,
    templates: Vec<String>,
}
```

- [ ] **Step 2: Add the shared `render_face_partial` helper**

Add this function after the existing `face_set` handler (around line 307), before `led_get`:

```rust
/// Shared helper: build and render the face partial from current state.
fn render_face_partial(state: &WebState) -> Html<String> {
    let current = state.app.face_name();
    let faces: Vec<FaceOption> = available_faces()
        .iter()
        .map(|f| FaceOption {
            id: f.id.to_string(),
            display_name: f.display_name.to_string(),
        })
        .collect();
    let templates = state.app.template_names();
    Html(FaceTemplate { current, faces, templates }.render().unwrap())
}
```

- [ ] **Step 3: Replace bodies of `face_get` and `face_set` to call the helper**

Replace `face_get` (around line 271):

```rust
/// GET /face - Face controls partial
async fn face_get(State(state): State<WebState>) -> impl IntoResponse {
    render_face_partial(&state)
}
```

Replace `face_set` (around line 290):

```rust
/// POST /face - Set face
async fn face_set(State(state): State<WebState>, Form(form): Form<FaceForm>) -> Response {
    if let Err(e) = state.app.set_face(&form.face) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to set face: {}", e),
        )
            .into_response();
    }
    render_face_partial(&state).into_response()
}
```

- [ ] **Step 4: Add `DeleteForm` struct and `face_delete` handler**

Add after `FaceForm` (around line 287):

```rust
/// Form data for template deletion.
#[derive(Deserialize)]
struct DeleteForm {
    name: String,
}

/// POST /face/delete - Delete a saved template
async fn face_delete(
    State(state): State<WebState>,
    Form(form): Form<DeleteForm>,
) -> Response {
    if let Err(e) = state.app.delete_template(&form.name) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete template: {}", e),
        )
            .into_response();
    }
    let _ = state.signal_tx.send(DaemonSignals::TemplatesChanged);
    render_face_partial(&state).into_response()
}
```

- [ ] **Step 5: Register `POST /face/delete` in `create_router`**

In `create_router` (around line 163), add the new route after the `/face` line:

```rust
        .route("/face", get(face_get).post(face_set))
        .route("/face/delete", post(face_delete))
```

- [ ] **Step 6: Build and check**

```bash
cargo build -p ht32-panel-daemon 2>&1 | tail -20
```

Expected: no errors. If clippy fires on unused field or missing Askama template field, fix before proceeding.

- [ ] **Step 7: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -30
```

Expected: no warnings or errors.

---

### Task 2: face.html — "Your templates" section with activate / edit / delete

**Files:**
- Modify: `crates/ht32-panel-daemon/templates/partials/face.html`

**Interfaces:**
- Consumes: `FaceTemplate { current: String, faces: Vec<FaceOption>, templates: Vec<String> }` from Task 1.

- [ ] **Step 1: Read the current file to confirm exact whitespace**

```
/home/kumar/workspace/ht32-panel/crates/ht32-panel-daemon/templates/partials/face.html
```

Current content (11 lines):
```html
<form hx-post="/face" hx-target="#face-controls" hx-swap="innerHTML" hx-disabled-elt="find button">
    <div class="controls">
        {% for face in faces %}
        <button type="submit" name="face" value="{{ face.id }}" class="btn{% if face.id == current %} active{% endif %}">
            {{ face.display_name }}
        </button>
        {% endfor %}
        <span class="htmx-indicator spinner"></span>
    </div>
</form>
```

- [ ] **Step 2: Replace the entire file with the new content**

```html
<form hx-post="/face" hx-target="#face-controls" hx-swap="innerHTML" hx-disabled-elt="find button">
    <div class="controls">
        {% for face in faces %}
        <button type="submit" name="face" value="{{ face.id }}" class="btn{% if face.id == current %} active{% endif %}">
            {{ face.display_name }}
        </button>
        {% endfor %}
        <span class="htmx-indicator spinner"></span>
    </div>
</form>

{% if templates|length > 0 %}
<h2>Your templates</h2>
{% for t in templates %}
<div class="controls" style="margin-top:0.5rem;align-items:center;">
    <button type="submit" form="face-form-{{ loop.index }}" name="face" value="{{ t }}"
            class="btn{% if t == current %} active{% endif %}">
        {{ t }}
    </button>
    <a class="btn" href="/editor?name={{ t }}">Edit</a>
    {% if t != current %}
    <form hx-post="/face/delete" hx-target="#face-controls" hx-swap="innerHTML"
          hx-confirm="Delete template {{ t }}?">
        <button class="btn" name="name" value="{{ t }}">Delete</button>
    </form>
    {% endif %}
</div>
<form id="face-form-{{ loop.index }}" hx-post="/face" hx-target="#face-controls" hx-swap="innerHTML"
      style="display:none"></form>
{% endfor %}
{% else %}
<p class="hint" style="margin-top:0.5rem;">No custom faces yet — <a href="/editor">open the Template Editor</a></p>
{% endif %}
```

Note: The activate button for custom templates needs its own hidden form (since each template row has Edit/Delete alongside it and putting everything in one form is messy). The hidden `face-form-N` approach associates the activate button with its own per-template form via the HTML `form=` attribute, which is valid HTML5.

- [ ] **Step 3: Build to verify Askama renders the template without errors**

```bash
cargo build -p ht32-panel-daemon 2>&1 | grep -E "error|warning" | head -20
```

Expected: no errors related to `face.html`.

---

### Task 3: base.html — nav link to the Template Editor

**Files:**
- Modify: `crates/ht32-panel-daemon/templates/base.html`

**Interfaces:**
- None (purely presentational).

- [ ] **Step 1: Add a "Template Editor" nav link to the `<h1>` header in base.html**

The current `<h1>` block (line 172) is:

```html
        <h1>HT32 Panel</h1>
```

Replace it with a header row that holds the title and a nav link:

```html
        <div style="display:flex;align-items:baseline;justify-content:space-between;margin-bottom:1rem;">
            <h1 style="margin-bottom:0;">HT32 Panel</h1>
            <a class="btn" href="/editor" style="font-size:0.85rem;">Template Editor</a>
        </div>
```

- [ ] **Step 2: Build to verify**

```bash
cargo build -p ht32-panel-daemon 2>&1 | grep "error" | head -10
```

Expected: clean.

---

### Task 4: index.html — SSE listener for `templates` event

**Files:**
- Modify: `crates/ht32-panel-daemon/templates/index.html`

**Interfaces:**
- None (purely JS wiring).

- [ ] **Step 1: Add the `templates` SSE listener**

In `index.html`, the existing SSE listeners end around line 93. After the `led` listener (around line 92) and before `evtSource.onerror`, add:

```js
    evtSource.addEventListener('templates', function(e) {
        htmx.trigger('#face-controls', 'load');
    });
```

The full block after the change looks like:

```js
    evtSource.addEventListener('led', function(e) {
        htmx.trigger('#led-controls', 'load');
    });

    evtSource.addEventListener('templates', function(e) {
        htmx.trigger('#face-controls', 'load');
    });

    evtSource.onerror = function() {
```

- [ ] **Step 2: Build to verify (templates are pure HTML, no Rust change)**

```bash
cargo build -p ht32-panel-daemon 2>&1 | grep "error" | head -10
```

Expected: clean.

---

### Task 5: editor.js — open named template from `?name=` + editor.html back-link

**Files:**
- Modify: `crates/ht32-panel-daemon/assets/editor.js`
- Modify: `crates/ht32-panel-daemon/templates/editor.html`

**Interfaces:**
- None externally (self-contained editor changes).

- [ ] **Step 1: Add `?name=` handling at the end of `init()` in editor.js**

Current `init()` (lines 25-29):

```js
    async init() {
      this.schema = await (await fetch("/api/template-schema")).json();
      this.templates = await (await fetch("/api/templates")).json();
      this.dims = this.spec.orientation.startsWith("portrait") ? [170,320] : [320,170];
    },
```

Replace with:

```js
    async init() {
      this.schema = await (await fetch("/api/template-schema")).json();
      this.templates = await (await fetch("/api/templates")).json();
      this.dims = this.spec.orientation.startsWith("portrait") ? [170,320] : [320,170];
      const q = new URLSearchParams(location.search).get("name");
      if (q) { this.name = q; await this.load(); }
    },
```

- [ ] **Step 2: Add "← Control panel" link to editor.html topbar**

The topbar in `editor.html` currently starts (line 18):

```html
    <div class="topbar">
      <strong>Template</strong>
```

Replace with:

```html
    <div class="topbar">
      <a href="/" style="color:inherit;text-decoration:none;font-size:0.85rem;">&#8592; Control panel</a>
      <strong>Template</strong>
```

- [ ] **Step 3: Build to confirm no compilation errors from asset changes**

Assets are embedded at compile time via `include_dir!` or similar — verify:

```bash
cargo build -p ht32-panel-daemon 2>&1 | grep "error" | head -10
```

Expected: clean.

---

### Task 6: Full gates + commit

**Files:**
- No new files (commit covers all changed files above).

- [ ] **Step 1: Run full gate suite**

```bash
cargo build -p ht32-panel-daemon 2>&1 | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
cargo fmt --all -- --check 2>&1
cargo test -p ht32-panel-daemon 2>&1 | tail -20
```

Expected for each:
- `cargo build`: `Finished` line, no errors.
- `cargo clippy`: no output (all warnings = errors policy).
- `cargo fmt --check`: no output (no formatting issues).
- `cargo test`: `test result: ok. N passed; 0 failed`.

- [ ] **Step 2: Fix any clippy/fmt issues**

If `cargo fmt --check` prints diffs, run `cargo fmt --all` and re-check. If clippy fires, fix the indicated line. Do not suppress with `#[allow]` unless the lint is clearly a false positive.

- [ ] **Step 3: Stage and commit**

```bash
git -C /home/kumar/workspace/ht32-panel add \
    crates/ht32-panel-daemon/src/web/mod.rs \
    crates/ht32-panel-daemon/templates/base.html \
    crates/ht32-panel-daemon/templates/index.html \
    crates/ht32-panel-daemon/templates/partials/face.html \
    crates/ht32-panel-daemon/templates/editor.html \
    crates/ht32-panel-daemon/assets/editor.js

git -C /home/kumar/workspace/ht32-panel commit -m "$(cat <<'EOF'
feat(web): surface template editor + saved templates in the control panel

- Refactor face_get/face_set to shared render_face_partial helper (DRY)
- FaceTemplate gains templates: Vec<String> from AppState::template_names()
- face.html shows saved templates with activate/edit/delete controls
- POST /face/delete route calls delete_template + emits TemplatesChanged
- base.html header gains Template Editor nav link
- index.html SSE listener for 'templates' event refreshes face-controls
- editor.js init() reads ?name= query param and auto-loads that template
- editor.html topbar gains back-link to control panel
EOF
)"
```

- [ ] **Step 4: Write the task report**

Create `/home/kumar/workspace/ht32-panel/.superpowers/sdd/task-15-report.md` documenting:
- The shared helper (`render_face_partial`) signature and location.
- The new `/face/delete` route handler.
- The `face.html` template section structure (hidden form per template row for the HTML `form=` attribute trick).
- Where the `base.html` link was inserted.
- How `editor.js` `?name=` handling works (reads after `init` fetches, calls `load()`).
- Gate output (one line per command, pass/fail).
- Self-review: what could break (e.g., Askama `loop.index` availability, `hx-confirm` quoting with template names containing spaces).
- Concerns: none anticipated if gates pass.

---

## Self-Review

**Spec coverage check:**

| Requirement | Task |
|---|---|
| Piece 1: templates in face list, shared helper | Task 1 (Rust) + Task 2 (face.html) |
| Piece 2: Edit/Delete per template | Task 2 (face.html) + Task 1 (face_delete route) |
| Piece 3: link to editor from control panel | Task 3 (base.html) |
| Piece 4: editor opens named template, back-link | Task 5 (editor.js + editor.html) |
| Piece 5: live refresh on templates SSE event | Task 4 (index.html) |
| Gates (build/clippy/fmt/test) | Task 6 |
| Report | Task 6, Step 4 |

All spec requirements are covered.

**Placeholder scan:** No TBDs. All code blocks show exact content. Commands show expected output.

**Type consistency:**
- `FaceTemplate.templates: Vec<String>` defined in Task 1 and referenced in Task 2 template loop (`{% for t in templates %}`). Consistent.
- `render_face_partial(&state)` called in `face_get`, `face_set`, `face_delete` — all pass `&WebState`. Consistent.
- `DeleteForm { name: String }` matches the form button `name="name"` in Task 2. Consistent.
- `editor.js`: `this.name = q; await this.load();` — `name` and `load()` both exist in the Alpine object (confirmed from reading editor.js lines 18 and 90-99). Consistent.

**One concern to note in the report:** Template names containing special characters (spaces, quotes) in the `hx-confirm="Delete template {{ t }}?"` attribute could break the HTML attribute. The spec's existing templates use simple alphanumeric names, but if a template name contained a `"` the Askama output would be malformed. Askama does NOT auto-escape attribute contexts in this fashion; a note in the report is appropriate. Mitigation: instruct users to use simple names, or use `{{ t | e }}` if Askama's `e` filter escapes HTML attribute-unsafe chars (it does HTML-escape `&"<>`).

Fix the template to use Askama HTML escaping on `{{ t }}` inside attribute context. In Askama, the default rendering of `String` fields already applies HTML escaping when using `{{ }}` inside HTML. Verify this is correct — Askama's default is HTML-escape in `.html` templates, so `{{ t }}` in an attribute context will escape `&`, `<`, `>`, `"`, `'`. This means the attribute is safe.
