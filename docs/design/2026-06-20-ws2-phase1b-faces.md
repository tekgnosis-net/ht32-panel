# WS2 Phase 1b — Migrate remaining faces to the Layout engine

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the four remaining faces (`ascii`, `digits`, `arcs`, `clock`) and `professional`'s ANALOGUE straggler to the typed-widget `Layout` model, then retire the legacy `render()` path so `Face::layout()` is the single, non-optional render path.

**Architecture:** Phase 1a built the `Layout`/`Widget`/`render_layout` framework and migrated `professional` (digital configs) as a byte-identical pilot. Phase 1b adds the remaining *pure-data* primitive widget kinds (`Arc`, `Line`, `Circle`) — each a 1:1 wrapper over an existing `Canvas` primitive — and rewrites each face's `layout()` to emit widgets whose draw args are identical to what its `render()` passes to the canvas. Because `render_layout` forwards to the *same* canvas calls, equivalence is by construction; a dual-path pixel-equality test proves it per face. Once every face+config returns `Some`, `render()` is deleted and `layout()` becomes non-optional.

**Tech Stack:** Rust, `tiny-skia` canvas, existing `Canvas` primitives (`draw_text`, `draw_text_scaled`, `fill_rect`, `draw_arc`, `draw_line`, `fill_circle`), `cargo test`/`clippy`.

## Global Constraints

- **Byte-identical migration.** For every face and every config/orientation, `render_layout(&mut c1, &face.layout(...).unwrap())` must produce a canvas whose `pixels()` are byte-identical to `face.render(&mut c2, ...)`. This is the acceptance gate for each face task. (Same guardrail Phase 1a used for `professional`.)
- **Pure data only — no `Custom(fn)` escape.** Widgets carry data; `render_layout` is the only place that touches `Canvas`. Do not add a closure/fn-pointer widget. (User decision, 2026-06-20.)
- **New widget kinds are 1:1 canvas wrappers.** `Arc`/`Line`/`Circle` fields mirror the `Canvas` primitive signatures exactly (below). Do not reinterpret geometry, rounding, or color in `render_layout` — forward verbatim.
- **Coverage both orientations.** Every face branches on `portrait = width < 200`. Equivalence tests must cover **landscape (320×170)** and **portrait (170×320)**, plus each face's distinct modes (e.g. clock analog vs digital).
- **YAGNI / no over-building.** Add only `Arc`, `Line`, `Circle`. No `Gauge`/`Pie`/`Clock` composite kinds — the faces decompose into the primitives. `rect`/`kind`/`cadence` metadata stays best-effort (consumed by later phases); set sensible values, don't invent a scheduler.
- **DRY, frequent commits, clippy `-D warnings` clean, `cargo fmt --all` before every commit** (the flake's `fmt` gate is CI-only; see repo memory).

## Canvas primitive signatures (the new widget kinds mirror these verbatim)

```
fill_circle(cx: i32, cy: i32, radius: u32, color: u32)
draw_line(x1: i32, y1: i32, x2: i32, y2: i32, stroke_width: f32, color: u32)
draw_arc(cx: i32, cy: i32, radius: u32, start_angle: f32, end_angle: f32, stroke_width: f32, color: u32)   // radians
draw_text(x: i32, y: i32, text: &str, size: f32, color: u32)
draw_text_scaled(x, y, text, size: f32, x_scale: f32, color: u32)   // confirm exact tail args in canvas.rs
fill_rect(x: i32, y: i32, width: u32, height: u32, color: u32)
```

## File Structure

- `crates/ht32-panel-daemon/src/faces/layout.rs` — add `Arc`/`Line`/`Circle` to `WidgetContent`, matching `render_layout` arms, unit tests (Task 1). Add `draw_text_scaled` mapping if any migrated face needs it (Task 5/6 — extend `Text` or add `TextScaled`).
- `crates/ht32-panel-daemon/src/faces/{ascii,digits,arcs,clock}.rs` — implement `layout()`, add dual-path equivalence test (Tasks 2–5).
- `crates/ht32-panel-daemon/src/faces/professional.rs` — migrate ANALOGUE branch, drop the `None` fallback, extend equivalence test (Task 6).
- `crates/ht32-panel-daemon/src/faces/mod.rs` — `Face::layout()` → non-optional `fn layout(...) -> Layout`; remove `render()` from the trait (Task 7).
- `crates/ht32-panel-daemon/src/state.rs` — update the call site: drop the `match Some/None → render()` fallback, call `layout()` directly (Task 7).

---

### Task 1: Primitive widget kinds (`Arc`, `Line`, `Circle`)

**Files:**
- Modify: `crates/ht32-panel-daemon/src/faces/layout.rs`
- Test: same file `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: three new `WidgetContent` variants + their `render_layout` arms, consumed by Tasks 4–6.

- [ ] **Step 1: Write failing unit tests** — one per kind, asserting a known pixel after `render_layout`. Pattern mirrors the existing `render_layout_draws_bar_into_canvas` test (sample a pixel via `canvas.pixels()`).

```rust
#[test]
fn render_layout_draws_line_circle_arc() {
    let mut canvas = Canvas::new(60, 60);
    canvas.set_background(0x000000);
    canvas.clear();
    let mut layout = Layout::new();
    layout.push(Widget { id: "ln", rect: Rect { x:0,y:0,w:60,h:60 }, kind: ZoneKind::Static,
        cadence: Cadence::OnChange,
        content: WidgetContent::Line { x1:5, y1:30, x2:55, y2:30, stroke:3.0, color:0xFFFFFF } });
    layout.push(Widget { id: "ci", rect: Rect { x:0,y:0,w:60,h:60 }, kind: ZoneKind::Static,
        cadence: Cadence::OnChange,
        content: WidgetContent::Circle { cx:30, cy:30, r:8, color:0xFF0000 } });
    render_layout(&mut canvas, &layout);
    let px = canvas.pixels();
    let at = |x: usize, y: usize| { let i=(y*60+x)*4; (px[i],px[i+1],px[i+2]) };
    assert_eq!(at(30, 30), (255, 0, 0), "circle center red");
    assert_eq!(at(30, 5),  (0, 0, 0),   "above the line stays bg");
}
```
(Add a separate arc test asserting a stroked pixel on the arc path is non-background.)

- [ ] **Step 2: Run, verify it fails** — `cargo test -p ht32-panel-daemon render_layout_draws_line_circle_arc` → fails to compile (variants absent).

- [ ] **Step 3: Add the variants** to `WidgetContent` (verbatim canvas arg names):

```rust
Line   { x1: i32, y1: i32, x2: i32, y2: i32, stroke: f32, color: u32 },
Circle { cx: i32, cy: i32, r: u32, color: u32 },
Arc    { cx: i32, cy: i32, r: u32, start_angle: f32, end_angle: f32, stroke: f32, color: u32 },
```

- [ ] **Step 4: Add `render_layout` arms** — forward verbatim, no reinterpretation:

```rust
WidgetContent::Line { x1, y1, x2, y2, stroke, color } =>
    canvas.draw_line(*x1, *y1, *x2, *y2, *stroke, *color),
WidgetContent::Circle { cx, cy, r, color } =>
    canvas.fill_circle(*cx, *cy, *r, *color),
WidgetContent::Arc { cx, cy, r, start_angle, end_angle, stroke, color } =>
    canvas.draw_arc(*cx, *cy, *r, *start_angle, *end_angle, *stroke, *color),
```

- [ ] **Step 5: Run tests** → PASS. `cargo clippy --workspace --all-targets -- -D warnings` clean. `cargo fmt --all`.
- [ ] **Step 6: Commit** — `feat(faces): add Line/Circle/Arc pure-data widget kinds`.

---

### Task 2: Migrate `ascii` face (pure `Text`)

**Files:**
- Modify: `crates/ht32-panel-daemon/src/faces/ascii.rs` (add `layout()`; `render()` stays as the equivalence reference)
- Test: same file

**Interfaces:**
- Consumes: `Text` widget (Phase 1a). Reference: this face's existing `render()` (vocabulary: `draw_text` ×33 only).

- [ ] **Step 1: Write the failing equivalence test** — landscape + portrait, default + a non-default complication set. Skeleton (reuse the helper pattern from `professional.rs`'s Phase-1a tests):

```rust
#[test]
fn ascii_layout_matches_render() {
    for (w, h) in [(320u32, 170u32), (170u32, 320u32)] {
        let face = AsciiFace::new();
        let (data, theme, comps) = sample_inputs();      // representative SystemData/Theme/complications
        let mut c_render = Canvas::new(w, h); c_render.set_background(0); c_render.clear();
        face.render(&mut c_render, &data, &theme, &comps);
        let mut c_layout = Canvas::new(w, h); c_layout.set_background(0); c_layout.clear();
        render_layout(&mut c_layout, &face.layout(&c_layout, &data, &theme, &comps).unwrap());
        assert_eq!(c_render.pixels(), c_layout.pixels(), "ascii mismatch at {w}x{h}");
    }
}
```

- [ ] **Step 2: Run, verify it fails** — `layout()` returns `None` (default trait impl) → `.unwrap()` panics.
- [ ] **Step 3: Implement `layout()`** — lift every `canvas.draw_text(x, y, s, size, color)` in `render()` into a `WidgetContent::Text` widget with identical args, in the same order. Compute the same strings/positions (same `portrait`, `text_width`, `line_height` logic). Return `Some(layout)`.
- [ ] **Step 4: Run test** → PASS for both orientations. If a pixel differs, the args diverged — fix the computation, not the test.
- [ ] **Step 5:** clippy clean, `cargo fmt --all`.
- [ ] **Step 6: Commit** — `feat(faces): convert ascii face to Layout (byte-identical)`.

---

### Task 3: Migrate `digits` face (`Text` + divider)

**Files:** Modify `crates/ht32-panel-daemon/src/faces/digits.rs`; test same file.

**Interfaces:** Consumes `Text` + `Bar`. Reference: `render()` (vocabulary: `draw_text` ×19 + one `fill_rect` divider).

- [ ] **Step 1: Failing equivalence test** — same skeleton as Task 2 (`digits_layout_matches_render`, landscape+portrait, default + alt complication).
- [ ] **Step 2: Run, verify it fails.**
- [ ] **Step 3: Implement `layout()`** — `draw_text` → `Text` widgets; the `draw_divider` `fill_rect(x, y, w, h, color)` → `WidgetContent::Bar { x, y, w, h, percent: 100.0, fill: color, bg: color }` (a 100% bar = solid fill, byte-identical to `fill_rect`). Verify the `Bar` arm produces the identical rect (it calls `fill_rect` for bg then fill; with `percent:100` and `fill==bg` the result is one solid rect — confirm against the divider). If any rounding differs, instead add a thin `WidgetContent::Rect { x,y,w,h,color }` 1:1 wrapper over `fill_rect` (add to Task 1 retroactively only if needed; prefer reuse).
- [ ] **Step 4: Run test** → PASS both orientations.
- [ ] **Step 5:** clippy + fmt.
- [ ] **Step 6: Commit** — `feat(faces): convert digits face to Layout (byte-identical)`.

---

### Task 4: Migrate `arcs` face (`Text` + `Arc` gauges)

**Files:** Modify `crates/ht32-panel-daemon/src/faces/arcs.rs`; test same file.

**Interfaces:** Consumes `Text` + `Arc` (Task 1). Reference: `render()` + helpers `draw_arc_gauge` (105), `draw_activity_arc` (149) — vocabulary `draw_text` ×34 + `draw_arc` ×4.

- [ ] **Step 1: Failing equivalence test** — `arcs_layout_matches_render`, landscape+portrait, default + a config that exercises every gauge (CPU/RAM/etc. complications enabled).
- [ ] **Step 2: Run, verify it fails.**
- [ ] **Step 3: Implement `layout()`** — the gauge helpers each end in `canvas.draw_arc(cx, cy, r, a0, a1, stroke, color)`; lift those exact args into `WidgetContent::Arc` widgets (compute the same angles from the same percentages). Text labels → `Text`. Preserve draw order (background arc track before the value arc, etc.).
- [ ] **Step 4: Run test** → PASS both orientations. Watch for `f32` angle computation: reproduce the helper's arithmetic exactly (same operations/order) so the `f32` bits match.
- [ ] **Step 5:** clippy + fmt.
- [ ] **Step 6: Commit** — `feat(faces): convert arcs face to Layout (byte-identical gauges)`.

---

### Task 5: Migrate `clock` face (`Text` + `Line`/`Circle`/`Arc`)

**Files:** Modify `crates/ht32-panel-daemon/src/faces/clock.rs`; test same file.

**Interfaces:** Consumes `Text`/`Line`/`Circle`/`Arc`. Reference: `render()` + `draw_digital_time` (100), `draw_analog_clock` (201), `draw_clock_face` (255) — vocabulary `draw_line` ×3 (hands), `fill_circle` ×1 (hub), `draw_arc` ×1 (bezel), `draw_text`/`draw_text_scaled`.

- [ ] **Step 1: Failing equivalence test** — `clock_layout_matches_render` covering **both modes** (analog and digital) × **both orientations**, at a **fixed clock time** (inject a fixed `SystemData` timestamp so hand angles are deterministic). If the digital path uses `draw_text_scaled`, first extend the `Text` widget or add a `TextScaled { x,y,text,size,x_scale,color }` variant to `layout.rs` (with a render_layout arm forwarding to `draw_text_scaled`) in this task — add a unit test for it like Task 1.
- [ ] **Step 2: Run, verify it fails.**
- [ ] **Step 3: Implement `layout()`** — analog: compute each hand's endpoint exactly as `draw_analog_clock` does, emit `Line` widgets with identical `(x1,y1,x2,y2,stroke,color)`; hub → `Circle`; bezel → `Arc`. Digital: `Text`/`TextScaled` widgets. Reproduce the portrait font-scaling branch (clock.rs:110–151).
- [ ] **Step 4: Run test** → PASS for analog+digital × landscape+portrait.
- [ ] **Step 5:** clippy + fmt.
- [ ] **Step 6: Commit** — `feat(faces): convert clock face to Layout incl. analog geometry (byte-identical)`.

---

### Task 6: Close `professional`'s ANALOGUE straggler

**Files:** Modify `crates/ht32-panel-daemon/src/faces/professional.rs`; test same file.

**Interfaces:** Consumes the Task-5 analog primitives. Phase 1a left `build_layout` returning `None` for the ANALOGUE-time complication (falls back to `render()`); this closes it so `professional.layout()` returns `Some` for **all** configs.

- [ ] **Step 1: Extend the existing professional equivalence test** to include the ANALOGUE-time config (the one that currently asserts `layout() == None`). Flip that assertion to expect byte-identical equivalence, landscape+portrait.
- [ ] **Step 2: Run, verify it fails** — ANALOGUE still returns `None`.
- [ ] **Step 3: Implement** — replace the `None` branch in `build_layout` with analog-clock widgets (`Line`/`Circle`/`Arc`) computed identically to professional's `render()` analog path; return `Some`.
- [ ] **Step 4: Run** the full professional test set → all PASS (digital configs unchanged, ANALOGUE now equivalent).
- [ ] **Step 5:** clippy + fmt.
- [ ] **Step 6: Commit** — `feat(faces): migrate professional ANALOGUE branch; layout() now total`.

---

### Task 7: Retire `render()`; make `layout()` the single path

**Files:** Modify `mod.rs` (trait), `state.rs` (call site), all five face files (delete `render()` + helpers only `render()` used), `layout.rs` (tests).

**Interfaces:** After this task `Face` has no `render()`; `fn layout(&self, ...) -> Layout` (non-optional). The Phase-1a/1b dual-path equivalence tests lose their `render()` reference — convert them to golden snapshots **before** deleting `render()`.

- [ ] **Step 1: Add golden-snapshot regression tests** — for each face × covered config/orientation, hash the canvas (`sha256(canvas.pixels())`) produced by the *current, proven-equivalent* `layout()` path and store the hash as a test constant. Assert `layout()` keeps producing it. (Generate the constants from a one-time run; commit the values.) This preserves regression protection without `render()`.
- [ ] **Step 2: Run** golden tests → PASS (they pin current output).
- [ ] **Step 3: Delete the dual-path equivalence tests** (their `render()` reference is going away; golden tests replace them).
- [ ] **Step 4: Make `layout()` non-optional** in `mod.rs`: `fn layout(&self, canvas: &Canvas, data: &SystemData, theme: &Theme, complications: &EnabledComplications) -> layout::Layout;` and remove `fn render(...)` from the trait.
- [ ] **Step 5: Delete each face's `render()`** and any private helpers used *only* by `render()` (keep helpers shared with `layout()`). Update the call site in `state.rs` — replace the `match face.layout(..) { Some(l) => render_layout, None => face.render(..) }` with `render_layout(&mut canvas, &face.layout(..))`.
- [ ] **Step 6: Run** `cargo test -p ht32-panel-daemon` (golden + framework) and `cargo build --workspace` → all PASS, no `render` references remain (`grep -rn 'fn render' crates/ht32-panel-daemon/src/faces` → only `render_layout`). clippy `-D warnings` clean, `cargo fmt --all`.
- [ ] **Step 7: Commit** — `refactor(faces): retire legacy render(); layout() is the sole render path`.

---

## Sequencing & validation

1. Task 1 (framework) first — unblocks the gauge/analog faces.
2. Tasks 2→3→4→5 in order (ascii simplest → clock hardest); each is independently byte-identical-gated and committable.
3. Task 6 closes professional's straggler using Task-5 machinery → `layout()` total across all faces.
4. Task 7 retires `render()` once every face+config is proven equivalent.
5. End state: single render path, golden-snapshot regression guard, ready for Phase 2 (wrap-around graphs + heartbeat-noise throttle) to build on `layout()` only.

## Notes

- The pixel-equality gate is the whole safety net — never weaken an assertion to make a test pass; a mismatch means the widget args diverged from `render()`.
- `draw_text_scaled` exact tail signature must be confirmed in `canvas.rs` before Task 5 (the head shows `x, y, text, size, x_scale, …`).
- Phase 2+ depends on `render()` being gone (so changes touch one path); do not defer Task 7 without revisiting that.
