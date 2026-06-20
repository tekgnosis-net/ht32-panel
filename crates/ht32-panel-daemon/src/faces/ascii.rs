//! ASCII text-only face with ASCII art graphs.
//!
//! Portrait layout (135x240):
//! ```text
//! endeavour         18:45
//! Up: 5d 12h 34m    2025-01-31
//! IP:
//! 192.168.1.100
//! Temp:                  45°C
//! CPU: 45%
//! [########...............]
//! RAM: 67%
//! [##########..............]
//! DSK:             R:12M W:5M
//! [_._.-=+*##*+=-._.____..]
//! NET:           D:1.2M U:0.8M
//! [__..--==++**##**++==..]
//! ```
//!
//! Landscape layout (320x170):
//! ```text
//! endeavour               18:45
//! Up: 5d 12h 34m
//! IP: 192.168.1.100
//! CPU [########........] 45%
//! RAM [##########......] 67%
//! DSK  R:12M W:5M
//! [_._.-=+*##*+=-._.____..]
//! NET  D:1.2M U:0.8M
//! [__..--==++**##**++==..]
//! ```

use super::{
    complication_names, complication_options, complications, date_formats, draw_mini_analog_clock,
    mini_analog_clock_draws, time_formats, Complication, EnabledComplications, Face, MiniClockDraw,
    Theme,
};
use crate::faces::layout::{Cadence, Layout, Rect, Widget, WidgetContent, ZoneKind};
use crate::rendering::Canvas;
use crate::sensors::data::SystemData;

/// Dim a color by mixing it toward the background.
fn dim_color(color: u32, background: u32, factor: f32) -> u32 {
    let r1 = ((color >> 16) & 0xFF) as f32;
    let g1 = ((color >> 8) & 0xFF) as f32;
    let b1 = (color & 0xFF) as f32;
    let r2 = ((background >> 16) & 0xFF) as f32;
    let g2 = ((background >> 8) & 0xFF) as f32;
    let b2 = (background & 0xFF) as f32;

    let r = (r1 * factor + r2 * (1.0 - factor)) as u32;
    let g = (g1 * factor + g2 * (1.0 - factor)) as u32;
    let b = (b1 * factor + b2 * (1.0 - factor)) as u32;

    (r << 16) | (g << 8) | b
}

/// Derive colors from theme for the ASCII face.
struct FaceColors {
    /// Primary highlight color (hostname, interface name)
    highlight: u32,
    /// Main text color
    text: u32,
    /// Dimmed text color (uptime, IPs)
    dim: u32,
    /// Graph background
    bar_bg: u32,
    /// Disk graph fill color
    bar_disk: u32,
    /// Network graph fill color
    bar_net: u32,
}

impl FaceColors {
    fn from_theme(theme: &Theme) -> Self {
        Self {
            highlight: theme.primary,
            text: theme.text,
            dim: dim_color(theme.text, theme.background, 0.7), // Higher for better contrast
            bar_bg: dim_color(theme.primary, theme.background, 0.2),
            bar_disk: dim_color(theme.primary, theme.secondary, 0.5),
            bar_net: theme.secondary,
        }
    }
}

/// Font sizes.
const FONT_LARGE: f32 = 16.0;
const FONT_NORMAL: f32 = 14.0;
const FONT_SMALL: f32 = 12.0;

/// Creates an ASCII progress bar string.
/// Returns something like "[########........]"
fn ascii_bar(percent: f64, width: usize) -> String {
    let filled = ((percent / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    format!("[{}{}]", "#".repeat(filled), ".".repeat(empty))
}

/// Creates an ASCII sparkline from historical data.
/// Uses ASCII characters to represent different heights:
/// `_` (lowest), `.`, `-`, `=`, `+`, `*`, `#` (highest)
fn ascii_sparkline(data: &std::collections::VecDeque<f64>, max_value: f64, width: usize) -> String {
    const CHARS: [char; 7] = ['_', '.', '-', '=', '+', '*', '#'];

    if data.is_empty() || max_value <= 0.0 {
        return "_".repeat(width);
    }

    // Sample data to fit width
    let num_points = data.len();
    let mut result = String::with_capacity(width);

    for i in 0..width {
        // Map output position to data index
        let data_idx = if width <= num_points {
            // More data than width: sample from recent data
            num_points - width + i
        } else {
            // Less data than width: stretch or pad
            (i * num_points) / width
        };

        let value = data.get(data_idx).copied().unwrap_or(0.0);
        let normalized = (value / max_value).clamp(0.0, 1.0);
        let level = (normalized * (CHARS.len() - 1) as f64).round() as usize;
        result.push(CHARS[level.min(CHARS.len() - 1)]);
    }

    result
}

/// A text-only ASCII face.
pub struct AsciiFace;

impl AsciiFace {
    /// Creates a new ASCII face.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsciiFace {
    fn default() -> Self {
        Self::new()
    }
}

impl Face for AsciiFace {
    fn name(&self) -> &str {
        "ascii"
    }

    fn available_complications(&self) -> Vec<Complication> {
        vec![
            complications::time(true),
            complications::date(true, date_formats::ISO),
            complications::ip_address(true),
            complications::network(true),
            complications::disk_io(true),
            complications::cpu_temp(true),
        ]
    }

    fn render(
        &self,
        canvas: &mut Canvas,
        data: &SystemData,
        theme: &Theme,
        complications: &EnabledComplications,
    ) {
        let colors = FaceColors::from_theme(theme);
        let (width, _height) = canvas.dimensions();
        let portrait = width < 200;
        let margin = 6;
        let mut y = 4; // Start near top
        let bar_chars = if portrait { 10 } else { 16 };

        let is_enabled = |id: &str| complications.is_enabled(self.name(), id, true);

        // Get time format option
        let time_format = complications
            .get_option(
                self.name(),
                complication_names::TIME,
                complication_options::TIME_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(time_formats::DIGITAL_24H);

        // Get date format option
        let date_format = complications
            .get_option(
                self.name(),
                complication_names::DATE,
                complication_options::DATE_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(date_formats::ISO);

        if portrait {
            // Portrait layout - labels on separate lines, wider graphs
            let line_height = canvas.line_height(FONT_SMALL);
            let section_spacing = 6; // Extra spacing between label/value pairs
                                     // Calculate bar width to fill most of the line (leave margin on each side).
                                     // Subtract 2 for the bracket chars '[' and ']' that wrap the bar content.
            let bar_width = ((width as i32 - margin * 2) / 7 - 2).max(8) as usize; // ~7 pixels per char

            // Hostname (always shown)
            canvas.draw_text(margin, y, &data.hostname, FONT_LARGE, colors.highlight);

            // Complication: Time (right-aligned)
            if is_enabled(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock on the right
                    let clock_radius = 10_u32;
                    let clock_cx = width as i32 - margin - clock_radius as i32;
                    let clock_cy = y + clock_radius as i32;
                    draw_mini_analog_clock(
                        canvas,
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.highlight,
                        colors.text,
                    );
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_LARGE);
                    canvas.draw_text(
                        width as i32 - margin - time_width,
                        y,
                        &time_str,
                        FONT_LARGE,
                        colors.text,
                    );
                }
            }
            y += canvas.line_height(FONT_LARGE) + 1;

            // Complication: Date (right-aligned)
            if is_enabled(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_SMALL);
                    canvas.draw_text(
                        width as i32 - margin - date_width,
                        y,
                        &date_str,
                        FONT_SMALL,
                        colors.dim,
                    );
                }
            }
            y += line_height; // Skip line for date
            y += line_height; // Extra line before Up

            // Up: on its own line (two lines below date)
            let uptime_text = format!("Up: {}", data.uptime);
            canvas.draw_text(margin, y, &uptime_text, FONT_SMALL, colors.dim);
            y += line_height + section_spacing;

            // IP: on its own line
            if is_enabled(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    canvas.draw_text(margin, y, "IP:", FONT_SMALL, colors.dim);
                    y += line_height;
                    // IP value on next line, possibly split for IPv6
                    let max_width = width as i32 - margin * 2;
                    let ip_width = canvas.text_width(ip, FONT_SMALL);
                    if ip_width > max_width && ip.contains(':') {
                        let mid = ip.len() / 2;
                        let split_pos = ip[..mid].rfind(':').map(|p| p + 1).unwrap_or(mid);
                        let (first, second) = ip.split_at(split_pos);
                        canvas.draw_text(margin, y, first, FONT_SMALL, colors.text);
                        y += line_height;
                        canvas.draw_text(margin, y, second, FONT_SMALL, colors.text);
                    } else {
                        canvas.draw_text(margin, y, ip, FONT_SMALL, colors.text);
                    }
                    y += line_height + section_spacing * 2;
                }
            }

            // Temp: on its own line
            if is_enabled(complication_names::CPU_TEMP) {
                if let Some(temp) = data.cpu_temp {
                    canvas.draw_text(margin, y, "Temp:", FONT_SMALL, colors.dim);
                    let temp_val = format!("{:.0}°C", temp);
                    let temp_w = canvas.text_width(&temp_val, FONT_SMALL);
                    canvas.draw_text(
                        width as i32 - margin - temp_w,
                        y,
                        &temp_val,
                        FONT_SMALL,
                        colors.text,
                    );
                    y += line_height + section_spacing;
                }
            }

            // CPU: label line, then bar on next line
            let cpu_label = format!("CPU: {:2.0}%", data.cpu_percent);
            canvas.draw_text(margin, y, &cpu_label, FONT_SMALL, colors.dim);
            y += line_height;
            let cpu_bar = ascii_bar(data.cpu_percent, bar_width);
            canvas.draw_text(margin, y, &cpu_bar, FONT_SMALL, colors.text);
            y += line_height + section_spacing;

            // RAM: label line, then bar on next line
            let ram_label = format!("RAM: {:2.0}%", data.ram_percent);
            canvas.draw_text(margin, y, &ram_label, FONT_SMALL, colors.dim);
            y += line_height;
            let ram_bar = ascii_bar(data.ram_percent, bar_width);
            canvas.draw_text(margin, y, &ram_bar, FONT_SMALL, colors.text);
            y += line_height + section_spacing;

            // DSK: label line, then sparkline on next line
            if is_enabled(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                canvas.draw_text(margin, y, "DSK:", FONT_SMALL, colors.dim);
                let disk_rates = format!("R:{} W:{}", disk_r, disk_w);
                let disk_rates_w = canvas.text_width(&disk_rates, FONT_SMALL);
                canvas.draw_text(
                    width as i32 - margin - disk_rates_w,
                    y,
                    &disk_rates,
                    FONT_SMALL,
                    colors.text,
                );
                y += line_height;
                let sparkline = ascii_sparkline(
                    &data.disk_history,
                    SystemData::compute_graph_scale(&data.disk_history),
                    bar_width,
                );
                canvas.draw_text(
                    margin,
                    y,
                    &format!("[{}]", sparkline),
                    FONT_SMALL,
                    colors.bar_disk,
                );
                y += line_height + section_spacing;
            }

            // NET: label line, then sparkline on next line
            if is_enabled(complication_names::NETWORK) {
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                canvas.draw_text(margin, y, "NET:", FONT_SMALL, colors.dim);
                let net_rates = format!("D:{} U:{}", net_rx, net_tx);
                let net_rates_w = canvas.text_width(&net_rates, FONT_SMALL);
                canvas.draw_text(
                    width as i32 - margin - net_rates_w,
                    y,
                    &net_rates,
                    FONT_SMALL,
                    colors.text,
                );
                y += line_height;
                let sparkline = ascii_sparkline(
                    &data.net_history,
                    SystemData::compute_graph_scale(&data.net_history),
                    bar_width,
                );
                canvas.draw_text(
                    margin,
                    y,
                    &format!("[{}]", sparkline),
                    FONT_SMALL,
                    colors.bar_net,
                );
            }
        } else {
            // Landscape layout
            // Hostname (always shown)
            canvas.draw_text(margin, y, &data.hostname, FONT_LARGE, colors.highlight);

            // Complication: Time (right-aligned)
            if is_enabled(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock on the right
                    let clock_radius = 10_u32;
                    let clock_cx = width as i32 - margin - clock_radius as i32;
                    let clock_cy = y + clock_radius as i32;
                    draw_mini_analog_clock(
                        canvas,
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.highlight,
                        colors.text,
                    );
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_LARGE);
                    canvas.draw_text(
                        width as i32 - margin - time_width,
                        y,
                        &time_str,
                        FONT_LARGE,
                        colors.text,
                    );
                }
            }
            y += canvas.line_height(FONT_LARGE) + 1;

            // Complication: Date (right-aligned, under time)
            if is_enabled(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_NORMAL);
                    canvas.draw_text(
                        width as i32 - margin - date_width,
                        y,
                        &date_str,
                        FONT_NORMAL,
                        colors.dim,
                    );
                }
            }

            // Base element: Uptime (always shown, same line as date on left)
            let uptime_text = format!("Up: {}", data.uptime);
            canvas.draw_text(margin, y, &uptime_text, FONT_NORMAL, colors.dim);
            y += canvas.line_height(FONT_NORMAL) + 1;

            // Complication: IP address
            if is_enabled(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    canvas.draw_text(margin, y, &format!("IP: {}", ip), FONT_SMALL, colors.dim);
                    y += canvas.line_height(FONT_SMALL) + 4;
                } else {
                    y += 4;
                }
            }

            // Base element: CPU bar with optional temperature (always shown)
            let cpu_bar = ascii_bar(data.cpu_percent, bar_chars);
            let cpu_text = if is_enabled(complication_names::CPU_TEMP) {
                if let Some(temp) = data.cpu_temp {
                    format!("CPU {} {:3.0}%  {:.0}°C", cpu_bar, data.cpu_percent, temp)
                } else {
                    format!("CPU {} {:3.0}%", cpu_bar, data.cpu_percent)
                }
            } else {
                format!("CPU {} {:3.0}%", cpu_bar, data.cpu_percent)
            };
            canvas.draw_text(margin, y, &cpu_text, FONT_NORMAL, colors.text);
            y += canvas.line_height(FONT_NORMAL) + 1;

            // Base element: RAM bar (always shown)
            let ram_bar = ascii_bar(data.ram_percent, bar_chars);
            let ram_text = format!("RAM {} {:3.0}%", ram_bar, data.ram_percent);
            canvas.draw_text(margin, y, &ram_text, FONT_NORMAL, colors.text);
            y += canvas.line_height(FONT_NORMAL) + 2;

            // Complication: Disk I/O
            if is_enabled(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                canvas.draw_text(margin, y, "DSK", FONT_NORMAL, colors.text);
                canvas.draw_text(
                    margin + 40,
                    y,
                    &format!("R:{} W:{}", disk_r, disk_w),
                    FONT_NORMAL,
                    colors.dim,
                );
                y += canvas.line_height(FONT_NORMAL);
                let sparkline = ascii_sparkline(
                    &data.disk_history,
                    SystemData::compute_graph_scale(&data.disk_history),
                    bar_chars + 20,
                );
                canvas.draw_text(
                    margin,
                    y,
                    &format!("[{}]", sparkline),
                    FONT_NORMAL,
                    colors.bar_disk,
                );
                y += canvas.line_height(FONT_NORMAL) + 2;
            }

            // Complication: Network
            if is_enabled(complication_names::NETWORK) {
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                canvas.draw_text(margin, y, "NET", FONT_NORMAL, colors.text);
                canvas.draw_text(
                    margin + 40,
                    y,
                    &format!("D:{} U:{}", net_rx, net_tx),
                    FONT_NORMAL,
                    colors.dim,
                );
                y += canvas.line_height(FONT_NORMAL);
                let sparkline = ascii_sparkline(
                    &data.net_history,
                    SystemData::compute_graph_scale(&data.net_history),
                    bar_chars + 20,
                );
                canvas.draw_text(
                    margin,
                    y,
                    &format!("[{}]", sparkline),
                    FONT_NORMAL,
                    colors.bar_net,
                );
            }
        }
        let _ = y;
    }

    fn layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        complications: &EnabledComplications,
    ) -> Option<Layout> {
        self.build_layout(canvas, data, theme, complications)
    }
}

/// Maps a single [`MiniClockDraw`] spec to a stable widget id and a
/// [`WidgetContent`] variant for use in `build_layout`.
///
/// The `index` parameter is the 0-based position in the slice returned by
/// `mini_analog_clock_draws`; it is used to produce a stable static id string.
fn mini_clock_draw_to_widget(draw: MiniClockDraw, index: usize) -> (&'static str, WidgetContent) {
    // The draw order from mini_analog_clock_draws is fixed:
    //   0 → Arc  (bezel)
    //   1 → Line (hour hand)
    //   2 → Line (minute hand)
    //   3 → Circle (hub)
    match (index, draw) {
        (
            _,
            MiniClockDraw::Arc {
                cx,
                cy,
                r,
                start_angle,
                end_angle,
                stroke,
                color,
            },
        ) => (
            "clock_bezel",
            WidgetContent::Arc {
                cx,
                cy,
                r,
                start_angle,
                end_angle,
                stroke,
                color,
            },
        ),
        (
            1,
            MiniClockDraw::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                color,
            },
        ) => (
            "clock_hour",
            WidgetContent::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                color,
            },
        ),
        (
            _,
            MiniClockDraw::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                color,
            },
        ) => (
            "clock_minute",
            WidgetContent::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                color,
            },
        ),
        (_, MiniClockDraw::Circle { cx, cy, r, color }) => {
            ("clock_hub", WidgetContent::Circle { cx, cy, r, color })
        }
    }
}

impl AsciiFace {
    /// Builds the typed-widget layout, covering ALL configs including ANALOGUE
    /// time (which emits `Line`/`Arc`/`Circle` widgets from `mini_analog_clock_draws`).
    fn build_layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        complications: &EnabledComplications,
    ) -> Option<Layout> {
        let colors = FaceColors::from_theme(theme);
        let (width, _height) = canvas.dimensions();
        let portrait = width < 200;
        let margin = 6;
        let mut y = 4; // Start near top
        let bar_chars = if portrait { 10 } else { 16 };
        let mut layout = Layout::new();

        let is_enabled = |id: &str| complications.is_enabled(self.name(), id, true);

        // Get time format option
        let time_format = complications
            .get_option(
                self.name(),
                complication_names::TIME,
                complication_options::TIME_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(time_formats::DIGITAL_24H);

        // Get date format option
        let date_format = complications
            .get_option(
                self.name(),
                complication_names::DATE,
                complication_options::DATE_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(date_formats::ISO);

        if portrait {
            // Portrait layout - labels on separate lines, wider graphs
            let line_height = canvas.line_height(FONT_SMALL);
            let section_spacing = 6;
            // Subtract 2 for the bracket chars '[' and ']' that wrap the bar content.
            let bar_width = ((width as i32 - margin * 2) / 7 - 2).max(8) as usize;

            // Hostname (always shown)
            layout.push(Widget {
                id: "hostname",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&data.hostname, FONT_LARGE).max(0) as u32,
                    h: canvas.line_height(FONT_LARGE).max(0) as u32,
                },
                kind: ZoneKind::Static,
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: data.hostname.clone(),
                    x: margin,
                    y,
                    size: FONT_LARGE,
                    color: colors.highlight,
                },
            });

            // Complication: Time (right-aligned)
            if is_enabled(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    let clock_radius = 10_u32;
                    let clock_cx = width as i32 - margin - clock_radius as i32;
                    let clock_cy = y + clock_radius as i32;
                    for (i, draw) in mini_analog_clock_draws(
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.highlight,
                        colors.text,
                    )
                    .into_iter()
                    .enumerate()
                    {
                        let (id, content) = mini_clock_draw_to_widget(draw, i);
                        layout.push(Widget {
                            id,
                            rect: Rect {
                                x: clock_cx - clock_radius as i32,
                                y: clock_cy - clock_radius as i32,
                                w: clock_radius * 2,
                                h: clock_radius * 2,
                            },
                            kind: ZoneKind::Dynamic,
                            cadence: Cadence::Seconds(60),
                            content,
                        });
                    }
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_LARGE);
                    let tx = width as i32 - margin - time_width;
                    layout.push(Widget {
                        id: "time",
                        rect: Rect {
                            x: tx,
                            y,
                            w: time_width.max(0) as u32,
                            h: canvas.line_height(FONT_LARGE).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: time_str,
                            x: tx,
                            y,
                            size: FONT_LARGE,
                            color: colors.text,
                        },
                    });
                }
            }
            y += canvas.line_height(FONT_LARGE) + 1;

            // Complication: Date (right-aligned)
            if is_enabled(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_SMALL);
                    let dx = width as i32 - margin - date_width;
                    layout.push(Widget {
                        id: "date",
                        rect: Rect {
                            x: dx,
                            y,
                            w: date_width.max(0) as u32,
                            h: line_height.max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: date_str,
                            x: dx,
                            y,
                            size: FONT_SMALL,
                            color: colors.dim,
                        },
                    });
                }
            }
            y += line_height; // Skip line for date
            y += line_height; // Extra line before Up

            // Up: on its own line (two lines below date)
            let uptime_text = format!("Up: {}", data.uptime);
            layout.push(Widget {
                id: "uptime",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&uptime_text, FONT_SMALL).max(0) as u32,
                    h: line_height.max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content: WidgetContent::Text {
                    text: uptime_text,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });
            y += line_height + section_spacing;

            // IP: on its own line
            if is_enabled(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    layout.push(Widget {
                        id: "ip_label",
                        rect: Rect {
                            x: margin,
                            y,
                            w: canvas.text_width("IP:", FONT_SMALL).max(0) as u32,
                            h: line_height.max(0) as u32,
                        },
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: "IP:".to_string(),
                            x: margin,
                            y,
                            size: FONT_SMALL,
                            color: colors.dim,
                        },
                    });
                    y += line_height;
                    // IP value on next line, possibly split for IPv6
                    let max_width = width as i32 - margin * 2;
                    let ip_width = canvas.text_width(ip, FONT_SMALL);
                    if ip_width > max_width && ip.contains(':') {
                        let mid = ip.len() / 2;
                        let split_pos = ip[..mid].rfind(':').map(|p| p + 1).unwrap_or(mid);
                        let (first, second) = ip.split_at(split_pos);
                        layout.push(Widget {
                            id: "ip_addr_line1",
                            rect: Rect {
                                x: margin,
                                y,
                                w: canvas.text_width(first, FONT_SMALL).max(0) as u32,
                                h: line_height.max(0) as u32,
                            },
                            kind: ZoneKind::Static,
                            cadence: Cadence::OnChange,
                            content: WidgetContent::Text {
                                text: first.to_string(),
                                x: margin,
                                y,
                                size: FONT_SMALL,
                                color: colors.text,
                            },
                        });
                        y += line_height;
                        layout.push(Widget {
                            id: "ip_addr_line2",
                            rect: Rect {
                                x: margin,
                                y,
                                w: canvas.text_width(second, FONT_SMALL).max(0) as u32,
                                h: line_height.max(0) as u32,
                            },
                            kind: ZoneKind::Static,
                            cadence: Cadence::OnChange,
                            content: WidgetContent::Text {
                                text: second.to_string(),
                                x: margin,
                                y,
                                size: FONT_SMALL,
                                color: colors.text,
                            },
                        });
                    } else {
                        layout.push(Widget {
                            id: "ip_addr",
                            rect: Rect {
                                x: margin,
                                y,
                                w: ip_width.max(0) as u32,
                                h: line_height.max(0) as u32,
                            },
                            kind: ZoneKind::Static,
                            cadence: Cadence::OnChange,
                            content: WidgetContent::Text {
                                text: ip.clone(),
                                x: margin,
                                y,
                                size: FONT_SMALL,
                                color: colors.text,
                            },
                        });
                    }
                    y += line_height + section_spacing * 2;
                }
            }

            // Temp: on its own line
            if is_enabled(complication_names::CPU_TEMP) {
                if let Some(temp) = data.cpu_temp {
                    layout.push(Widget {
                        id: "temp_label",
                        rect: Rect {
                            x: margin,
                            y,
                            w: canvas.text_width("Temp:", FONT_SMALL).max(0) as u32,
                            h: line_height.max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(5),
                        content: WidgetContent::Text {
                            text: "Temp:".to_string(),
                            x: margin,
                            y,
                            size: FONT_SMALL,
                            color: colors.dim,
                        },
                    });
                    let temp_val = format!("{:.0}°C", temp);
                    let temp_w = canvas.text_width(&temp_val, FONT_SMALL);
                    let tx = width as i32 - margin - temp_w;
                    layout.push(Widget {
                        id: "temp_val",
                        rect: Rect {
                            x: tx,
                            y,
                            w: temp_w.max(0) as u32,
                            h: line_height.max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(5),
                        content: WidgetContent::Text {
                            text: temp_val,
                            x: tx,
                            y,
                            size: FONT_SMALL,
                            color: colors.text,
                        },
                    });
                    y += line_height + section_spacing;
                }
            }

            // CPU: label line, then bar on next line
            let cpu_label = format!("CPU: {:2.0}%", data.cpu_percent);
            layout.push(Widget {
                id: "cpu_label",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&cpu_label, FONT_SMALL).max(0) as u32,
                    h: line_height.max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: cpu_label,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });
            y += line_height;
            let cpu_bar = ascii_bar(data.cpu_percent, bar_width);
            layout.push(Widget {
                id: "cpu_bar",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&cpu_bar, FONT_SMALL).max(0) as u32,
                    h: line_height.max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: cpu_bar,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.text,
                },
            });
            y += line_height + section_spacing;

            // RAM: label line, then bar on next line
            let ram_label = format!("RAM: {:2.0}%", data.ram_percent);
            layout.push(Widget {
                id: "ram_label",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&ram_label, FONT_SMALL).max(0) as u32,
                    h: line_height.max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: ram_label,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });
            y += line_height;
            let ram_bar = ascii_bar(data.ram_percent, bar_width);
            layout.push(Widget {
                id: "ram_bar",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&ram_bar, FONT_SMALL).max(0) as u32,
                    h: line_height.max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: ram_bar,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.text,
                },
            });
            y += line_height + section_spacing;

            // DSK: label line, then sparkline on next line
            if is_enabled(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                layout.push(Widget {
                    id: "dsk_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("DSK:", FONT_SMALL).max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "DSK:".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.dim,
                    },
                });
                let disk_rates = format!("R:{} W:{}", disk_r, disk_w);
                let disk_rates_w = canvas.text_width(&disk_rates, FONT_SMALL);
                let drx = width as i32 - margin - disk_rates_w;
                layout.push(Widget {
                    id: "dsk_rates",
                    rect: Rect {
                        x: drx,
                        y,
                        w: disk_rates_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_rates,
                        x: drx,
                        y,
                        size: FONT_SMALL,
                        color: colors.text,
                    },
                });
                y += line_height;
                let sparkline = ascii_sparkline(
                    &data.disk_history,
                    SystemData::compute_graph_scale(&data.disk_history),
                    bar_width,
                );
                let dsk_spark = format!("[{}]", sparkline);
                layout.push(Widget {
                    id: "dsk_spark",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width(&dsk_spark, FONT_SMALL).max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: dsk_spark,
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_disk,
                    },
                });
                y += line_height + section_spacing;
            }

            // NET: label line, then sparkline on next line
            if is_enabled(complication_names::NETWORK) {
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                layout.push(Widget {
                    id: "net_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("NET:", FONT_SMALL).max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "NET:".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.dim,
                    },
                });
                let net_rates = format!("D:{} U:{}", net_rx, net_tx);
                let net_rates_w = canvas.text_width(&net_rates, FONT_SMALL);
                let nrx = width as i32 - margin - net_rates_w;
                layout.push(Widget {
                    id: "net_rates",
                    rect: Rect {
                        x: nrx,
                        y,
                        w: net_rates_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_rates,
                        x: nrx,
                        y,
                        size: FONT_SMALL,
                        color: colors.text,
                    },
                });
                y += line_height;
                let sparkline = ascii_sparkline(
                    &data.net_history,
                    SystemData::compute_graph_scale(&data.net_history),
                    bar_width,
                );
                let net_spark = format!("[{}]", sparkline);
                layout.push(Widget {
                    id: "net_spark",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width(&net_spark, FONT_SMALL).max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_spark,
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_net,
                    },
                });
            }
        } else {
            // Landscape layout
            // Hostname (always shown)
            layout.push(Widget {
                id: "hostname",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&data.hostname, FONT_LARGE).max(0) as u32,
                    h: canvas.line_height(FONT_LARGE).max(0) as u32,
                },
                kind: ZoneKind::Static,
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: data.hostname.clone(),
                    x: margin,
                    y,
                    size: FONT_LARGE,
                    color: colors.highlight,
                },
            });

            // Complication: Time (right-aligned)
            if is_enabled(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    let clock_radius = 10_u32;
                    let clock_cx = width as i32 - margin - clock_radius as i32;
                    let clock_cy = y + clock_radius as i32;
                    for (i, draw) in mini_analog_clock_draws(
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.highlight,
                        colors.text,
                    )
                    .into_iter()
                    .enumerate()
                    {
                        let (id, content) = mini_clock_draw_to_widget(draw, i);
                        layout.push(Widget {
                            id,
                            rect: Rect {
                                x: clock_cx - clock_radius as i32,
                                y: clock_cy - clock_radius as i32,
                                w: clock_radius * 2,
                                h: clock_radius * 2,
                            },
                            kind: ZoneKind::Dynamic,
                            cadence: Cadence::Seconds(60),
                            content,
                        });
                    }
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_LARGE);
                    let tx = width as i32 - margin - time_width;
                    layout.push(Widget {
                        id: "time",
                        rect: Rect {
                            x: tx,
                            y,
                            w: time_width.max(0) as u32,
                            h: canvas.line_height(FONT_LARGE).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: time_str,
                            x: tx,
                            y,
                            size: FONT_LARGE,
                            color: colors.text,
                        },
                    });
                }
            }
            y += canvas.line_height(FONT_LARGE) + 1;

            // Complication: Date (right-aligned, under time)
            if is_enabled(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_NORMAL);
                    let dx = width as i32 - margin - date_width;
                    layout.push(Widget {
                        id: "date",
                        rect: Rect {
                            x: dx,
                            y,
                            w: date_width.max(0) as u32,
                            h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: date_str,
                            x: dx,
                            y,
                            size: FONT_NORMAL,
                            color: colors.dim,
                        },
                    });
                }
            }

            // Base element: Uptime (always shown, same line as date on left)
            let uptime_text = format!("Up: {}", data.uptime);
            layout.push(Widget {
                id: "uptime",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&uptime_text, FONT_NORMAL).max(0) as u32,
                    h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content: WidgetContent::Text {
                    text: uptime_text,
                    x: margin,
                    y,
                    size: FONT_NORMAL,
                    color: colors.dim,
                },
            });
            y += canvas.line_height(FONT_NORMAL) + 1;

            // Complication: IP address
            if is_enabled(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    let ip_text = format!("IP: {}", ip);
                    layout.push(Widget {
                        id: "ip_addr",
                        rect: Rect {
                            x: margin,
                            y,
                            w: canvas.text_width(&ip_text, FONT_SMALL).max(0) as u32,
                            h: canvas.line_height(FONT_SMALL).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: ip_text,
                            x: margin,
                            y,
                            size: FONT_SMALL,
                            color: colors.dim,
                        },
                    });
                    y += canvas.line_height(FONT_SMALL) + 4;
                } else {
                    y += 4;
                }
            }

            // Base element: CPU bar with optional temperature (always shown)
            let cpu_bar = ascii_bar(data.cpu_percent, bar_chars);
            let cpu_text = if is_enabled(complication_names::CPU_TEMP) {
                if let Some(temp) = data.cpu_temp {
                    format!("CPU {} {:3.0}%  {:.0}°C", cpu_bar, data.cpu_percent, temp)
                } else {
                    format!("CPU {} {:3.0}%", cpu_bar, data.cpu_percent)
                }
            } else {
                format!("CPU {} {:3.0}%", cpu_bar, data.cpu_percent)
            };
            layout.push(Widget {
                id: "cpu_bar",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&cpu_text, FONT_NORMAL).max(0) as u32,
                    h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: cpu_text,
                    x: margin,
                    y,
                    size: FONT_NORMAL,
                    color: colors.text,
                },
            });
            y += canvas.line_height(FONT_NORMAL) + 1;

            // Base element: RAM bar (always shown)
            let ram_bar = ascii_bar(data.ram_percent, bar_chars);
            let ram_text = format!("RAM {} {:3.0}%", ram_bar, data.ram_percent);
            layout.push(Widget {
                id: "ram_bar",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&ram_text, FONT_NORMAL).max(0) as u32,
                    h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: ram_text,
                    x: margin,
                    y,
                    size: FONT_NORMAL,
                    color: colors.text,
                },
            });
            y += canvas.line_height(FONT_NORMAL) + 2;

            // Complication: Disk I/O
            if is_enabled(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                layout.push(Widget {
                    id: "dsk_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("DSK", FONT_NORMAL).max(0) as u32,
                        h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "DSK".to_string(),
                        x: margin,
                        y,
                        size: FONT_NORMAL,
                        color: colors.text,
                    },
                });
                let disk_rates = format!("R:{} W:{}", disk_r, disk_w);
                layout.push(Widget {
                    id: "dsk_rates",
                    rect: Rect {
                        x: margin + 40,
                        y,
                        w: canvas.text_width(&disk_rates, FONT_NORMAL).max(0) as u32,
                        h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_rates,
                        x: margin + 40,
                        y,
                        size: FONT_NORMAL,
                        color: colors.dim,
                    },
                });
                y += canvas.line_height(FONT_NORMAL);
                let sparkline = ascii_sparkline(
                    &data.disk_history,
                    SystemData::compute_graph_scale(&data.disk_history),
                    bar_chars + 20,
                );
                let dsk_spark = format!("[{}]", sparkline);
                layout.push(Widget {
                    id: "dsk_spark",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width(&dsk_spark, FONT_NORMAL).max(0) as u32,
                        h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: dsk_spark,
                        x: margin,
                        y,
                        size: FONT_NORMAL,
                        color: colors.bar_disk,
                    },
                });
                y += canvas.line_height(FONT_NORMAL) + 2;
            }

            // Complication: Network
            if is_enabled(complication_names::NETWORK) {
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                layout.push(Widget {
                    id: "net_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("NET", FONT_NORMAL).max(0) as u32,
                        h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "NET".to_string(),
                        x: margin,
                        y,
                        size: FONT_NORMAL,
                        color: colors.text,
                    },
                });
                let net_rates = format!("D:{} U:{}", net_rx, net_tx);
                layout.push(Widget {
                    id: "net_rates",
                    rect: Rect {
                        x: margin + 40,
                        y,
                        w: canvas.text_width(&net_rates, FONT_NORMAL).max(0) as u32,
                        h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_rates,
                        x: margin + 40,
                        y,
                        size: FONT_NORMAL,
                        color: colors.dim,
                    },
                });
                y += canvas.line_height(FONT_NORMAL);
                let sparkline = ascii_sparkline(
                    &data.net_history,
                    SystemData::compute_graph_scale(&data.net_history),
                    bar_chars + 20,
                );
                let net_spark = format!("[{}]", sparkline);
                layout.push(Widget {
                    id: "net_spark",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width(&net_spark, FONT_NORMAL).max(0) as u32,
                        h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_spark,
                        x: margin,
                        y,
                        size: FONT_NORMAL,
                        color: colors.bar_net,
                    },
                });
            }
        }
        let _ = y;
        Some(layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::layout::render_layout;
    use crate::faces::Theme;
    use crate::faces::{EnabledComplications, Face};
    use crate::rendering::Canvas;
    use crate::sensors::data::SystemData;

    // Deterministic sample data so both render paths see identical input.
    #[allow(clippy::field_reassign_with_default)]
    fn sample() -> SystemData {
        let mut d = SystemData::default();
        d.hostname = "endeavour".into();
        d.uptime = "5d 12h".into();
        d.cpu_percent = 45.0;
        d.ram_percent = 67.0;
        d.cpu_temp = Some(45.0);
        d.hour = 18;
        d.minute = 45;
        d.disk_history = vec![0.1, 0.5, 0.9, 0.3].into();
        d.net_history = vec![0.3, 0.7, 0.2, 0.8].into();
        d
    }

    fn render_both(width: u32, height: u32) -> (Vec<u8>, Vec<u8>) {
        render_both_with(width, height, sample())
    }

    fn render_both_with(width: u32, height: u32, data: SystemData) -> (Vec<u8>, Vec<u8>) {
        render_both_comps(width, height, data, EnabledComplications::new())
    }

    #[allow(clippy::field_reassign_with_default)]
    fn sample_with_ip(ip: &str) -> SystemData {
        let mut d = sample();
        d.display_ip = Some(ip.to_string());
        d
    }

    /// Landscape pixel-identical equivalence: default complications.
    #[test]
    fn ascii_layout_matches_render_landscape() {
        let (legacy, via_layout) = render_both(320, 170);
        assert_eq!(
            legacy, via_layout,
            "ascii landscape mismatch (default comps)"
        );
    }

    /// Landscape pixel-identical equivalence: alternate complication set (IP populated).
    #[test]
    fn ascii_layout_matches_render_landscape_with_ip() {
        let (legacy, via_layout) = render_both_with(320, 170, sample_with_ip("192.168.1.100"));
        assert_eq!(legacy, via_layout, "ascii landscape mismatch (IPv4)");
    }

    /// Helper: build complications configured for ANALOGUE time.
    fn analogue_comps() -> EnabledComplications {
        let mut comps = EnabledComplications::new();
        comps.set_enabled("ascii", complication_names::TIME, true);
        comps.set_option(
            "ascii",
            complication_names::TIME,
            complication_options::TIME_FORMAT,
            time_formats::ANALOGUE.to_string(),
        );
        comps
    }

    /// Helper: render via both paths for a given complications set.
    fn render_both_comps(
        width: u32,
        height: u32,
        data: SystemData,
        comps: EnabledComplications,
    ) -> (Vec<u8>, Vec<u8>) {
        let face = AsciiFace::new();
        let theme = Theme::from_preset("default");

        let mut legacy = Canvas::new(width, height);
        legacy.set_background(0);
        legacy.clear();
        face.render(&mut legacy, &data, &theme, &comps);

        let mut via_layout = Canvas::new(width, height);
        let lay = face
            .layout(&via_layout, &data, &theme, &comps)
            .expect("layout() should be Some");
        via_layout.set_background(0);
        via_layout.clear();
        render_layout(&mut via_layout, &lay);

        (legacy.pixels().to_vec(), via_layout.pixels().to_vec())
    }

    /// Portrait pixel-identical equivalence: default complications (digital time).
    #[test]
    fn ascii_layout_matches_render_portrait() {
        let (legacy, via_layout) = render_both(170, 320);
        assert_eq!(
            legacy, via_layout,
            "ascii portrait mismatch (default comps)"
        );
    }

    /// Portrait pixel-identical equivalence: IPv4 address populated.
    #[test]
    fn ascii_layout_matches_render_portrait_with_ip() {
        let (legacy, via_layout) = render_both_with(170, 320, sample_with_ip("192.168.1.100"));
        assert_eq!(legacy, via_layout, "ascii portrait mismatch (IPv4)");
    }

    /// Portrait + IPv6: exercises the rfind(':') / split_at two-line split path.
    #[test]
    fn ascii_layout_portrait_ipv6_split_produces_two_widgets() {
        let face = AsciiFace::new();
        let data = sample_with_ip("2001:db8::dead:beef:1:2");
        let theme = Theme::from_preset("default");
        let comps = EnabledComplications::new();
        let canvas = Canvas::new(170, 320);
        let layout = face
            .layout(&canvas, &data, &theme, &comps)
            .expect("layout() must return Some");
        let ids: Vec<&str> = layout.widgets.iter().map(|w| w.id).collect();
        // IPv6 long enough to wrap: both lines must be present.
        assert!(
            ids.contains(&"ip_addr_line1") && ids.contains(&"ip_addr_line2"),
            "IPv6 wrap widgets missing; got: {ids:?}"
        );
    }

    /// Portrait + IPv6: pixel-identical equivalence across the highest-complexity
    /// portrait branch (the rfind(':')/split_at two-line wrap). Parity with the
    /// professional face's IPv6 coverage.
    #[test]
    fn ascii_layout_matches_render_portrait_ipv6() {
        let (legacy, via_layout) =
            render_both_with(170, 320, sample_with_ip("2001:db8::dead:beef:1:2"));
        assert_eq!(legacy, via_layout, "ascii portrait mismatch (IPv6 split)");
    }

    /// Landscape ANALOGUE time: pixel-identical equivalence.
    #[test]
    fn ascii_layout_matches_render_landscape_analogue() {
        let (legacy, via_layout) = render_both_comps(320, 170, sample(), analogue_comps());
        assert_eq!(legacy, via_layout, "ascii landscape analogue mismatch");
    }

    /// Portrait ANALOGUE time: pixel-identical equivalence.
    #[test]
    fn ascii_layout_matches_render_portrait_analogue() {
        let (legacy, via_layout) = render_both_comps(170, 320, sample(), analogue_comps());
        assert_eq!(legacy, via_layout, "ascii portrait analogue mismatch");
    }
}
