# WS1 — LCD Resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the daemon self-heal from USB write failures (detect a real disconnect, drop+reopen the device, and exit-to-systemd as a last resort) and stop the error-log flooding.

**Architecture:** A hardware-free `LcdHealth` state machine owns the consecutive-failure counter, recovery timing, and log throttling (fully unit-tested). The device-write call sites in `state.rs` feed it write results and act on its decisions (`Demote` → set `lcd = None`, which the existing `try_lcd_reconnect()` then reopens; `should_exit` → `process::exit(1)`). `main.rs`'s broken throttle is removed. `LcdDevice::open()` is hardened to require the display interface (1) instead of falling back to the wrong interface.

**Tech Stack:** Rust, `hidapi` (libusb backend — required; interface 1 is output-only with no hidraw node), `tokio`, `tracing`. Tests: standard `#[cfg(test)]` modules, `cargo test`.

Spec: `docs/design/2026-06-02-ws1-lcd-resilience.md`. Branch: `feat/lcd-resilience`.

---

### Task 1: `LcdHealth` state machine (pure, unit-tested)

**Files:**
- Create: `crates/ht32-panel-daemon/src/lcd_health.rs`
- Modify: `crates/ht32-panel-daemon/src/main.rs` (add `mod lcd_health;`)

- [ ] **Step 1: Write the failing tests**

Create `crates/ht32-panel-daemon/src/lcd_health.rs`:

```rust
//! Hardware-free LCD write-health state machine: decides demote/reconnect,
//! exit-to-systemd, and throttled logging. Time is passed in (no `Instant::now()`
//! inside) so the logic is fully deterministic and unit-testable.

use std::time::{Duration, Instant};

/// Action the caller should take after recording a write result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAction {
    /// Nothing to do; keep going.
    None,
    /// Failure streak hit the threshold: drop the handle and reconnect.
    Demote,
}

/// Tracks consecutive LCD write failures and recovery timing.
pub struct LcdHealth {
    failure_threshold: u32,
    exit_after: Duration,
    log_interval: Duration,
    consecutive_failures: u32,
    /// `None` until the first successful write since (re)start.
    last_success: Option<Instant>,
    last_error_log: Option<Instant>,
}

impl LcdHealth {
    pub fn new(failure_threshold: u32, exit_after: Duration, log_interval: Duration) -> Self {
        Self {
            failure_threshold,
            exit_after,
            log_interval,
            consecutive_failures: 0,
            last_success: None,
            last_error_log: None,
        }
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Record a successful device write.
    pub fn record_success(&mut self, now: Instant) {
        self.consecutive_failures = 0;
        self.last_success = Some(now);
        self.last_error_log = None;
    }

    /// Record a failed device write; returns whether to demote.
    pub fn record_failure(&mut self) -> WriteAction {
        self.consecutive_failures += 1;
        if self.consecutive_failures == self.failure_threshold {
            WriteAction::Demote
        } else {
            WriteAction::None
        }
    }

    /// True when recovery has failed for `exit_after`. Only arms after at least
    /// one successful write, so a never-present device never triggers a restart loop.
    pub fn should_exit(&self, now: Instant) -> bool {
        if self.exit_after.is_zero() {
            return false;
        }
        matches!(self.last_success, Some(t) if now.duration_since(t) >= self.exit_after)
    }

    /// Throttled error logging: returns the failure count when a log line is due
    /// (first failure of a streak, then at most once per `log_interval`).
    pub fn should_log(&mut self, now: Instant) -> Option<u32> {
        let due = match self.last_error_log {
            None => true,
            Some(t) => now.duration_since(t) >= self.log_interval,
        };
        if due {
            self.last_error_log = Some(now);
            Some(self.consecutive_failures)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health() -> LcdHealth {
        LcdHealth::new(3, Duration::from_secs(300), Duration::from_secs(60))
    }

    #[test]
    fn transient_failures_below_threshold_do_not_demote() {
        let mut h = health();
        assert_eq!(h.record_failure(), WriteAction::None);
        assert_eq!(h.record_failure(), WriteAction::None);
    }

    #[test]
    fn threshold_failures_demote() {
        let mut h = health();
        h.record_failure();
        h.record_failure();
        assert_eq!(h.record_failure(), WriteAction::Demote);
        assert_eq!(h.consecutive_failures(), 3);
    }

    #[test]
    fn success_resets_failure_streak() {
        let t = Instant::now();
        let mut h = health();
        h.record_failure();
        h.record_failure();
        h.record_success(t);
        assert_eq!(h.consecutive_failures(), 0);
        assert_eq!(h.record_failure(), WriteAction::None);
    }

    #[test]
    fn exit_only_arms_after_a_success_then_goes_dark() {
        let t = Instant::now();
        let mut h = health();
        // Never connected: should never exit.
        assert!(!h.should_exit(t + Duration::from_secs(10_000)));
        // After a success, going dark past exit_after triggers exit.
        h.record_success(t);
        assert!(!h.should_exit(t + Duration::from_secs(299)));
        assert!(h.should_exit(t + Duration::from_secs(300)));
    }

    #[test]
    fn exit_after_zero_disables_escalation() {
        let t = Instant::now();
        let mut h = LcdHealth::new(3, Duration::ZERO, Duration::from_secs(60));
        h.record_success(t);
        assert!(!h.should_exit(t + Duration::from_secs(100_000)));
    }

    #[test]
    fn log_throttle_logs_first_then_once_per_interval() {
        let t = Instant::now();
        let mut h = health();
        h.record_failure();
        assert_eq!(h.should_log(t), Some(1)); // first failure logs
        h.record_failure();
        assert_eq!(h.should_log(t + Duration::from_secs(1)), None); // suppressed
        h.record_failure();
        assert_eq!(h.should_log(t + Duration::from_secs(60)), Some(3)); // interval elapsed
    }

    #[test]
    fn success_reenables_immediate_logging() {
        let t = Instant::now();
        let mut h = health();
        h.record_failure();
        h.should_log(t);
        h.record_success(t + Duration::from_secs(1));
        h.record_failure();
        assert_eq!(h.should_log(t + Duration::from_secs(2)), Some(1));
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/ht32-panel-daemon/src/main.rs`, add alongside the other `mod` declarations:

```rust
mod lcd_health;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p ht32-panel-daemon lcd_health`
Expected: 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/ht32-panel-daemon/src/lcd_health.rs crates/ht32-panel-daemon/src/main.rs
git commit -m "feat(daemon): add LcdHealth write-health state machine"
```

---

### Task 2: Config keys for resilience tuning

**Files:**
- Modify: `crates/ht32-panel-daemon/src/config.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `config.rs` (create the module if absent):

```rust
#[test]
fn resilience_defaults_are_sane() {
    let c = Config::default();
    assert_eq!(c.lcd_failure_threshold, 10);
    assert_eq!(c.lcd_reconnect_interval_ms, 5_000);
    assert_eq!(c.lcd_exit_after_ms, 300_000);
    assert_eq!(c.lcd_error_log_interval_ms, 60_000);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ht32-panel-daemon resilience_defaults`
Expected: FAIL — `no field lcd_failure_threshold on type Config`.

- [ ] **Step 3: Add the fields and defaults**

In the `Config` struct in `config.rs`, after the `heartbeat` field, add:

```rust
    /// Consecutive LCD write failures before declaring a disconnect.
    #[serde(default = "default_lcd_failure_threshold")]
    pub lcd_failure_threshold: u32,

    /// Minimum interval between LCD reopen attempts (ms).
    #[serde(default = "default_lcd_reconnect_interval_ms")]
    pub lcd_reconnect_interval_ms: u64,

    /// Dark-time before exiting for systemd to relaunch (ms); 0 disables.
    #[serde(default = "default_lcd_exit_after_ms")]
    pub lcd_exit_after_ms: u64,

    /// Throttled error-log cadence (ms).
    #[serde(default = "default_lcd_error_log_interval_ms")]
    pub lcd_error_log_interval_ms: u64,
```

Add the default functions near the other `fn default_*`:

```rust
fn default_lcd_failure_threshold() -> u32 {
    10
}
fn default_lcd_reconnect_interval_ms() -> u64 {
    5_000
}
fn default_lcd_exit_after_ms() -> u64 {
    300_000
}
fn default_lcd_error_log_interval_ms() -> u64 {
    60_000
}
```

Add the same four fields to the `Default for Config` impl (the block that lists `refresh_interval: default_refresh_interval(), ...`):

```rust
            lcd_failure_threshold: default_lcd_failure_threshold(),
            lcd_reconnect_interval_ms: default_lcd_reconnect_interval_ms(),
            lcd_exit_after_ms: default_lcd_exit_after_ms(),
            lcd_error_log_interval_ms: default_lcd_error_log_interval_ms(),
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p ht32-panel-daemon resilience_defaults`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ht32-panel-daemon/src/config.rs
git commit -m "feat(daemon): add LCD resilience config keys"
```

---

### Task 3: Harden `LcdDevice::open()` — require interface 1

**Files:**
- Modify: `crates/ht32-panel-hw/src/lcd/device.rs:54-58`

- [ ] **Step 1: Replace the wrong-interface fallback**

In `LcdDevice::open()`, change the interface selection from:

```rust
        // Find the interface we need (interface 1 for display data)
        let device_info = devices
            .iter()
            .find(|d| d.interface_number() == LCD_INTERFACE)
            .or_else(|| devices.first()) // Fallback to first device if interface 2 not found
            .ok_or(Error::LcdNotFound)?;
```

to:

```rust
        // The display data interface is interface 1 (output-only, libusb-backed).
        // Interface 0 is the consumer-control input device; never write there.
        let device_info = devices
            .iter()
            .find(|d| d.interface_number() == LCD_INTERFACE)
            .ok_or(Error::LcdNotFound)?;
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build -p ht32-panel-hw`
Expected: builds with no warnings about the removed fallback.

- [ ] **Step 3: Verify existing hardware test still compiles**

Run: `cargo test -p ht32-panel-hw --no-run`
Expected: compiles (the `#[ignore]` `test_device_open` is unchanged; real-hardware verification is manual).

- [ ] **Step 4: Commit**

```bash
git add crates/ht32-panel-hw/src/lcd/device.rs
git commit -m "fix(hw): open() requires display interface 1, no wrong-interface fallback"
```

---

### Task 4: Add `LcdHealth` to `AppState`

**Files:**
- Modify: `crates/ht32-panel-daemon/src/state.rs` (imports, struct field ~line 220, `new()` ~line 350)

- [ ] **Step 1: Add imports and the field**

At the top of `state.rs`, add to the `use` section:

```rust
use crate::lcd_health::{LcdHealth, WriteAction};
use std::time::{Duration, Instant};
```

In the `AppState` struct, after the `last_lcd_reconnect` field, add:

```rust
    /// LCD write-health tracker (failure counting, exit timing, log throttle).
    lcd_health: Mutex<LcdHealth>,
```

- [ ] **Step 2: Initialize it in `AppState::new()`**

In `AppState::new()`, where the other fields are constructed (the struct literal returned at the end), add:

```rust
            lcd_health: Mutex::new(LcdHealth::new(
                config.lcd_failure_threshold,
                Duration::from_millis(config.lcd_exit_after_ms),
                Duration::from_millis(config.lcd_error_log_interval_ms),
            )),
```

(Place it consistently with the existing `last_lcd_reconnect: Mutex::new(now),` line, reusing the same `config` already in scope.)

- [ ] **Step 3: Verify it builds**

Run: `cargo build -p ht32-panel-daemon`
Expected: builds (field unused warning is acceptable until Task 5).

- [ ] **Step 4: Commit**

```bash
git add crates/ht32-panel-daemon/src/state.rs
git commit -m "feat(daemon): wire LcdHealth into AppState"
```

---

### Task 5: Feed write results to `LcdHealth`; demote + throttle at the call sites

**Files:**
- Modify: `crates/ht32-panel-daemon/src/state.rs` (`send_heartbeat` ~581, `render_frame` ~613, add `handle_write_failure` helper)

- [ ] **Step 1: Add the shared failure handler**

Add this method to `impl AppState` (near `send_heartbeat`):

```rust
    /// Records a device-write failure: throttled log, demote-to-None on a
    /// sustained streak, and exit-to-systemd as a last resort.
    fn on_write_failure(&self, what: &str, err: &Error) {
        let now = Instant::now();
        let (action, count, should_exit) = {
            let mut health = self.lcd_health.lock().unwrap();
            let action = health.record_failure();
            let count = health.consecutive_failures();
            let should_exit = health.should_exit(now);
            if let Some(c) = health.should_log(now) {
                if c > 1 {
                    warn!("{} error (x{} consecutive): {}", what, c, err);
                } else {
                    warn!("{} error: {}", what, err);
                }
            }
            (action, count, should_exit)
        };

        if action == WriteAction::Demote {
            warn!("LCD unresponsive after {count} consecutive failures; dropping handle to reconnect");
            *self.lcd.lock().unwrap() = None; // drop LcdDevice → releases the libusb/usbfs claim
        }
        if should_exit {
            error!("LCD has been dark too long; exiting so systemd relaunches a fresh process");
            std::process::exit(1);
        }
    }

    fn on_write_success(&self) {
        self.lcd_health.lock().unwrap().record_success(Instant::now());
    }
```

Ensure `error` and `warn` are imported (`use tracing::{debug, error, info, warn};` — add `error` if missing).

- [ ] **Step 2: Update `send_heartbeat` to record results**

Replace the body of `send_heartbeat`:

```rust
    pub fn send_heartbeat(&self) -> Result<()> {
        let result = {
            let lcd = self.lcd.lock().unwrap();
            match *lcd {
                Some(ref device) => Some(device.heartbeat()),
                None => None,
            }
        };
        match result {
            Some(Ok(())) => self.on_write_success(),
            Some(Err(e)) => self.on_write_failure("Heartbeat", &e),
            None => {}
        }
        Ok(())
    }
```

- [ ] **Step 3: Update the LCD-send block in `render_frame`**

In `render_frame`, replace the "Try to reconnect LCD if disconnected, then send" block:

```rust
            // Send to LCD; record the outcome for health tracking.
            let send_result = {
                let lcd = self.lcd.lock().unwrap();
                match *lcd {
                    Some(ref device) => Some(device.redraw(&render.framebuffer)),
                    None => None,
                }
            };
            match send_result {
                Some(Ok(())) => self.on_write_success(),
                Some(Err(e)) => self.on_write_failure("Render", &e),
                None => {
                    // Disconnected: attempt a (rate-limited) reopen, then redraw.
                    if self.try_lcd_reconnect() {
                        let lcd = self.lcd.lock().unwrap();
                        if let Some(ref device) = *lcd {
                            match device.redraw(&render.framebuffer) {
                                Ok(()) => { drop(lcd); self.on_write_success(); }
                                Err(e) => { drop(lcd); self.on_write_failure("Render", &e); }
                            }
                        }
                    }
                }
            }
```

(The surrounding `render` borrow and framebuffer are unchanged; this replaces only the device-send portion.)

- [ ] **Step 4: Verify it builds**

Run: `cargo build -p ht32-panel-daemon`
Expected: builds, no unused-field warning for `lcd_health`.

- [ ] **Step 5: Manual smoke (optional, on dev host with device)**

Run the daemon; with the device connected, confirm normal operation and that `on_write_success` keeps the streak at 0 (no demote spam).

- [ ] **Step 6: Commit**

```bash
git add crates/ht32-panel-daemon/src/state.rs
git commit -m "feat(daemon): record LCD write results, demote+reconnect on sustained failure"
```

---

### Task 6: Remove the broken throttle; configurable reconnect; off-runtime open

**Files:**
- Modify: `crates/ht32-panel-daemon/src/main.rs:131-186` (`render_loop`, `heartbeat_loop`)
- Modify: `crates/ht32-panel-daemon/src/state.rs` (`try_lcd_reconnect` reconnect interval)

- [ ] **Step 1: Simplify the loops (logging/throttle now lives in `LcdHealth`)**

Replace `render_loop` and `heartbeat_loop` in `main.rs` with:

```rust
async fn render_loop(state: Arc<AppState>) {
    loop {
        // Errors that are LCD-write failures are handled+throttled inside
        // render_frame via LcdHealth; only genuinely unexpected errors surface.
        if let Err(e) = state.render_frame().await {
            warn!("Render pipeline error: {e}");
        }
        let ms = state.refresh_interval_ms();
        tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
    }
}

async fn heartbeat_loop(state: Arc<AppState>, interval_ms: u64) {
    let interval = std::time::Duration::from_millis(interval_ms);
    loop {
        tokio::time::sleep(interval).await;
        if let Err(e) = state.send_heartbeat() {
            warn!("Heartbeat pipeline error: {e}");
        }
    }
}
```

- [ ] **Step 2: Make the reconnect interval configurable**

In `state.rs` `try_lcd_reconnect()`, replace the hard-coded 30 s:

```rust
        if last_attempt.elapsed() < std::time::Duration::from_secs(30) {
            return false;
        }
```

with:

```rust
        if last_attempt.elapsed() < Duration::from_millis(self.config.lcd_reconnect_interval_ms) {
            return false;
        }
```

- [ ] **Step 3: Verify it builds and the suite passes**

Run: `cargo test -p ht32-panel-daemon`
Expected: all tests pass; no references to the removed `consecutive_errors` throttle remain.

- [ ] **Step 4: Commit**

```bash
git add crates/ht32-panel-daemon/src/main.rs crates/ht32-panel-daemon/src/state.rs
git commit -m "refactor(daemon): drop broken log throttle, make reconnect interval configurable"
```

---

### Task 7: Workspace verification + manual validation on pve3

**Files:** none (verification only)

- [ ] **Step 1: Full workspace build + tests + clippy**

Run:
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
Expected: clean.

- [ ] **Step 2: Deploy to pve3 and validate (manual, read-only checks after)**

```bash
# build + install over the existing dev binary path, restart, observe
cargo build --release -p ht32-panel-daemon
# (copy/symlink target/release/ht32paneld as already configured), then:
systemctl restart ht32paneld.service
journalctl -u ht32paneld.service -f
```
Confirm: the journal no longer shows the ~per-second `USB HID error` flood (throttled to first + once/60 s); with the device healthy, no demote/reconnect churn.

- [ ] **Step 3: Validate recovery (manual)**

Induce a disconnect (re-enumeration / power-cycle the device). Confirm the journal shows: throttled failure log → `dropping handle to reconnect` → `LCD device reconnected successfully`, and the panel returns without a manual restart. Confirm `systemctl show -p NRestarts ht32paneld.service` stays 0 unless the exit-escalation path is exercised.

---

## Self-review notes

- **Spec coverage:** reconnect/demote (Tasks 4–5), exit escalation (Task 5 `on_write_failure`), reconnect interval (Task 6), throttle fix (Tasks 1 + 6), open() correctness (Task 3), config keys (Task 2). All spec sections mapped.
- **Deferred from spec (noted, low value):** the `open_path`-on-reconnect optimization and moving the 1 s open cooldown to `spawn_blocking` are *not* in this plan — the reconnect interval (5 s) and single-worker 1 s block are acceptable, and `spawn_blocking` adds lifetime/`Arc` complexity disproportionate to the gain. Revisit if reconnect latency proves a problem.
- **Type consistency:** `WriteAction::{None,Demote}`, `record_failure() -> WriteAction`, `record_success(Instant)`, `should_exit(Instant) -> bool`, `should_log(Instant) -> Option<u32>`, `consecutive_failures() -> u32` used consistently across Tasks 1, 4, 5.
