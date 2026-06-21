//! Resolver layer for the WS5 template face.
//!
//! Turns a [`TemplateWidget`] + live [`SystemData`] + active [`Theme`] into one
//! or more [`ResolvedWidget`]s that the template face can pass straight to
//! [`render_layout`] (Task 3 wires this into `TemplateFace::layout`).
//!
//! # Widget expansion
//!
//! Most widgets expand 1-to-1.  Two expand to multiple primitives:
//!
//! - **Gauge** → 2 [`WidgetContent::Arc`]s: a track (full sweep) + a value arc.
//! - **Analog Clock** → 4 primitives via [`mini_analog_clock_draws`]: bezel Arc,
//!   hour Line, minute Line, hub Circle.
//!
//! All other variants expand to exactly 1 widget.

use std::collections::VecDeque;
use std::f32::consts::PI;

use super::spec::{
    Align, ClockMode, ColorRef, DateFmt, HistorySource, NumberBinding, NumberFmt, NumberSource,
    ScaleMode, TemplateContent, TemplateWidget, TextSource, ThemeSlot, TimeFmt,
};
use crate::faces::layout::{Rect, WidgetContent};
use crate::faces::{mini_analog_clock_draws, mini_clock_draw_to_widget, Theme};
use crate::rendering::Canvas;
use crate::sensors::data::SystemData;

// ── Public output types ───────────────────────────────────────────────────────

/// A fully-resolved, render-ready widget produced from a [`TemplateWidget`].
///
/// The `id` is either the template widget's own `id` (for 1-to-1 expansions) or
/// `"{id}__{suffix}"` for multi-primitive expansions (Gauge track/value, Clock
/// bezel/hands/hub).
#[derive(Debug, Clone)]
pub struct ResolvedWidget {
    /// Stable per-widget identifier (mirrors the template's `id` field for
    /// 1-to-1 expansions; suffixed for multi-primitive ones).
    pub id: String,
    /// Bounding box (canvas coordinates).
    pub rect: Rect,
    /// The concrete draw primitive.
    pub content: WidgetContent,
}

// ── Source resolvers ──────────────────────────────────────────────────────────

/// Resolves a [`TextSource`] against live system data into a display string.
///
/// `Time`/`Date` variants use `data.hour`, `data.minute`, `data.day`,
/// `data.month`, `data.year` directly (no system-clock call at render time).
pub fn resolve_text(src: &TextSource, data: &SystemData) -> String {
    match src {
        TextSource::Literal(s) => s.clone(),
        TextSource::Hostname => data.hostname.clone(),
        TextSource::Uptime => data.uptime.clone(),
        TextSource::Ip => data.display_ip.clone().unwrap_or_default(),
        TextSource::NetInterface => data.net_interface.clone(),

        TextSource::Time(fmt) => match fmt {
            TimeFmt::Hhmm => format!("{:02}:{:02}", data.hour, data.minute),
            TimeFmt::Hhmmss => {
                // We have no `second` field on SystemData; render HH:MM:00 as a
                // best-effort until a `second` field is added.
                format!("{:02}:{:02}:00", data.hour, data.minute)
            }
            TimeFmt::Hhmm12h => {
                let (h12, suffix) = if data.hour == 0 {
                    (12u8, "am")
                } else if data.hour < 12 {
                    (data.hour, "am")
                } else if data.hour == 12 {
                    (12u8, "pm")
                } else {
                    (data.hour - 12, "pm")
                };
                format!("{:2}:{:02} {}", h12, data.minute, suffix)
            }
        },

        TextSource::Date(fmt) => {
            let month_abbrev = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let mi = (data.month.saturating_sub(1) as usize).min(11);
            match fmt {
                DateFmt::Iso => {
                    format!("{:04}-{:02}-{:02}", data.year, data.month, data.day)
                }
                DateFmt::Eu => {
                    format!("{:02}/{:02}/{:04}", data.day, data.month, data.year)
                }
                DateFmt::Us => {
                    format!("{:02}/{:02}/{:04}", data.month, data.day, data.year)
                }
                DateFmt::Short => {
                    format!("{} {}", month_abbrev[mi], data.day)
                }
            }
        }

        TextSource::Number(nb) => resolve_number_binding(nb, data),
    }
}

/// Formats a [`NumberBinding`] (source + style) into a display string.
fn resolve_number_binding(nb: &NumberBinding, data: &SystemData) -> String {
    let value = resolve_number(&nb.source, data);
    match nb.style {
        NumberFmt::Percent => format!("{:.0}%", value),
        NumberFmt::Rate => SystemData::format_rate_compact(value),
        NumberFmt::Raw => format!("{:.2}", value),
    }
}

/// Resolves a [`NumberSource`] to a raw `f64` from live system data.
pub fn resolve_number(src: &NumberSource, data: &SystemData) -> f64 {
    match src {
        NumberSource::CpuPercent => data.cpu_percent,
        NumberSource::RamPercent => data.ram_percent,
        NumberSource::CpuTemp => data.cpu_temp.unwrap_or(0.0),
        NumberSource::DiskReadRate => data.disk_read_rate,
        NumberSource::DiskWriteRate => data.disk_write_rate,
        NumberSource::NetRxRate => data.net_rx_rate,
        NumberSource::NetTxRate => data.net_tx_rate,
    }
}

/// Borrows the `VecDeque<f64>` history buffer identified by `src`.
pub fn resolve_history<'d>(src: &HistorySource, data: &'d SystemData) -> &'d VecDeque<f64> {
    match src {
        HistorySource::DiskHistory => &data.disk_history,
        HistorySource::DiskReadHistory => &data.disk_read_history,
        HistorySource::DiskWriteHistory => &data.disk_write_history,
        HistorySource::NetHistory => &data.net_history,
        HistorySource::NetRxHistory => &data.net_rx_history,
        HistorySource::NetTxHistory => &data.net_tx_history,
    }
}

/// Resolves a [`ColorRef`] to an RGB-888 `u32` using the active theme.
pub fn resolve_color(c: &ColorRef, theme: &Theme) -> u32 {
    match c {
        ColorRef::Theme(slot) => match slot {
            ThemeSlot::Primary => theme.primary,
            ThemeSlot::Secondary => theme.secondary,
            ThemeSlot::Text => theme.text,
            ThemeSlot::Background => theme.background,
        },
        ColorRef::Hex(v) => *v,
    }
}

// ── Main resolver ─────────────────────────────────────────────────────────────

/// Resolves a single [`TemplateWidget`] into one or more [`ResolvedWidget`]s.
///
/// The caller (Task 3 `TemplateFace::layout`) iterates the returned `Vec` and
/// pushes each element into the `Layout`.
pub fn resolve(
    w: &TemplateWidget,
    data: &SystemData,
    theme: &Theme,
    canvas: &Canvas,
) -> Vec<ResolvedWidget> {
    match &w.content {
        // ── Text ────────────────────────────────────────────────────────────
        TemplateContent::Text {
            value,
            size,
            color,
            align,
        } => {
            let text = resolve_text(value, data);
            let color_val = resolve_color(color, theme);

            let x = match align {
                Align::Left => w.rect.x,
                Align::Center => {
                    let tw = canvas.text_width(&text, *size);
                    w.rect.x + (w.rect.w as i32 - tw) / 2
                }
                Align::Right => {
                    let tw = canvas.text_width(&text, *size);
                    w.rect.x + w.rect.w as i32 - tw
                }
            };

            vec![ResolvedWidget {
                id: w.id.clone(),
                rect: w.rect,
                content: WidgetContent::Text {
                    text,
                    x,
                    y: w.rect.y,
                    size: *size,
                    color: color_val,
                },
            }]
        }

        // ── Bar ─────────────────────────────────────────────────────────────
        TemplateContent::Bar { value, fill, bg } => {
            let percent = resolve_number(value, data).clamp(0.0, 100.0);
            let fill_val = resolve_color(fill, theme);
            let bg_val = resolve_color(bg, theme);

            vec![ResolvedWidget {
                id: w.id.clone(),
                rect: w.rect,
                content: WidgetContent::Bar {
                    x: w.rect.x,
                    y: w.rect.y,
                    w: w.rect.w,
                    h: w.rect.h,
                    percent,
                    fill: fill_val,
                    bg: bg_val,
                },
            }]
        }

        // ── Gauge ────────────────────────────────────────────────────────────
        // Expands to 2 Arc widgets: "{id}__track" (full sweep) + "{id}__value"
        // (proportional sweep).  Angle convention mirrors ArcsFace::arc_gauge_specs:
        // 135° → 405° (270° sweep, starting bottom-left).
        TemplateContent::Gauge {
            value,
            min,
            max,
            color,
            track,
        } => {
            let val = resolve_number(value, data);
            let range = max - min;
            let normalized = if range > 0.0 {
                ((val - min) / range).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let color_val = resolve_color(color, theme);
            let track_val = resolve_color(track, theme);

            let cx = w.rect.x + w.rect.w as i32 / 2;
            let cy = w.rect.y + w.rect.h as i32 / 2;
            let r = (w.rect.w.min(w.rect.h) / 2).saturating_sub(2);
            let stroke = (r as f32 * 0.15).clamp(2.0, 8.0);

            let start_angle = 135.0_f32 * PI / 180.0;
            let end_angle = 405.0_f32 * PI / 180.0;
            let sweep = end_angle - start_angle;
            let value_end = start_angle + sweep * normalized as f32;

            let gauge_rect = Rect {
                x: cx - r as i32,
                y: cy - r as i32,
                w: r * 2,
                h: r * 2,
            };

            vec![
                ResolvedWidget {
                    id: format!("{}__track", w.id),
                    rect: gauge_rect,
                    content: WidgetContent::Arc {
                        cx,
                        cy,
                        r,
                        start_angle,
                        end_angle,
                        stroke,
                        color: track_val,
                    },
                },
                ResolvedWidget {
                    id: format!("{}__value", w.id),
                    rect: gauge_rect,
                    content: WidgetContent::Arc {
                        cx,
                        cy,
                        r,
                        start_angle,
                        end_angle: value_end,
                        stroke,
                        color: color_val,
                    },
                },
            ]
        }

        // ── Sparkline ───────────────────────────────────────────────────────
        TemplateContent::Sparkline {
            a,
            b,
            wrap_around,
            color_a,
            color_b,
            bg,
            scale,
        } => {
            let history_a = resolve_history(a, data);
            let a_vec: Vec<f64> = history_a.iter().copied().collect();

            let b_vec: Vec<f64> = b
                .as_ref()
                .map(|src| resolve_history(src, data).iter().copied().collect())
                .unwrap_or_default();

            let scale_val = match scale {
                ScaleMode::Auto => SystemData::compute_graph_scale(history_a),
                ScaleMode::Fixed(v) => *v,
            };

            let color_a_val = resolve_color(color_a, theme);
            let color_b_val = resolve_color(color_b, theme);
            let bg_val = resolve_color(bg, theme);

            // Choose the sample counter that matches the primary history source.
            let count = match a {
                HistorySource::DiskHistory
                | HistorySource::DiskReadHistory
                | HistorySource::DiskWriteHistory => data.disk_sample_count,
                HistorySource::NetHistory
                | HistorySource::NetRxHistory
                | HistorySource::NetTxHistory => data.net_sample_count,
            };

            vec![ResolvedWidget {
                id: w.id.clone(),
                rect: w.rect,
                content: WidgetContent::DualSparkline {
                    x: w.rect.x,
                    y: w.rect.y,
                    w: w.rect.w,
                    h: w.rect.h,
                    a: a_vec,
                    b: b_vec,
                    scale: scale_val,
                    color_a: color_a_val,
                    color_b: color_b_val,
                    bg: bg_val,
                    wrap_around: *wrap_around,
                    count,
                },
            }]
        }

        // ── Clock ────────────────────────────────────────────────────────────
        TemplateContent::Clock { mode, color } => {
            let color_val = resolve_color(color, theme);

            match mode {
                ClockMode::Digital => {
                    // Simple centered HH:MM
                    let text = format!("{:02}:{:02}", data.hour, data.minute);
                    // Use a sensible default size that fits most rect heights
                    let size = (w.rect.h as f32 * 0.6).clamp(10.0, 48.0);
                    let tw = canvas.text_width(&text, size);
                    let x = w.rect.x + (w.rect.w as i32 - tw) / 2;
                    let lh = canvas.line_height(size);
                    let y = w.rect.y + (w.rect.h as i32 - lh) / 2;

                    vec![ResolvedWidget {
                        id: w.id.clone(),
                        rect: w.rect,
                        content: WidgetContent::Text {
                            text,
                            x,
                            y,
                            size,
                            color: color_val,
                        },
                    }]
                }

                ClockMode::Analog => {
                    let cx = w.rect.x + w.rect.w as i32 / 2;
                    let cy = w.rect.y + w.rect.h as i32 / 2;
                    let radius = (w.rect.w.min(w.rect.h) / 2).saturating_sub(2);

                    mini_analog_clock_draws(
                        cx,
                        cy,
                        radius,
                        data.hour,
                        data.minute,
                        color_val,
                        color_val,
                    )
                    .into_iter()
                    .enumerate()
                    .map(|(i, draw)| {
                        let (suffix, content) = mini_clock_draw_to_widget(draw, i);
                        ResolvedWidget {
                            id: format!("{}__{}", w.id, suffix),
                            rect: w.rect,
                            content,
                        }
                    })
                    .collect()
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::layout::Rect;
    use crate::faces::Theme;
    use crate::rendering::Canvas;
    use crate::sensors::data::SystemData;

    // ── Helpers ──────────────────────────────────────────────────────────────

    #[allow(clippy::field_reassign_with_default)]
    fn sample_data() -> SystemData {
        let mut d = SystemData::default();
        d.hostname = "testhost".into();
        d.uptime = "1d 2h 3m".into();
        d.display_ip = Some("192.168.1.1".into());
        d.net_interface = "eth0".into();
        d.hour = 14;
        d.minute = 30;
        d.day = 15;
        d.month = 6;
        d.year = 2024;
        d.cpu_percent = 42.0;
        d.ram_percent = 75.0;
        d.cpu_temp = Some(65.0);
        d.disk_read_rate = 1_500_000.0;
        d.disk_write_rate = 500_000.0;
        d.net_rx_rate = 2_000_000.0;
        d.net_tx_rate = 100_000.0;
        d.disk_sample_count = 100;
        d.net_sample_count = 200;
        // Push some history values
        for i in 0..10 {
            let v = (i * 100_000) as f64;
            d.disk_history.push_back(v);
            d.disk_read_history.push_back(v);
            d.disk_write_history.push_back(v / 2.0);
            d.net_history.push_back(v);
            d.net_rx_history.push_back(v);
            d.net_tx_history.push_back(v / 4.0);
        }
        d
    }

    fn default_theme() -> Theme {
        Theme::from_preset("nord")
    }

    fn canvas() -> Canvas {
        Canvas::new(320, 170)
    }

    fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    // ── resolve_text ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_text_literal() {
        let d = sample_data();
        assert_eq!(
            resolve_text(&TextSource::Literal("hello".into()), &d),
            "hello"
        );
    }

    #[test]
    fn resolve_text_hostname() {
        let d = sample_data();
        assert_eq!(resolve_text(&TextSource::Hostname, &d), "testhost");
    }

    #[test]
    fn resolve_text_uptime() {
        let d = sample_data();
        assert_eq!(resolve_text(&TextSource::Uptime, &d), "1d 2h 3m");
    }

    #[test]
    fn resolve_text_ip_present() {
        let d = sample_data();
        assert_eq!(resolve_text(&TextSource::Ip, &d), "192.168.1.1");
    }

    #[test]
    fn resolve_text_ip_absent() {
        let mut d = sample_data();
        d.display_ip = None;
        assert_eq!(resolve_text(&TextSource::Ip, &d), "");
    }

    #[test]
    fn resolve_text_net_interface() {
        let d = sample_data();
        assert_eq!(resolve_text(&TextSource::NetInterface, &d), "eth0");
    }

    #[test]
    fn resolve_text_time_hhmm() {
        let d = sample_data();
        assert_eq!(resolve_text(&TextSource::Time(TimeFmt::Hhmm), &d), "14:30");
    }

    #[test]
    fn resolve_text_time_hhmmss() {
        let d = sample_data();
        let s = resolve_text(&TextSource::Time(TimeFmt::Hhmmss), &d);
        assert!(
            s.starts_with("14:30:"),
            "HH:MM:SS should start with 14:30:, got: {s}"
        );
    }

    #[test]
    fn resolve_text_time_12h_pm() {
        let mut d = sample_data();
        d.hour = 14;
        d.minute = 5;
        let s = resolve_text(&TextSource::Time(TimeFmt::Hhmm12h), &d);
        assert!(s.contains("pm"), "14h should be pm, got: {s}");
        assert!(s.contains("2:05"), "14:05 -> 2:05 pm, got: {s}");
    }

    #[test]
    fn resolve_text_time_12h_midnight() {
        let mut d = sample_data();
        d.hour = 0;
        d.minute = 0;
        let s = resolve_text(&TextSource::Time(TimeFmt::Hhmm12h), &d);
        assert!(s.contains("12"), "midnight is 12am, got: {s}");
        assert!(s.contains("am"), "midnight is am, got: {s}");
    }

    #[test]
    fn resolve_text_time_12h_noon() {
        let mut d = sample_data();
        d.hour = 12;
        d.minute = 0;
        let s = resolve_text(&TextSource::Time(TimeFmt::Hhmm12h), &d);
        assert!(s.contains("12"), "noon is 12pm, got: {s}");
        assert!(s.contains("pm"), "noon is pm, got: {s}");
    }

    #[test]
    fn resolve_text_date_iso() {
        let d = sample_data();
        assert_eq!(
            resolve_text(&TextSource::Date(DateFmt::Iso), &d),
            "2024-06-15"
        );
    }

    #[test]
    fn resolve_text_date_eu() {
        let d = sample_data();
        assert_eq!(
            resolve_text(&TextSource::Date(DateFmt::Eu), &d),
            "15/06/2024"
        );
    }

    #[test]
    fn resolve_text_date_us() {
        let d = sample_data();
        assert_eq!(
            resolve_text(&TextSource::Date(DateFmt::Us), &d),
            "06/15/2024"
        );
    }

    #[test]
    fn resolve_text_date_short() {
        let d = sample_data();
        assert_eq!(
            resolve_text(&TextSource::Date(DateFmt::Short), &d),
            "Jun 15"
        );
    }

    #[test]
    fn resolve_text_number_percent() {
        let d = sample_data();
        let nb = NumberBinding {
            source: NumberSource::CpuPercent,
            style: NumberFmt::Percent,
        };
        assert_eq!(resolve_text(&TextSource::Number(nb), &d), "42%");
    }

    #[test]
    fn resolve_text_number_rate() {
        let d = sample_data();
        let nb = NumberBinding {
            source: NumberSource::DiskReadRate,
            style: NumberFmt::Rate,
        };
        let s = resolve_text(&TextSource::Number(nb), &d);
        // 1_500_000 bytes/s → "1.5M"
        assert_eq!(s, "1.5M");
    }

    #[test]
    fn resolve_text_number_raw() {
        let d = sample_data();
        let nb = NumberBinding {
            source: NumberSource::RamPercent,
            style: NumberFmt::Raw,
        };
        let s = resolve_text(&TextSource::Number(nb), &d);
        // 75.0 → "75.00"
        assert_eq!(s, "75.00");
    }

    // ── resolve_number ───────────────────────────────────────────────────────

    #[test]
    fn resolve_number_all_sources() {
        let d = sample_data();
        assert_eq!(resolve_number(&NumberSource::CpuPercent, &d), 42.0);
        assert_eq!(resolve_number(&NumberSource::RamPercent, &d), 75.0);
        assert_eq!(resolve_number(&NumberSource::CpuTemp, &d), 65.0);
        assert_eq!(resolve_number(&NumberSource::DiskReadRate, &d), 1_500_000.0);
        assert_eq!(resolve_number(&NumberSource::DiskWriteRate, &d), 500_000.0);
        assert_eq!(resolve_number(&NumberSource::NetRxRate, &d), 2_000_000.0);
        assert_eq!(resolve_number(&NumberSource::NetTxRate, &d), 100_000.0);
    }

    #[test]
    fn resolve_number_cpu_temp_absent() {
        let mut d = sample_data();
        d.cpu_temp = None;
        assert_eq!(resolve_number(&NumberSource::CpuTemp, &d), 0.0);
    }

    // ── resolve_history ──────────────────────────────────────────────────────

    #[test]
    fn resolve_history_all_sources() {
        let d = sample_data();
        // Each history has 10 entries pushed in sample_data()
        assert_eq!(resolve_history(&HistorySource::DiskHistory, &d).len(), 10);
        assert_eq!(
            resolve_history(&HistorySource::DiskReadHistory, &d).len(),
            10
        );
        assert_eq!(
            resolve_history(&HistorySource::DiskWriteHistory, &d).len(),
            10
        );
        assert_eq!(resolve_history(&HistorySource::NetHistory, &d).len(), 10);
        assert_eq!(resolve_history(&HistorySource::NetRxHistory, &d).len(), 10);
        assert_eq!(resolve_history(&HistorySource::NetTxHistory, &d).len(), 10);
    }

    #[test]
    fn resolve_history_returns_correct_buffer() {
        let d = sample_data();
        // net_tx_history values are v/4 in sample_data
        let buf = resolve_history(&HistorySource::NetTxHistory, &d);
        // last element: i=9 → v=900_000, /4 = 225_000
        assert!(
            (buf.back().copied().unwrap_or(0.0) - 225_000.0).abs() < 0.1,
            "last net_tx_history entry should be 225_000"
        );
    }

    // ── resolve_color ────────────────────────────────────────────────────────

    #[test]
    fn resolve_color_hex() {
        let theme = default_theme();
        assert_eq!(resolve_color(&ColorRef::Hex(0xFF00FF), &theme), 0xFF00FF);
    }

    #[test]
    fn resolve_color_theme_slots() {
        let theme = default_theme();
        assert_eq!(
            resolve_color(&ColorRef::Theme(ThemeSlot::Primary), &theme),
            theme.primary
        );
        assert_eq!(
            resolve_color(&ColorRef::Theme(ThemeSlot::Secondary), &theme),
            theme.secondary
        );
        assert_eq!(
            resolve_color(&ColorRef::Theme(ThemeSlot::Text), &theme),
            theme.text
        );
        assert_eq!(
            resolve_color(&ColorRef::Theme(ThemeSlot::Background), &theme),
            theme.background
        );
    }

    // ── resolve() — per-variant ───────────────────────────────────────────────

    fn make_widget(id: &str, r: Rect, content: TemplateContent) -> TemplateWidget {
        TemplateWidget {
            id: id.to_string(),
            rect: r,
            content,
        }
    }

    #[test]
    fn resolve_text_widget_left_align() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "lbl",
            rect(10, 20, 200, 20),
            TemplateContent::Text {
                value: TextSource::Literal("hi".into()),
                size: 12.0,
                color: ColorRef::Theme(ThemeSlot::Text),
                align: Align::Left,
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        assert_eq!(resolved.len(), 1, "Text should resolve to 1 widget");
        assert_eq!(resolved[0].id, "lbl");
        match &resolved[0].content {
            WidgetContent::Text { text, x, y, .. } => {
                assert_eq!(text, "hi");
                assert_eq!(*x, 10, "left-align x should equal rect.x");
                assert_eq!(*y, 20);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn resolve_text_widget_center_align_offsets_x() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "clbl",
            rect(0, 0, 200, 20),
            TemplateContent::Text {
                value: TextSource::Literal("hi".into()),
                size: 12.0,
                color: ColorRef::Theme(ThemeSlot::Text),
                align: Align::Center,
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        let tw = c.text_width("hi", 12.0);
        let expected_x = (200 - tw) / 2;
        match &resolved[0].content {
            WidgetContent::Text { x, .. } => {
                assert_eq!(*x, expected_x, "center-align x should be (rect.w - tw) / 2");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn resolve_text_widget_right_align() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let text = "hi";
        let w = make_widget(
            "rlbl",
            rect(0, 0, 200, 20),
            TemplateContent::Text {
                value: TextSource::Literal(text.into()),
                size: 12.0,
                color: ColorRef::Theme(ThemeSlot::Text),
                align: Align::Right,
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        let tw = c.text_width(text, 12.0);
        let expected_x = 200 - tw;
        match &resolved[0].content {
            WidgetContent::Text { x, .. } => {
                assert_eq!(*x, expected_x, "right-align x should be rect.w - tw");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn resolve_bar_widget() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "cpu_bar",
            rect(0, 0, 320, 8),
            TemplateContent::Bar {
                value: NumberSource::CpuPercent,
                fill: ColorRef::Theme(ThemeSlot::Primary),
                bg: ColorRef::Hex(0x202020),
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        assert_eq!(resolved.len(), 1, "Bar should resolve to 1 widget");
        assert_eq!(resolved[0].id, "cpu_bar");
        match &resolved[0].content {
            WidgetContent::Bar {
                percent, fill, bg, ..
            } => {
                assert!(
                    (*percent - 42.0).abs() < 0.01,
                    "bar percent should be cpu_percent"
                );
                assert_eq!(*fill, theme.primary);
                assert_eq!(*bg, 0x202020);
            }
            other => panic!("expected Bar, got {other:?}"),
        }
    }

    #[test]
    fn resolve_bar_clamps_to_100() {
        let mut d = sample_data();
        d.cpu_percent = 150.0;
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "bar",
            rect(0, 0, 100, 8),
            TemplateContent::Bar {
                value: NumberSource::CpuPercent,
                fill: ColorRef::Hex(0xFF0000),
                bg: ColorRef::Hex(0x000000),
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        match &resolved[0].content {
            WidgetContent::Bar { percent, .. } => {
                assert!(
                    *percent <= 100.0,
                    "bar percent must be clamped to 100, got {percent}"
                );
            }
            other => panic!("expected Bar, got {other:?}"),
        }
    }

    #[test]
    fn resolve_gauge_expands_to_two_arcs() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "gauge",
            rect(10, 10, 60, 60),
            TemplateContent::Gauge {
                value: NumberSource::CpuPercent, // 42%
                min: 0.0,
                max: 100.0,
                color: ColorRef::Theme(ThemeSlot::Primary),
                track: ColorRef::Theme(ThemeSlot::Background),
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        assert_eq!(resolved.len(), 2, "Gauge should resolve to 2 Arc widgets");
        assert_eq!(resolved[0].id, "gauge__track");
        assert_eq!(resolved[1].id, "gauge__value");

        // Track arc spans full sweep (135° → 405°)
        match &resolved[0].content {
            WidgetContent::Arc {
                start_angle,
                end_angle,
                color,
                ..
            } => {
                let sa = 135.0_f32 * PI / 180.0;
                let ea = 405.0_f32 * PI / 180.0;
                assert!(
                    (start_angle - sa).abs() < 0.001,
                    "track start_angle mismatch"
                );
                assert!((end_angle - ea).abs() < 0.001, "track end_angle mismatch");
                assert_eq!(
                    *color, theme.background,
                    "track should use background color"
                );
            }
            other => panic!("expected Arc for track, got {other:?}"),
        }

        // Value arc spans proportional to 42% of sweep
        match &resolved[1].content {
            WidgetContent::Arc {
                start_angle,
                end_angle,
                color,
                ..
            } => {
                let sa = 135.0_f32 * PI / 180.0;
                let ea = 405.0_f32 * PI / 180.0;
                let sweep = ea - sa;
                let expected_end = sa + sweep * 0.42;
                assert!(
                    (start_angle - sa).abs() < 0.001,
                    "value start_angle mismatch"
                );
                assert!(
                    (end_angle - expected_end).abs() < 0.01,
                    "value end_angle mismatch: expected {expected_end}, got {end_angle}"
                );
                assert_eq!(*color, theme.primary, "value should use primary color");
            }
            other => panic!("expected Arc for value, got {other:?}"),
        }
    }

    #[test]
    fn resolve_sparkline_widget() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "spark",
            rect(0, 80, 160, 40),
            TemplateContent::Sparkline {
                a: HistorySource::NetRxHistory,
                b: Some(HistorySource::NetTxHistory),
                wrap_around: true,
                color_a: ColorRef::Theme(ThemeSlot::Primary),
                color_b: ColorRef::Theme(ThemeSlot::Secondary),
                bg: ColorRef::Hex(0x000000),
                scale: ScaleMode::Auto,
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        assert_eq!(resolved.len(), 1, "Sparkline should resolve to 1 widget");
        assert_eq!(resolved[0].id, "spark");
        match &resolved[0].content {
            WidgetContent::DualSparkline {
                a,
                b,
                wrap_around,
                count,
                color_a,
                color_b,
                bg,
                ..
            } => {
                assert_eq!(a.len(), 10, "a history should have 10 entries");
                assert_eq!(b.len(), 10, "b history should have 10 entries");
                assert!(wrap_around, "wrap_around should be true");
                assert_eq!(
                    *count, 200,
                    "net_sample_count should be used for net source"
                );
                assert_eq!(*color_a, theme.primary);
                assert_eq!(*color_b, theme.secondary);
                assert_eq!(*bg, 0x000000);
            }
            other => panic!("expected DualSparkline, got {other:?}"),
        }
    }

    #[test]
    fn resolve_sparkline_no_b() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "spark2",
            rect(0, 80, 160, 40),
            TemplateContent::Sparkline {
                a: HistorySource::DiskHistory,
                b: None,
                wrap_around: false,
                color_a: ColorRef::Theme(ThemeSlot::Primary),
                color_b: ColorRef::Theme(ThemeSlot::Secondary),
                bg: ColorRef::Hex(0x000000),
                scale: ScaleMode::Fixed(10_000_000.0),
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        match &resolved[0].content {
            WidgetContent::DualSparkline {
                b, scale, count, ..
            } => {
                assert!(b.is_empty(), "b should be empty when None");
                assert!(
                    (*scale - 10_000_000.0).abs() < 0.1,
                    "fixed scale should be 10_000_000"
                );
                assert_eq!(*count, 100, "disk_sample_count used for disk source");
            }
            other => panic!("expected DualSparkline, got {other:?}"),
        }
    }

    #[test]
    fn resolve_clock_digital() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "clock",
            rect(200, 0, 80, 30),
            TemplateContent::Clock {
                mode: ClockMode::Digital,
                color: ColorRef::Theme(ThemeSlot::Text),
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        assert_eq!(
            resolved.len(),
            1,
            "Digital clock should resolve to 1 widget"
        );
        assert_eq!(resolved[0].id, "clock");
        match &resolved[0].content {
            WidgetContent::Text { text, .. } => {
                assert_eq!(text, "14:30", "digital clock should show HH:MM");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn resolve_clock_analog_expands_to_four_widgets() {
        let d = sample_data();
        let theme = default_theme();
        let c = canvas();
        let w = make_widget(
            "aclock",
            rect(200, 10, 80, 80),
            TemplateContent::Clock {
                mode: ClockMode::Analog,
                color: ColorRef::Theme(ThemeSlot::Primary),
            },
        );
        let resolved = resolve(&w, &d, &theme, &c);
        // mini_analog_clock_draws returns 4 primitives: bezel, hour, minute, hub
        assert_eq!(resolved.len(), 4, "Analog clock should expand to 4 widgets");
        // IDs are "{widget_id}__{suffix}"
        assert!(
            resolved[0].id.starts_with("aclock__"),
            "first widget id should be aclock__<suffix>"
        );
        assert!(
            resolved[1].id.starts_with("aclock__"),
            "second widget id should be aclock__<suffix>"
        );
        assert!(
            resolved[2].id.starts_with("aclock__"),
            "third widget id should be aclock__<suffix>"
        );
        assert!(
            resolved[3].id.starts_with("aclock__"),
            "fourth widget id should be aclock__<suffix>"
        );
        // Widget types: Arc, Line, Line, Circle
        assert!(
            matches!(resolved[0].content, WidgetContent::Arc { .. }),
            "first primitive should be Arc (bezel)"
        );
        assert!(
            matches!(resolved[1].content, WidgetContent::Line { .. }),
            "second primitive should be Line (hour hand)"
        );
        assert!(
            matches!(resolved[2].content, WidgetContent::Line { .. }),
            "third primitive should be Line (minute hand)"
        );
        assert!(
            matches!(resolved[3].content, WidgetContent::Circle { .. }),
            "fourth primitive should be Circle (hub)"
        );
    }
}
