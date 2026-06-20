# WS2 Phase 2 — Wrap-around (oscilloscope) graphs

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the scrolling dual-bar graph with an oscilloscope/htop-style **wrap-around** graph: sample *i* → column *i mod W*, with a 1-px gap at the write head. This is the first *visible* WS2 change and the precondition for Phase 4's tiny per-rect partial sends (a +1 sample changes only ~2–3 columns instead of the whole row).

**Architecture:** The `DualSparkline` widget already carries a `wrap_around` flag (currently `false` → legacy scrolling). Phase 2 (a) adds a pure-function wrap renderer to `Canvas`, (b) threads a monotonic per-history **sample count** from the sensors through `SystemData` into the widget, (c) dispatches `wrap_around: true` to the new renderer in `render_layout`, and (d) flips the only consumer — the `professional` face — to wrap mode and regenerates its golden snapshots. Bars are kept (not a line) — minimal visual change, re-styleable later.

**Tech Stack:** Rust, `tiny-skia` canvas, the Phase-1b typed-widget Layout engine, golden FNV-1a pixel-hash snapshots.

## Global Constraints

- **Only `professional` is affected.** It is the sole face emitting `DualSparkline` widgets (4 of them: disk + net × portrait/landscape). `ascii`/`digits` text sparklines are out of scope.
- **W = `HISTORY_SIZE` = 60** sample columns. The graph is `width` px wide; `bar_width = width / 60` (matches the existing `draw_dual_graph` convention of `width / num_points`).
- **Wrap is a pure function of `(data1, data2, count, max, …)`.** Determinism is the core invariant: same inputs ⇒ identical pixels; `count+1` (one new sample) ⇒ only the head column(s) differ. No hidden state in the renderer.
- **Behavioral parity for the web preview.** The daemon's web UI renders the same composite canvas (`state.rs` render path → PNG), so wrap-around must appear identically in `/lcd.png`. No separate web rendering exists — verify the shared path.
- **Golden snapshots:** professional's graph-bearing goldens WILL change (intended). Regenerate exactly those, and confirm non-graph goldens are unchanged.
- DRY, YAGNI, TDD, frequent commits; `cargo fmt --all` + `clippy --all-targets -D warnings` clean before each commit (CI's only fmt/clippy gate is `nix flake check`).

## Data-flow facts (verified)

- `state.rs:155` builds `SystemData`, cloning `self.disk.history()` (`disk_history`) and `self.network.history()` (`net_history`) — windowed `VecDeque<f64>`, cap 60, newest last, `pop_front`+`push_back` per sensor update (`disk.rs:135-147`, `network.rs:327-329`). **No monotonic counter exists.**
- `render_layout`'s `DualSparkline` arm (`layout.rs:167-183`) currently rebuilds `VecDeque`s from the widget's `Vec`s every call and calls `draw_dual_graph`. Phase 2 collapses that bridge.
- Render cadence = `refresh_interval` ms; sensor updates are independent — so the count MUST advance per *sample pushed*, not per render (else the head drifts when the sensor is idle).

## File Structure

- `crates/ht32-panel-daemon/src/rendering/canvas.rs` — add `draw_dual_graph_wrap` (pure, slice-based) (Task 1).
- `crates/ht32-panel-daemon/src/sensors/disk.rs`, `network.rs` — track a `samples_pushed: u64` counter; expose via a getter (Task 2).
- `crates/ht32-panel-daemon/src/sensors/data.rs` — add `disk_sample_count`/`net_sample_count: u64` to `SystemData` (Task 2).
- `crates/ht32-panel-daemon/src/state.rs:155` — populate the new counts from the sensors (Task 2).
- `crates/ht32-panel-daemon/src/faces/layout.rs` — add `count: u64` to `WidgetContent::DualSparkline`; dispatch wrap vs legacy in `render_layout` using slices (Task 3).
- `crates/ht32-panel-daemon/src/faces/professional.rs` — set `wrap_around: true` + pass `count` on its 4 DualSparkline widgets; regenerate its graph goldens (Task 4).

---

### Task 1: Wrap-around renderer (pure function) on `Canvas`

**Files:**
- Modify: `crates/ht32-panel-daemon/src/rendering/canvas.rs`
- Test: same file `#[cfg(test)] mod tests` (or wherever canvas tests live)

**Interfaces:**
- Produces: `pub fn draw_dual_graph_wrap(&mut self, x: i32, y: i32, width: u32, height: u32, data1: &[f64], data2: &[f64], count: u64, max_value: f64, color1: u32, color2: u32, bg_color: u32)`. Slice-based (not `VecDeque`) so the Task-3 widget passes `&vec[..]` with no conversion.

**Semantics (W = 60):**
- Fill background (same as `draw_dual_graph`). Return early if both empty or `max_value <= 0`.
- `bar_width = (width as f64 / 60.0).max(1.0)`.
- `len = data1.len().max(data2.len())` (≤ 60). For window-index `k` in `0..len`, the sample's absolute index is `count - len as u64 + k as u64`; its **column** `s = (abs_index % 60) as i32`; pixel `bar_x = x + (s as f64 * bar_width) as i32`. Draw the bar exactly as `draw_dual_graph` does (same normalize, same high/max color thresholds, same two-series layering) — only the x-placement formula changes.
- **Write-head gap:** after drawing, blank a 1-px-wide vertical strip (`bg_color`) at the head's leading pixel: head column `h = (count % 60) as i32`; `gap_x = x + (h as f64 * bar_width) as i32`; `fill_rect(gap_x, y, 1, height, bg_color)`. (The gap marks "next column to be overwritten".)
- Reuse the existing `brighten_color` and color-threshold logic verbatim — extract a shared private helper if it avoids duplication with `draw_dual_graph`.

- [ ] **Step 1: Write failing determinism + shape tests.**

```rust
#[test]
fn wrap_graph_is_deterministic_and_head_local() {
    let w = 60u32; let h = 20u32;
    let d1: Vec<f64> = (0..60).map(|i| (i % 10) as f64).collect();
    let d2: Vec<f64> = vec![0.0; 60];
    let mk = |count: u64| {
        let mut c = Canvas::new(w, h); c.set_background(0); c.clear();
        c.draw_dual_graph_wrap(0, 0, w, h, &d1, &d2, count, 9.0, 0xFF0000, 0x00FF00, 0x000000);
        c.pixels().to_vec()
    };
    // Determinism: same (data, count) -> identical pixels.
    assert_eq!(mk(100), mk(100), "wrap graph not deterministic");
    // Locality: count+1 changes only a few columns (the head region), not the whole row.
    let a = mk(100); let b = mk(101);
    let differing_cols = (0..w as usize).filter(|&x| {
        (0..h as usize).any(|y| { let i=(y*w as usize + x)*4; a[i..i+3] != b[i..i+3] })
    }).count();
    assert!(differing_cols <= 3, "count+1 changed {differing_cols} columns; expected <= 3 (head-local)");
}
```

- [ ] **Step 2: Run, verify it fails** (method absent) — `cargo test -p ht32-panel-daemon wrap_graph_is_deterministic_and_head_local`.
- [ ] **Step 3: Implement `draw_dual_graph_wrap`** per the semantics above.
- [ ] **Step 4: Run tests** → PASS. Add a small positive-pixel test (a known sample produces a non-bg bar at its expected column) to prove placement, not just locality.
- [ ] **Step 5:** clippy `-D warnings` clean; `cargo fmt --all`.
- [ ] **Step 6: Commit** — `feat(canvas): add wrap-around (oscilloscope) draw_dual_graph_wrap`.

---

### Task 2: Thread a monotonic sample count (sensors → SystemData)

**Files:**
- Modify: `crates/ht32-panel-daemon/src/sensors/disk.rs`, `network.rs` (counter + getter), `data.rs` (SystemData fields), `state.rs:155` (populate).
- Test: a unit test on the sensor counter; update existing `SystemData` constructors/`Default`.

**Interfaces:**
- Produces: `SystemData.disk_sample_count: u64`, `SystemData.net_sample_count: u64` (consumed by Task 3/4). Sensor getters e.g. `DiskSensor::sample_count(&self) -> u64`.

- [ ] **Step 1: Write the failing test** — pushing N samples advances the counter by N.

```rust
#[test]
fn disk_sensor_counts_samples() {
    let mut s = DiskSensor::new();
    let before = s.sample_count();
    // drive two history updates (use the same path update() uses; if update() needs /proc,
    // factor the history-append into a testable `record(combined, read, write)` and call it).
    s.record_for_test(1.0, 0.5, 0.5);
    s.record_for_test(2.0, 1.0, 1.0);
    assert_eq!(s.sample_count(), before + 2);
}
```

- [ ] **Step 2: Run, verify it fails.**
- [ ] **Step 3: Implement** — add `samples_pushed: u64` to each sensor, `+= 1` at the existing `history.push_back` site (disk.rs:137, network.rs:329). Add `sample_count()` getters. Add `disk_sample_count`/`net_sample_count` to `SystemData` (default 0). Populate them in `state.rs:155` from the sensors. Update every `SystemData { .. }` literal + `Default` (the face test modules each build `SystemData` — add the new fields = 0).
- [ ] **Step 4: Run** `cargo test -p ht32-panel-daemon` → compiles, counter test passes, all existing tests still green (new fields default 0).
- [ ] **Step 5:** clippy + fmt.
- [ ] **Step 6: Commit** — `feat(sensors): track monotonic sample count for wrap-around graphs`.

---

### Task 3: Widget `count` + wrap dispatch in `render_layout` (collapse the Vec↔VecDeque bridge)

**Files:**
- Modify: `crates/ht32-panel-daemon/src/faces/layout.rs` (variant field + render arm).
- Test: same file — a render_layout test that `wrap_around: true` routes to the wrap renderer (assert head-locality on the composed layout) and `false` still matches legacy.

**Interfaces:**
- Consumes: `draw_dual_graph_wrap` (Task 1). Produces: `WidgetContent::DualSparkline { …, wrap_around: bool, count: u64 }` (Task 4 sets these).

- [ ] **Step 1: Write the failing test** — build a `Layout` with one `DualSparkline { wrap_around: true, count: 100, … }`, `render_layout`, assert it equals a direct `draw_dual_graph_wrap` call (same pixels); and a `wrap_around: false` widget equals a direct legacy `draw_dual_graph` call.
- [ ] **Step 2: Run, verify it fails** (no `count` field).
- [ ] **Step 3: Implement** — add `count: u64` to the `DualSparkline` variant. In the `render_layout` arm: pass slices directly (`a.as_slice()`, `b.as_slice()`) — **remove the `VecDeque` rebuild** (layout.rs:181-182). If `wrap_around` → `canvas.draw_dual_graph_wrap(x, y, w, h, a, b, *count, *scale, color_a, color_b, bg)`; else keep legacy `draw_dual_graph` (convert to `VecDeque` only on the legacy branch, or refactor `draw_dual_graph` to accept slices too — preferred, fully removing the bridge). Keep `#[allow(dead_code)]` off now that `wrap_around` is read.
- [ ] **Step 4: Run tests** → PASS.
- [ ] **Step 5:** clippy + fmt.
- [ ] **Step 6: Commit** — `feat(faces): route DualSparkline wrap_around to the oscilloscope renderer`.

---

### Task 4: Flip `professional` to wrap mode + regenerate goldens + verify web

**Files:**
- Modify: `crates/ht32-panel-daemon/src/faces/professional.rs` (4 DualSparkline widgets + its test goldens).

**Interfaces:** Consumes Task 2's `SystemData.{disk,net}_sample_count` and Task 3's widget fields.

- [ ] **Step 1: Set `wrap_around: true`** on all 4 `DualSparkline` widgets in `professional.rs`, and pass the matching `count` (disk graphs → `data.disk_sample_count`, net graphs → `data.net_sample_count`). Ensure the face's `sample()` test data sets deterministic counts (e.g. `disk_sample_count = 100`, `net_sample_count = 100`) and non-trivial `disk_history`/`net_history` so the graph renders.
- [ ] **Step 2: Regenerate the affected golden hashes.** Run the professional golden tests; the graph-bearing configs (full-complication landscape/portrait) will fail with the new hash. Replace each stale golden constant with the new value from the failure output. **Do NOT blindly paste — first eyeball that only the graph-bearing goldens changed** (non-graph configs like ANALOGUE-without-graphs or text-only must be unchanged; if a non-graph golden changed, that's a bug, investigate).
- [ ] **Step 3: Run** `cargo test -p ht32-panel-daemon` → all green; `clippy -D warnings` clean; `cargo fmt --all`.
- [ ] **Step 4: Web/visual verification (manual, report observations).** Build + run the daemon headless (no LCD needed — the web path composes the canvas): `cargo run -p ht32-panel-daemon -- <config>` then GET `/lcd.png` with the professional face selected and some disk/net activity; confirm the graph shows the wrap-around sweep (head gap visible, columns stable between ticks, only the head advancing). Note: this is observational; capture what you see in the report. (If running the daemon is impractical in the sandbox, state that and rely on the pixel determinism/locality tests + goldens as the guarantee.)
- [ ] **Step 5: Commit** — `feat(faces): professional disk/net graphs use wrap-around sweep`.

---

## Sequencing & validation

1. Task 1 (renderer) → 2 (count plumbing) → 3 (widget dispatch) → 4 (flip professional + goldens). Each is independently testable/committable.
2. The determinism + head-locality test (Task 1) is the core correctness guarantee and also the Phase-4 enabler (proves a +1 sample touches ~2–3 columns).
3. After Task 4, the only behavioral change is professional's graph rows; everything else is byte-identical (golden-guarded).

## Notes / deferred

- **Heartbeat-noise throttle** (the WS1 set_time-rejection log spam) is a *separate* daemon-write concern, NOT bundled here — do it as its own small change when desired.
- **Phase 3** (persistent canvas + per-zone scheduler) needs the duplicate-widget-id fix first (digits `"divider"`, clock `"clock_tick"`) and the `id: &'static str` → `Box<str>`/interned decision (see fork-sync-state memory).
- **Phase 4** (partial `0xA2` transport) is where wrap-around pays off — it needs a `Widget.rect` accuracy audit (rects are best-effort today).
- Update this note when adding a line/area style option (the wrap *column* logic is independent of the per-column draw style, so a restyle is local to `draw_dual_graph_wrap`).
