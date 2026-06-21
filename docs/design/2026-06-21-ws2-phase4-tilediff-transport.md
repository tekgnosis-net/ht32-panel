# WS2 Phase 4 — Tile-diff partial USB transport (the payoff)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Cut the LCD USB duty from ~70% (full ~110 KB redraw every frame) to ~1–2% by transmitting **only the pixels that changed** — a face-agnostic **tile-diff**: keep the previously-transmitted framebuffer, diff it against the new one in tiles, and send only changed tiles via the existing `0xA2` partial-write. This eliminates the endpoint saturation that causes the chronic NAK / `USB HID error` stream on pve3, at the source. (Phase 3's per-widget scheduler was dropped as redundant — tile-diff gates transmission face-agnostically and the daemon is CPU-idle.)

**Architecture:** The diff lives in the **HW device** (`ht32-panel-hw`), which already owns the orientation rotation and the wire protocol. A new `redraw_diff(framebuffer, force_full)` replicates `redraw()`'s "framebuffer → transmitted bytes" transform, stores the previous transmitted bytes, diffs in tiles, and sends each changed tile via a **no-re-rotate** partial write (`build_refresh_packet`). The daemon calls `redraw_diff` instead of `redraw`, passing `force_full = true` on structural changes (first frame, face/theme/orientation change, reconnect). The **canvas always holds the full composite** (web `/lcd.png` unaffected — partial logic gates *only* USB).

**Tech Stack:** Rust; `ht32-panel-hw` (`lcd/device.rs`, `lcd/protocol.rs`); the `Framebuffer` (320×170 RGB565, `data: Vec<u16>`); `0xA2` partial (`build_refresh_packet`, ≤2048 px / ≤255 per dim).

## Global Constraints

- **Equivalence to `redraw()` is the safety property.** `redraw_diff(fb, force_full=true)` must place the **same pixels on screen** as `redraw(fb)` (full repaint). An **unchanged** frame must send **zero** partial packets. A frame that changed region R must send only the tiles overlapping R. These are the binding tests — they make the rotation/coordinate handling correct by construction (don't re-derive the rotation math; match `redraw`'s transmitted bytes exactly).
- **Diff in transmitted (post-everything) space.** Keep the previous *transmitted* `Vec<u16>` (the exact bytes `redraw` would send after its rotation), diff against the new transmitted bytes. The partial write must NOT re-apply rotation (the existing `refresh()` rotates per-tile, which is positionally wrong for 180° — do not reuse it; add a raw, no-rotation partial).
- **0xA2 limits:** each packet ≤ 2048 pixels and width ≤ 255, height ≤ 255 (both `u8`). Tiles must respect this; a tile that would exceed it is sub-tiled.
- **Full-redraw triggers (force_full):** first frame after (re)connect, face change, theme change, orientation change, explicit `force_redraw()`. On force_full: send a full `redraw` (27 chunks) and set prev = transmitted.
- **Web invariant untouched:** the daemon's canvas/framebuffer composition is unchanged; only the *send* call changes. `/lcd.png` must stay complete and current.
- TDD; `cargo fmt --all` + `clippy --all-targets -D warnings` clean per commit.

## Key facts (verified)

- `Framebuffer`: `data: Vec<u16>`, 320×170 RGB565 (`PIXEL_COUNT = 54400`).
- `device.redraw(fb)` (device.rs:142): copies `fb.data()`, `rotate_180` if `orientation.needs_rotation()`, sends `CHUNK_COUNT=27` chunks (`0xA3`).
- `device.refresh(x,y,w:u8,h:u8,&[u16])` (device.rs:174): rotates the tile's pixels per-orientation then `build_refresh_packet` — **positionally wrong for 180° partials; do not reuse for tile-diff.**
- `build_refresh_packet(x:u16,y:u16,w:u8,h:u8,&[u16])` (protocol.rs:87): `0xA2`; pos little-endian, pixels big-endian. One packet ≤2048 px.
- `render_to_framebuffer` (state.rs:746) already applies orientation into `render.framebuffer`; `render_frame` (state.rs:673) then calls `device.redraw(&render.framebuffer)`.

## File Structure

- `crates/ht32-panel-hw/src/lcd/device.rs` — add the transmitted-bytes transform helper, `refresh_raw` (no-rotation partial + sub-tiling), prev-framebuffer storage, and `redraw_diff(framebuffer, force_full)` (Tasks 1–2).
- `crates/ht32-panel-hw/src/lcd/protocol.rs` — reuse `build_refresh_packet`; add a tile-sub-tiling helper if needed (Task 1).
- `crates/ht32-panel-daemon/src/state.rs` — `render_frame` calls `redraw_diff` with a `force_full` flag; track structural-change triggers (Task 3).

---

### Task 1: No-rotation raw partial write + rect→packet tiling

**Files:** `crates/ht32-panel-hw/src/lcd/device.rs` (+ `protocol.rs` if a helper fits there). Test: in-crate unit tests.

**Interfaces:**
- Produces: `fn refresh_raw(&self, x: u16, y: u16, width: u16, height: u16, pixels: &[u16]) -> Result<()>` — sends a rectangle of *already-final* (post-rotation, device-space) pixels, applying NO rotation, **sub-tiling** into ≤2048 px / ≤255-per-dim `0xA2` packets via `build_refresh_packet`. `pixels` is row-major `width*height`.

- [ ] **Step 1: failing test** — `refresh_raw` of a 304×4 region (1216 px, width>255) splits into the right number of `0xA2` packets with correct sub-rect (x,y,w,h) and pixel slices. Assert against the bytes `build_refresh_packet` produces for each sub-tile (capture `device.write` via a mock/recording transport, or test the pure sub-tiling helper that returns `Vec<(x,y,w,h,Vec<u16>)>`). Prefer a **pure helper** `fn tile_rect(x,y,w,h,&[u16]) -> Vec<SubTile>` tested directly (boundary cases: width=255 exact, width=256 → 2 tiles, 2048-px exact, the 304-px graph row → N tiles).
- [ ] **Step 2: run, verify fail.**
- [ ] **Step 3: implement** the pure `tile_rect` sub-tiling (split width into ≤255 chunks AND so each sub-tile ≤2048 px; extract the sub-rect's pixel slice from the row-major `pixels`), and `refresh_raw` looping `build_refresh_packet` + `device.write` over sub-tiles. NO rotation anywhere.
- [ ] **Step 4: run → pass.** clippy + fmt.
- [ ] **Step 5: commit** — `feat(hw): raw no-rotation partial write with 0xA2 sub-tiling`.

---

### Task 2: `redraw_diff` — tile-diff against the previous transmitted frame

**Files:** `crates/ht32-panel-hw/src/lcd/device.rs`. Test: in-crate unit tests.

**Interfaces:**
- Produces: `fn redraw_diff(&self, framebuffer: &Framebuffer, force_full: bool) -> Result<usize>` — returns the number of tiles sent (0 if unchanged). Consumes Task 1's `refresh_raw`.
- Internal: prev transmitted bytes stored as `Mutex<Option<Vec<u16>>>` on the device (interior mutability, like `current_orientation`).

**Semantics:**
- Compute `transmitted: Vec<u16>` EXACTLY as `redraw` would send it: copy `framebuffer.data()`, then apply the SAME rotation `redraw` applies (`rotate_180` iff `needs_rotation()`). (Factor `redraw`'s transform into a shared `fn transmitted_bytes(&self, fb) -> Vec<u16>` used by both `redraw` and `redraw_diff` so they cannot drift.)
- If `force_full` OR prev is `None` OR prev length ≠ transmitted length (orientation/size change): do a **full `redraw`-equivalent send** (the 27 `0xA3` chunks), set prev = transmitted, return (a sentinel, e.g. `CHUNK_COUNT` or `usize::MAX`).
- Else: walk a fixed **tile grid** over the 320×170 transmitted image (tile size const, e.g. `TILE_W=80, TILE_H=16` → ≤2048 px, ≤255 dim; last row/col clipped). For each tile, compare the prev vs new pixel sub-rects; if any pixel differs, `refresh_raw(tile.x, tile.y, tile.w, tile.h, &tile_pixels)`. Count changed tiles. Set prev = transmitted. Return count.
- The grid coordinates are in **transmitted/device space** (post-rotation), matching `refresh_raw` (no rotation) — so a changed tile lands exactly where `redraw` would have put those pixels.

- [ ] **Step 1: failing tests** (the binding equivalence properties):
  1. `redraw_diff(fb, force_full=true)` then reading the recorded `device.write` packets reconstructs the SAME on-screen pixels as `redraw(fb)` (compare the union of sent regions == full transmitted frame).
  2. After a `force_full`, `redraw_diff(fb, false)` with the SAME `fb` sends **0** tiles (`returns 0`, zero `0xA2` packets).
  3. After a `force_full`, mutate a single 10×10 region of `fb`, `redraw_diff(fb, false)` sends only the tiles overlapping that region (assert tile count == expected overlap, and the sent pixels match the region).
  4. Orientation-change path: prev length mismatch → full redraw, prev reset.
  (Use a recording transport: inject a `Box<dyn Transport>`/test double that captures every `write(&[u8])`, or test a pure `fn diff_tiles(prev, new, grid) -> Vec<Tile>` directly and assert tiles; prefer the pure `diff_tiles` for the diff logic + one integration test for the send path.)
- [ ] **Step 2: run, verify fail.**
- [ ] **Step 3: implement** `transmitted_bytes` (shared with `redraw`), the `Mutex<Option<Vec<u16>>>` prev, the tile-grid `diff_tiles`, and `redraw_diff`.
- [ ] **Step 4: run → pass.** clippy + fmt.
- [ ] **Step 5: commit** — `feat(hw): redraw_diff tile-diff partial transport (only changed tiles over 0xA2)`.

---

### Task 3: Wire `redraw_diff` into the daemon render loop

**Files:** `crates/ht32-panel-daemon/src/state.rs`. Test: existing daemon tests stay green; add a force_full-trigger unit test if practical.

- [ ] **Step 1:** In `render_frame` (state.rs:~700), replace `device.redraw(&render.framebuffer)` with `device.redraw_diff(&render.framebuffer, force_full)`. Determine `force_full`:
  - Track a `needs_full_redraw: AtomicBool` (or a field) set `true` on: first frame, face change (`set_face`), theme change (`set_theme`), orientation change (`set_orientation`), and after a reconnect (the reopen path). Read-and-clear it each `render_frame` to compute `force_full`.
  - The reconnect path (`on_write_failure` → reopen → redraw) must set `force_full=true` (new device handle → its prev is None anyway, but be explicit) so the first post-reconnect send is full.
- [ ] **Step 2:** Confirm the success/failure health accounting (`on_write_success`/`on_write_failure`) still wraps the send (a `redraw_diff` error is a write failure → demote/reconnect as today).
- [ ] **Step 3:** `cargo test -p ht32-panel-daemon` green; `cargo build --workspace`; clippy `-D warnings`; fmt.
- [ ] **Step 4: commit** — `feat(daemon): use tile-diff redraw_diff; full redraw only on structural change`.

---

### Task 4: Deploy to pve3 + verify the USB-duty collapse (manual)

- [ ] Build on pve3 (`cargo build --release -p ht32-panel-daemon`), restart `ht32paneld`.
- [ ] **Verify:** `/lcd.png` still correct + current (web invariant). Journal: the `Render error: USB HID error` stream collapses (count per minute drops to ~0). `ht32panelctl`-driven face/theme/orientation change triggers a clean full redraw then resumes partials. Confirm CPU stays low.
- [ ] Report observations; the chronic NAK stream collapsing is the success signal (design goal 1).

---

## Sequencing & validation

1. Task 1 (raw partial + tiling, pure-tested) → Task 2 (diff + equivalence-tested) → Task 3 (daemon wiring) → Task 4 (deploy).
2. The equivalence + zero-on-unchanged + only-changed-tiles tests (Task 2) are the correctness backbone — they make the rotation handling correct without deriving it.
3. The 320×170 frame at TILE 80×16 = 4×11 grid (44 tiles); a wrap-around graph tick dirties ~1–2 tiles → ~2 KB transmitted vs ~110 KB — the ~50× reduction.

## Notes / deferred

- Heartbeat-noise throttle (set_time rejections every ~10s) is still separate; can be folded in later (per-write-type backoff).
- Tile size (`TILE_W`/`TILE_H`) starts as a const; expose via config only if tuning is needed on hardware.
- Phase 3 (per-widget scheduler) intentionally skipped (YAGNI given tile-diff); revisit only if the template-builder needs per-widget structure/cadence.
