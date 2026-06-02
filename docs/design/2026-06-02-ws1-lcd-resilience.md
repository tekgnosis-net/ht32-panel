# WS1 — LCD Resilience (reconnect + log throttle)

Status: DRAFT for review · 2026-06-02 · fork: tekgnosis-net/ht32-panel

## Context

On the Proxmox host `pve3`, the panel reverts to its built-in default screen and never
recovers until the service/host is restarted. Investigation (read-only, live device) found:

- The display interface (USB IF 1, `EP 0x02 OUT`, interrupt, 64 B @ 1 ms) is driven over the
  **libusb backend** (`hidapi` feature `linux-static-libusb`). This is **required**, not
  incidental: IF 1 is output-only, so the kernel `usbhid` driver refuses it and creates **no
  `/dev/hidrawN`** (verified: `hidraw0`→IF0, `hidraw1`→IF2, IF1 driver = `usbfs`). The hidraw
  backend therefore cannot open the display. **We keep libusb.**
- When a write starts failing (transient endpoint NAK, or a real re-enumeration/brown-out), the
  daemon's `render_frame`/`send_heartbeat` propagate the error but **never demote the device
  handle**. `try_lcd_reconnect()` only runs when `*lcd` is `None`, and nothing ever sets it to
  `None` after startup → the stale handle is retried forever, the process never exits, and
  systemd's `Restart=` never fires. A transient glitch becomes a permanent blackout.
- The error-log throttle is broken: it resets `consecutive_errors = 0` after logging, so the
  `consecutive_errors == 1` branch is true on the next tick → it logs **every** error
  (observed: journal flooded; ~28 % of writes fail transiently even on a healthy device).

## Goals

1. A transient USB failure self-heals: detect a real disconnect, reopen the device in place, and
   restore the panel without human intervention.
2. If in-place recovery cannot succeed, exit cleanly so systemd relaunches a fresh process.
3. Stop the log flooding; surface disconnect/reconnect as a few clear lines.

## Non-goals (handled in WS2)

- No rendering changes: still full `redraw()` every frame, no partial updates, no zones, no
  wrap-around graphs. WS1 is purely the resilience/logging layer and ships independently.
- No backend change (libusb stays, see above).

## Design

### Connection state machine

```
            N consecutive failures              reopen ok
 Connected ───────────────────────► Disconnected ─────────► Connected
     ▲                                   │
     │                                   │ no successful write for `exit_after`
     └────────── (process relaunch) ◄──── process::exit(1)  ──► systemd restarts
```

- **Connected → Disconnected:** a shared consecutive-failure counter (written by *both* the
  heartbeat and render paths) crosses `failure_threshold`. On crossing we **drop the
  `LcdDevice`** (set `*lcd = None`) — dropping the handle releases the stale libusb/usbfs
  interface claim — log one `WARN` ("LCD disconnected after N failures; reconnecting"), and reset
  the counter.
- **Disconnected → Connected:** the existing `try_lcd_reconnect()` (reopen via fresh
  `LcdDevice::open()` → re-enumerate + re-claim IF1 → re-init orientation + heartbeat) succeeds.
  Log one `INFO` ("LCD reconnected").
- **Disconnected → exit:** if no successful write occurs for `exit_after`, call
  `process::exit(1)`; systemd (`Restart=on-failure`/`always`, `RestartSec=5`) relaunches a fully
  fresh process. This is the last resort when in-place reopen keeps failing (e.g. wedged libusb
  state). Restart cost is acceptable — display state is persisted and re-derived on boot.

### Failure detection

- A single source of truth in `AppState` (e.g. `lcd_failures: AtomicU32` + a
  `last_success: Instant`), updated by a small `record_write(result)` helper called from both
  `send_heartbeat` and `render_frame` after each device write.
  - success → reset counter to 0, stamp `last_success`.
  - failure → increment; if `>= failure_threshold` → demote to `None` (state transition above).
- **Threshold rationale:** transient NAKs are normal (reference calls them ignorable; ~28 %
  measured, clustered with clean ~5 s gaps). A real disconnect produces an unbroken run of
  failures. `failure_threshold` must sit above transient noise. Proposed default **10**
  (≈7–10 s of unbroken failure given 1 s heartbeat + 2.5 s render), configurable.

### Reconnect tuning

- Keep `try_lcd_reconnect()` but make the retry interval configurable; **reduce default 30 s →
  5 s** so a dark panel recovers quickly. Still rate-limited so a truly-absent device doesn't spin
  `HidApi::new()` (full USB enumeration) every frame.
- **Move the 1 s post-open cooldown off the async runtime.** `LcdDevice::open()` does a blocking
  `thread::sleep(1s)`; during reconnect this runs inside the tokio render task and stalls other
  tasks. Wrap reconnect opens in `spawn_blocking` (or make the cooldown caller-supplied).

### Device-open correctness

`LcdDevice::open()` currently does
`find(interface == 1).or_else(|| devices.first())`. The fallback opens the **wrong** interface
when IF1 is absent — the first `04D9:FD01` HID interface is IF0, the consumer-control *input*
device (`hidraw0`), which cannot accept display writes. Verified live: only IF1 is the display
(output-only, libusb/usbfs); IF0→hidraw0, IF2→hidraw1.

- **Remove the `.or_else(|| devices.first())` fallback;** require `interface_number() == 1`,
  return `LcdNotFound` otherwise. Fix the stale "interface 2" comment.
- **Optional:** cache the resolved device path and reopen via `open_path` so reconnects don't
  re-run `HidApi::new()` (a full system-wide HID enumeration) every 5 s.
- The kernel warning `usbhid 1-8:1.1: couldn't find an input interrupt endpoint` is a benign
  enumeration artifact (kernel probing the output-only IF1), **not** daemon-caused and out of scope.

### Log throttle fix

- Reset `consecutive_errors` **only on success**, never after logging.
- Gate logging purely on time: log the first error of a failure streak immediately, then at most
  once per `log_interval` (default 60 s) with the accumulated count. On recovery, log one line.

### Config additions (`config.rs`)

| key | default | meaning |
|-----|---------|---------|
| `lcd_failure_threshold` | 10 | consecutive write failures before declaring disconnect |
| `lcd_reconnect_interval_ms` | 5000 | min interval between reopen attempts |
| `lcd_exit_after_ms` | 300000 (5 min); 0 = disabled | dark-time before exit-to-systemd escalation |
| `lcd_error_log_interval_ms` | 60000 | throttled error-log cadence |

## Files to change

- `crates/ht32-panel-daemon/src/state.rs` — failure tracking + `record_write`, demote-to-`None`,
  reconnect interval/cooldown handling, exit-escalation check.
- `crates/ht32-panel-daemon/src/main.rs` — throttle fix in `render_loop`/`heartbeat_loop` (or move
  logging into state helpers).
- `crates/ht32-panel-daemon/src/config.rs` — the four config keys above.
- `crates/ht32-panel-hw/src/lcd/device.rs` — `open()` correctness: require interface 1, drop the
  `devices.first()` fallback, fix the stale comment; optional `open_path`-on-reconnect.
- Tests (below).

## Testing strategy (TDD)

- **Extract the resilience logic into a pure, hardware-free unit** — a small `LcdHealth`
  struct/state machine holding the counter, thresholds, timers, and transitions
  (Connected/Disconnected/ShouldExit/ShouldLog). This is fully unit-testable without USB.
  - tests: transient burst below threshold does **not** demote; an unbroken run `>= threshold`
    demotes; a success resets the counter; `exit_after` elapsed with no success → `ShouldExit`;
    throttle yields log-now on first error then suppresses until `log_interval`.
- **Thin hardware glue** (`record_write`, demote, reopen, exit) stays minimal and is exercised
  manually on `pve3`:
  - after deploy: confirm the journal is no longer flooded (throttled).
  - induce a drop (power-cycle the device / re-enumeration): confirm
    `disconnected → reconnecting → reconnected` appears and the panel returns on its own.
  - confirm `NRestarts` increments **only** when the exit-escalation path triggers.

## Open choices for review

1. `failure_threshold` default (10?) and `reconnect_interval` default (5 s?).
2. Include the **exit-to-systemd escalation**, or rely on in-place reopen only? (You said
   restart-loss is fine, so escalation is low-risk and adds a strong safety net.)
3. `exit_after` default (5 min?) and whether it should be disabled by default.
```
