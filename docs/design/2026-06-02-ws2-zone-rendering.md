# WS2 — Layout engine: typed widgets, per-zone cadence, partial updates, wrap-around graphs

Status: DRAFT for review (rev 2) · 2026-06-02 · fork: tekgnosis-net/ht32-panel

## Context & problem

The display interface is a 64 B interrupt-OUT endpoint @ 1 ms ≈ **64 KB/s ceiling** (verified).
The daemon re-sends the **entire ~110 KB framebuffer** via full `redraw()` every `refresh_interval`
(default 2.5 s) → ~70 % endpoint duty → the MCU (16 KB SRAM, streams to GRAM, can't buffer a frame)
NAKs → **~28 % write failures even on a healthy device** (measured). `refresh()` (partial `0xA2`)
exists but is **never called**; faces are monolithic `render()` with no static/dynamic structure.

## Goals

1. Cut the display endpoint from ~70 % duty to ~1–2 %; transient NAKs vanish at source.
2. A **Layout engine** of typed, independently-scheduled widgets — the foundation for both optimal
   partial updates and a future user-facing template generator.
3. Web UI preview stays pixel-consistent with the LCD.

## Resolved design decisions (from review)

- **Layout type, not `Vec<Zone>`** — typed widgets so gauges/speedometers/pie charts are native and
  a generator can emit them declaratively. (`Custom(draw_fn)` escape retained for migration.)
- **Partial updates are the only path** (no escape-hatch flag) — consolidates the Layout model and
  the generator; full redraw only on structural change.
- **Per-zone cadence in the Layout** — each widget refreshes atomically on its own schedule.

## Design

### 1. Layout model (typed widgets)

A face returns a `Layout` instead of a monolithic `render()`:

```
Layout { widgets: Vec<Widget> }

Widget {
  id:        &str,
  rect:      Rect,                  // canvas coordinates
  kind:      Static | Dynamic,
  cadence:   EveryTick | Every(Duration) | OnChange,   // Dynamic only
  content:   WidgetKind,
  bind:      fn(&SystemData) -> WidgetValue,            // value(s) to render
  change_key: fn(&WidgetValue) -> u64,                  // skip repaint when unchanged
}

enum WidgetKind {
  Text { font, align },
  Bar  { orientation, .. },
  Gauge { min, max, arc.. },        // speedometer / radial
  Pie  { .. },
  Sparkline { dual: bool, wrap_around: true },
  Clock { analog | digital },
  Icon { .. },
  Custom(fn(&mut Canvas, &WidgetValue, &Theme)),   // freeform escape
}
```

- Typed kinds render via shared `Canvas` primitives; the set can grow over time.
- The five existing faces (arcs, ascii, clock, digits, professional) are refactored to `Layout`s;
  elements that don't map cleanly to a typed kind start as `Custom` and migrate later.
- A generator (fork roadmap) emits a `Layout` = typed widgets + rects + bindings + cadence.

### 2. Persistent composite canvas + per-zone scheduler

The render model changes from "clear + full render every frame" to a **persistent composite**:

- **Static** widgets are painted **once** (on (re)connect / face / theme / orientation change) and
  left in place.
- A **scheduler** ticks at a base `tick_interval`; each **Dynamic** widget is repainted **in its
  rect** only when its `cadence` is due *and* its `change_key` differs from last paint.
- The canvas therefore always holds the **complete current image** (static persists, dynamic
  updated in place) — no full clear, no per-frame full re-render.

### 3. Partial-update transport (USB only), atomic per widget

```
widget due + data changed ─► repaint widget.rect into canvas ─► canvas→framebuffer for that rect
                                                                      │
   diff that rect (prev vs new framebuffer, post-orientation) ─► tiled 0xA2 writes (≤2048px, ≤255)
```

- **Diff in framebuffer space** (post-orientation = exactly what's transmitted) and send via a new
  **framebuffer-space partial write** that does *not* re-apply rotation. (The existing `refresh()`
  re-rotates for canvas-space callers; reusing it would double-rotate.) This sidesteps orientation
  coordinate math entirely.
- A widget's update is **atomic**: its rect repaints and transmits independently of other widgets.
- **Full `redraw()`** (all 27 chunks) only on: first frame after (re)connect, face change, theme
  change, orientation change, `force_redraw()`.

### 4. Wrap-around graphs

`Sparkline { wrap_around: true }` (and `Canvas::draw_dual_graph`) render oscilloscope/`htop`-style:
sample *i* → column *i mod W*, 1-px gap at the write head. Pure function of `(history, count)`, so a
per-tick repaint of the rect changes only ~2–3 columns → the rect diff transmits ~48 px, not 4 864.

### 5. Web preview consistency

The web `/lcd.png` is a PNG of the same persistent composite canvas (`get_screen_png()`), so it is
always complete and shows the wrap-around graphs identically to the LCD. **Invariant:** the canvas
always holds the full composite; partial logic gates only *USB transmission*, never canvas
completeness. Headless mode (no LCD) still composes the canvas for the web.

## Config additions (`config.rs`)

| key | default | meaning |
|-----|---------|---------|
| `tick_interval_ms` | 250 | scheduler base tick; per-widget `cadence` is expressed against it |

`refresh_interval` is superseded by per-widget cadence (kept as a back-compat alias mapping to the
default dynamic cadence). No `partial_updates` flag — partial is the only path.

## Files to change

- `crates/ht32-panel-daemon/src/faces/*.rs` — `Face` returns `Layout`; refactor all 5 faces; add
  the `WidgetKind` set + shared widget renderers.
- `crates/ht32-panel-daemon/src/rendering/canvas.rs` — wrap-around `draw_dual_graph`; gauge/pie
  primitives as needed (some exist: `draw_arc`, `fill_circle`).
- `crates/ht32-panel-daemon/src/state.rs` — persistent composite canvas; per-widget scheduler;
  per-rect framebuffer diff → partial sends; full-redraw triggers; prev-framebuffer storage.
- `crates/ht32-panel-hw/src/lcd/device.rs` — framebuffer-space partial write (no re-rotation);
  rect tiling to ≤2048 px / ≤255 per `0xA2` packet.
- `crates/ht32-panel-daemon/src/config.rs` — `tick_interval_ms`; `refresh_interval` back-compat.

## Suggested build order (independently verifiable)

1. **Layout + widget kinds + face refactor**, behaviour-preserving (composite output byte-identical
   to today per face) — still using full redraw.
2. **Wrap-around sparkline** (visible; verify LCD + web).
3. **Persistent canvas + per-widget scheduler** (cadence honored; still full-frame transmit).
4. **Partial-update transport** (per-rect framebuffer diff + tiled `0xA2`) — the payoff.

## Testing strategy (TDD)

- **Pure/unit:** `change_key` skip; framebuffer-rect diff; rect→packet tiling boundary cases
  (304-px graph row → 3 tiles); wrap-around determinism (same `(history,count)` ⇒ identical pixels;
  +1 sample ⇒ only head columns differ); scheduler due-time logic per cadence.
- **Output-equivalence:** Layout composite == current monolithic render, per face (guards refactor).
- **Web:** `/lcd.png` complete + current after partial sends; headless still composes.
- **Manual (pve3):** journal `USB HID error` stream collapses; LCD + web show wrap-around identically;
  widgets update at their distinct cadences.

## Scope note

Rev 2 is materially larger than rev 1 (a typed widget engine + scheduler, not just a diff). See the
accompanying chat recommendation to split this into **its own PR**, separate from the small WS1
resilience fix, for upstream review tractability.
