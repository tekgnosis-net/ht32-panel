# WS2 Phase 1a — Layout engine framework + professional-face pilot

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce the typed-widget `Layout` abstraction and prove it end-to-end on the active `professional` face, with byte-identical output (no visible change yet).

**Architecture:** A face gains an optional `layout()` that returns a `Layout` (a list of typed `Widget`s: `Text`/`Bar`/`DualSparkline`, each carrying a bounding `rect`, `Static|Dynamic` kind, and `cadence` for later phases). A pure `render_layout(canvas, &layout)` interprets widgets into existing `Canvas` calls. Faces migrate one at a time; until converted, a face's default `layout()` returns `None` and the legacy `render()` path is used — so the build and output never break mid-migration.

**Tech Stack:** Rust, `tiny_skia` canvas, `cargo test`. Faces live in `crates/ht32-panel-daemon/src/faces/`.

Spec: `docs/design/2026-06-02-ws2-zone-rendering.md` (rev 2). Branch: `feat/layout-engine` (= fork `main` with WS1 + this work).

## Global Constraints

- **Behaviour-preserving:** the rendered canvas for every face must stay byte-identical this phase. The pixel-equivalence test (Task 2) is the binding gate.
- **No transport changes yet:** still full `redraw()` every frame (partial updates are Phase 4). `rect`/`kind`/`cadence` fields are populated but unused until later phases — `wrap_around` stays `false` until Phase 2.
- **YAGNI:** implement only the widget kinds the existing faces need (`Text`, `Bar`, `DualSparkline`). `Gauge`/`Pie`/`Clock` kinds are deferred to the phase that needs them (template builder / clock-face conversion).
- **Web invariant:** the canvas is still rendered in full each frame; nothing here touches `get_screen_png` or the framebuffer path.

---

### Task 1: Layout framework + `render_layout` + render-path wiring

**Files:**
- Create: `crates/ht32-panel-daemon/src/faces/layout.rs`
- Modify: `crates/ht32-panel-daemon/src/faces/mod.rs` (add `pub mod layout;`, re-exports, default `Face::layout()`)
- Modify: `crates/ht32-panel-daemon/src/state.rs` (render path: prefer `layout()` over `render()`)

**Interfaces:**
- Produces:
  - `Rect { x: i32, y: i32, w: u32, h: u32 }`
  - `enum ZoneKind { Static, Dynamic }`
  - `enum Cadence { EveryFrame, Seconds(u32), OnChange }`
  - `enum WidgetContent { Text { text: String, x: i32, y: i32, size: f32, color: u32 }, Bar { x: i32, y: i32, w: u32, h: u32, percent: f64, fill: u32, bg: u32 }, DualSparkline { x: i32, y: i32, w: u32, h: u32, a: Vec<f64>, b: Vec<f64>, scale: f64, color_a: u32, color_b: u32, bg: u32, wrap_around: bool } }`
  - `struct Widget { id: &'static str, rect: Rect, kind: ZoneKind, cadence: Cadence, content: WidgetContent }`
  - `struct Layout { widgets: Vec<Widget> }` with `Layout::new()` and `Layout::push(&mut self, Widget)`
  - `fn render_layout(canvas: &mut Canvas, layout: &Layout)`
  - `Face::layout(&self, _canvas: &Canvas, _data: &SystemData, _theme: &Theme, _complications: &EnabledComplications) -> Option<Layout>` (default `None`)

- [ ] **Step 1: Write the failing test** (`faces/layout.rs`, `#[cfg(test)] mod tests`)

```rust
use super::*;
use crate::rendering::Canvas;

#[test]
fn render_layout_draws_text_and_bar_into_canvas() {
    let mut canvas = Canvas::new(60, 20);
    canvas.set_background(0x000000);
    canvas.clear();
    let mut layout = Layout::new();
    layout.push(Widget {
        id: "bar", rect: Rect { x: 0, y: 0, w: 40, h: 8 },
        kind: ZoneKind::Dynamic, cadence: Cadence::EveryFrame,
        content: WidgetContent::Bar { x: 0, y: 0, w: 40, h: 8, percent: 50.0, fill: 0xFFFFFF, bg: 0x202020 },
    });
    render_layout(&mut canvas, &layout);
    // Left half (filled) is white; right half is the bar background.
    let px = canvas.pixels(); // RGBA8, row-major, width*height*4
    let at = |x: usize, y: usize| -> (u8,u8,u8) {
        let i = (y * 60 + x) * 4; (px[i], px[i+1], px[i+2])
    };
    assert_eq!(at(2, 4), (255, 255, 255), "filled portion white");
    assert_eq!(at(38, 4), (0x20, 0x20, 0x20), "unfilled portion = bar bg");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ht32-panel-daemon faces::layout`
Expected: FAIL to compile — `Layout`, `Widget`, `render_layout` not found.

- [ ] **Step 3: Implement the framework**

Create `crates/ht32-panel-daemon/src/faces/layout.rs`:

```rust
//! Typed-widget Layout model for display faces.
//!
//! A face produces a `Layout` (a list of `Widget`s). `render_layout` interprets
//! the widgets into `Canvas` drawing calls. `rect`/`kind`/`cadence` describe each
//! widget's screen region and update policy; they are unused in this phase and
//! consumed by the per-zone scheduler and partial-update transport in later phases.

use crate::rendering::Canvas;

/// Bounding box of a widget, in canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Whether a widget is drawn once (static) or updated over time (dynamic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneKind {
    Static,
    Dynamic,
}

/// How often a dynamic widget refreshes (consumed by the Phase 3 scheduler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    EveryFrame,
    Seconds(u32),
    OnChange,
}

/// The drawable content of a widget. Only the kinds the current faces need.
#[derive(Debug, Clone)]
pub enum WidgetContent {
    Text {
        text: String,
        x: i32,
        y: i32,
        size: f32,
        color: u32,
    },
    Bar {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        percent: f64,
        fill: u32,
        bg: u32,
    },
    DualSparkline {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        a: Vec<f64>,
        b: Vec<f64>,
        scale: f64,
        color_a: u32,
        color_b: u32,
        bg: u32,
        /// Phase 2 flips this to true; false reproduces the legacy scrolling graph.
        wrap_around: bool,
    },
}

/// A positioned, typed widget with update metadata.
#[derive(Debug, Clone)]
pub struct Widget {
    pub id: &'static str,
    pub rect: Rect,
    pub kind: ZoneKind,
    pub cadence: Cadence,
    pub content: WidgetContent,
}

/// An ordered list of widgets composing a face.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub widgets: Vec<Widget>,
}

impl Layout {
    pub fn new() -> Self {
        Self { widgets: Vec::new() }
    }
    pub fn push(&mut self, widget: Widget) {
        self.widgets.push(widget);
    }
}

/// Draws every widget of `layout` onto `canvas`, in order.
pub fn render_layout(canvas: &mut Canvas, layout: &Layout) {
    for w in &layout.widgets {
        match &w.content {
            WidgetContent::Text { text, x, y, size, color } => {
                canvas.draw_text(*x, *y, text, *size, *color);
            }
            WidgetContent::Bar { x, y, w: bw, h: bh, percent, fill, bg } => {
                canvas.fill_rect(*x, *y, *bw, *bh, *bg);
                let fill_w = ((*bw as f64 * (percent / 100.0)) as u32).min(*bw);
                if fill_w > 0 {
                    canvas.fill_rect(*x, *y, fill_w, *bh, *fill);
                }
            }
            WidgetContent::DualSparkline { x, y, w: gw, h: gh, a, b, scale, color_a, color_b, bg, wrap_around: _ } => {
                // Phase 1: always the legacy dual graph. Phase 2 adds the wrap-around path.
                canvas.draw_dual_graph(*x, *y, *gw, *gh, a, b, *scale, *color_a, *color_b, *bg);
            }
        }
    }
}
```

In `crates/ht32-panel-daemon/src/faces/mod.rs`, add near the other module declarations / imports:

```rust
pub mod layout;
pub use layout::{Cadence, Layout, Rect, Widget, WidgetContent, ZoneKind};
```

Add the default method to the `Face` trait (inside `pub trait Face`, after `render`):

```rust
    /// Returns this face's typed-widget layout, or `None` to use the legacy
    /// `render()` path. Faces migrate to `Some(..)` one at a time.
    fn layout(
        &self,
        _canvas: &Canvas,
        _data: &SystemData,
        _theme: &Theme,
        _complications: &EnabledComplications,
    ) -> Option<layout::Layout> {
        None
    }
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p ht32-panel-daemon faces::layout`
Expected: PASS (1 test).

- [ ] **Step 5: Wire the render path to prefer `layout()`**

In `state.rs` `render_frame`, the canvas-render block currently reads (the face-render portion):

```rust
            render.canvas.clear();
            display.face.render(
                &mut render.canvas,
                &system_data,
                &theme,
                &display.complications,
            );
```

Replace with:

```rust
            let maybe_layout = display.face.layout(
                &render.canvas,
                &system_data,
                &theme,
                &display.complications,
            );
            render.canvas.clear();
            match maybe_layout {
                Some(lay) => faces::layout::render_layout(&mut render.canvas, &lay),
                None => display.face.render(
                    &mut render.canvas,
                    &system_data,
                    &theme,
                    &display.complications,
                ),
            }
```

(`faces` is already imported in `state.rs` as `use crate::faces::{self, ...}`.)

- [ ] **Step 6: Verify build + full suite (all faces still use `render()` → identical output)**

Run: `cargo build -p ht32-panel-daemon && cargo test -p ht32-panel-daemon`
Expected: builds clean; existing tests pass; new layout test passes.

- [ ] **Step 7: Commit**

```bash
git add crates/ht32-panel-daemon/src/faces/layout.rs \
        crates/ht32-panel-daemon/src/faces/mod.rs \
        crates/ht32-panel-daemon/src/state.rs
git commit -m "feat(faces): add typed-widget Layout framework + render-path wiring"
```

---

### Task 2: Convert the `professional` face to a `Layout` (pilot) with a pixel-equivalence guard

**Files:**
- Modify: `crates/ht32-panel-daemon/src/faces/professional.rs` (add `layout()`; keep `render()` until all faces convert)
- Test: `crates/ht32-panel-daemon/src/faces/professional.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Layout`, `Widget`, `WidgetContent`, `Rect`, `ZoneKind`, `Cadence`, `render_layout` (Task 1).
- Produces: `impl Face for ProfessionalFace { fn layout(..) -> Option<Layout> }` returning `Some`.

**Correctness gate:** the binding requirement is byte-identical output. The equivalence test renders the same `SystemData`/`Theme`/`complications` two ways — through the existing `render()` and through `layout()`+`render_layout()` — and asserts the two RGBA pixel buffers are equal. The `layout()` body must therefore reproduce `render()`'s exact positions, colors, strings, and draw order (same `margin`, `FONT_*`, `BAR_*`, `GRAPH_HEIGHT`, the `FaceColors::from_theme` palette, the incremental `y` advance, and `canvas.text_width`/`line_height` measurements — which is why `layout()` receives `&Canvas`).

- [ ] **Step 1: Write the failing equivalence test**

Add to `professional.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::layout::render_layout;
    use crate::faces::{EnabledComplications, Face};
    use crate::faces::Theme;
    use crate::rendering::Canvas;
    use crate::sensors::data::SystemData;

    // Deterministic sample data so both render paths see identical input.
    fn sample() -> SystemData {
        let mut d = SystemData::default();
        d.hostname = "endeavour".into();
        d.uptime = "5d 12h".into();
        d.cpu_percent = 45.0;
        d.ram_percent = 67.0;
        d.cpu_temp = Some(45.0);
        d.hour = 18; d.minute = 45;
        d.disk_read_history = vec![0.1, 0.5, 0.9, 0.3];
        d.disk_write_history = vec![0.2, 0.4, 0.1, 0.6];
        d.net_rx_history = vec![0.3, 0.7, 0.2, 0.8];
        d.net_tx_history = vec![0.1, 0.2, 0.5, 0.4];
        d
    }

    fn render_both(width: u32, height: u32) -> (Vec<u8>, Vec<u8>) {
        let face = ProfessionalFace::new();
        let data = sample();
        let theme = Theme::from_preset("default");
        let comps = EnabledComplications::new();

        let mut legacy = Canvas::new(width, height);
        legacy.clear();
        face.render(&mut legacy, &data, &theme, &comps);

        let mut via_layout = Canvas::new(width, height);
        let lay = face.layout(&via_layout, &data, &theme, &comps).expect("layout() should be Some");
        via_layout.clear();
        render_layout(&mut via_layout, &lay);

        (legacy.pixels().to_vec(), via_layout.pixels().to_vec())
    }

    #[test]
    fn layout_matches_legacy_render_landscape() {
        let (legacy, via_layout) = render_both(320, 170);
        assert_eq!(legacy, via_layout, "landscape: layout output must equal render()");
    }

    #[test]
    fn layout_matches_legacy_render_portrait() {
        let (legacy, via_layout) = render_both(170, 320);
        assert_eq!(legacy, via_layout, "portrait: layout output must equal render()");
    }
}
```

(If `SystemData` lacks a `Default`, the test adds `#[derive(Default)]` to it in `sensors/data.rs`, or constructs it field-by-field — confirm against the actual struct before running.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ht32-panel-daemon professional`
Expected: FAIL — `layout()` returns `None` (default), so `.expect("layout() should be Some")` panics.

- [ ] **Step 3: Implement `layout()` mirroring `render()`**

Add an `impl ProfessionalFace` helper that builds the `Layout`, then implement `Face::layout`. The body mirrors `render()` exactly but, instead of calling `canvas.draw_text`/`draw_progress_bar`/`draw_dual_graph`, it pushes the equivalent `Widget`s (`Text`/`Bar`/`DualSparkline`) computed from the same expressions. Reuse `FaceColors::from_theme(theme)`, the `FONT_LARGE/NORMAL/SMALL`, `BAR_WIDTH/HEIGHT`, `GRAPH_HEIGHT` constants, and `canvas.text_width`/`canvas.line_height` for positioning. Each text element → `WidgetContent::Text`; each `draw_progress_bar(..)` → `WidgetContent::Bar`; each `draw_dual_graph(..)` → `WidgetContent::DualSparkline { wrap_around: false, .. }`. Assign each a stable `id` (e.g. `"hostname"`, `"cpu_bar"`, `"net_graph"`), a `rect` matching its drawn region, `ZoneKind::Static` for hostname/IP/labels and `Dynamic` for values/bars/graphs, and a `Cadence` (`Seconds(60)` for time/uptime, `EveryFrame` for graphs, `OnChange` for cpu/ram/temp) — these fields are inert this phase but seed Phase 3.

The exact element list and ordering to reproduce is the landscape + portrait branches of the current `render()` (professional.rs); the equivalence test in Step 1 is the precise acceptance criterion — iterate `layout()` until both assertions pass.

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p ht32-panel-daemon professional`
Expected: PASS — both `layout_matches_legacy_render_landscape` and `_portrait`.

- [ ] **Step 5: Full suite + clippy**

Run: `cargo test -p ht32-panel-daemon && cargo clippy -p ht32-panel-daemon -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/ht32-panel-daemon/src/faces/professional.rs
git commit -m "feat(faces): convert professional face to typed-widget Layout (byte-identical)"
```

---

## What this phase deliberately leaves for later plans

- **Phase 1b:** convert the other four faces (`arcs`, `ascii`, `clock`, `digits`) to `layout()` — `arcs`/`clock` will need a `Gauge`/`Clock` kind or a `Custom` escape; add those kinds then. Once all faces return `Some`, drop the legacy `render()` and make `layout()` non-optional.
- **Phase 2:** wrap-around sparkline (`DualSparkline { wrap_around: true }` + `Canvas::draw_dual_graph` wrap mode).
- **Phase 3:** persistent composite canvas + per-widget cadence scheduler (consumes `rect`/`kind`/`cadence`).
- **Phase 4:** framebuffer-space partial-update transport (per-rect diff → tiled `0xA2`), and the heartbeat-noise fix (per-write-type throttle) folded in along the way.

## Self-review notes

- **Spec coverage (Phase 1a slice):** typed `Layout`/`Widget` model ✓ (Task 1); `render_layout` interpreter ✓; incremental face migration via default `layout()=None` ✓; behaviour-preserving guard ✓ (Task 2 pixel-equivalence, both orientations); `rect`/`kind`/`cadence`/`wrap_around` fields seeded for Phases 2–4 ✓. Phases 1b–4 are explicitly deferred to their own plans.
- **Placeholder scan:** Task 1 is fully coded. Task 2's `layout()` body is specified by the equivalence test as its exact acceptance gate (the mechanical re-expression of `render()`); test harness + interfaces are complete.
- **Type consistency:** `Layout`/`Widget`/`WidgetContent`/`Rect`/`ZoneKind`/`Cadence`/`render_layout` and `Face::layout(&Canvas, &SystemData, &Theme, &EnabledComplications) -> Option<Layout>` are used identically in Tasks 1 and 2 and the render-path wiring.
- **Pre-flight to confirm during execution:** `Canvas::pixels() -> &[u8]` RGBA8 (used by tests); `SystemData` field names/`Default`; `Theme::from_preset` signature — all referenced from the current code, verify before running each test.
