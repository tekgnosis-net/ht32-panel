//! Professional face with graphical progress bars.
//!
//! Landscape layout (320x170):
//! ```text
//! endeavour               18:45
//! Up: 5d 12h 34m          2025-01-31
//! IP:                  192.168.1.100
//! Temp:                        45°C
//! CPU: 45%
//! [████████████░░░░░░░░░░░░░░░░░░]
//! RAM: 67%
//! [██████████████████░░░░░░░░░░░░]
//! DSK:                   R:12M W:5M
//! [▁▁▂▃▄▅▆▇███▇▆▅▄▃▂▁▁▁▁▁▁▁▁▁▁▁▁]
//! NET:                 ↓:1.2M ↑:0.8M
//! [▁▁▁▂▂▃▃▄▄▅▅▆▆▇▇████▇▇▆▆▅▅▄▄▃▃]
//! ```

use super::{
    complication_names, complication_options, complications, date_formats, mini_analog_clock_draws,
    mini_clock_draw_to_widget, time_formats, Complication, EnabledComplications, Face, Theme,
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

/// Derive colors from theme for the professional face.
struct FaceColors {
    /// Primary highlight color (hostname, interface name)
    highlight: u32,
    /// Main text color
    text: u32,
    /// Dimmed text color (uptime, IPs)
    dim: u32,
    /// Progress bar background
    bar_bg: u32,
    /// CPU bar fill color
    bar_cpu: u32,
    /// RAM bar fill color
    bar_ram: u32,
    /// Disk read bar fill color
    bar_disk_read: u32,
    /// Disk write bar fill color
    bar_disk_write: u32,
    /// Network receive bar fill color
    bar_net_rx: u32,
    /// Network transmit bar fill color
    bar_net_tx: u32,
}

impl FaceColors {
    fn from_theme(theme: &Theme) -> Self {
        // Create distinct shades for read/write and rx/tx
        let disk_base = dim_color(theme.primary, theme.secondary, 0.5);
        let net_base = theme.secondary;

        Self {
            highlight: theme.primary,
            text: theme.text,
            dim: dim_color(theme.text, theme.background, 0.7), // Higher for better contrast
            bar_bg: dim_color(theme.primary, theme.background, 0.2),
            bar_cpu: theme.primary,
            bar_ram: theme.secondary,
            // Disk: read is brighter, write is dimmer
            bar_disk_read: disk_base,
            bar_disk_write: dim_color(disk_base, theme.background, 0.6),
            // Network: rx (download) is brighter, tx (upload) is dimmer
            bar_net_rx: net_base,
            bar_net_tx: dim_color(net_base, theme.background, 0.6),
        }
    }
}

/// Font sizes.
const FONT_LARGE: f32 = 16.0;
const FONT_NORMAL: f32 = 14.0;
const FONT_SMALL: f32 = 12.0;

/// Progress bar dimensions.
const BAR_WIDTH: u32 = 120;
const BAR_HEIGHT: u32 = 10;

/// Graph dimensions.
const GRAPH_HEIGHT: u32 = 16;

/// A professional face with graphical progress bars.
pub struct ProfessionalFace;

impl ProfessionalFace {
    /// Creates a new professional face.
    pub fn new() -> Self {
        Self
    }

    /// Builds the typed-widget layout: text fields become `Text` widgets,
    /// progress bars become `Bar` widgets, and the disk/net history graphs
    /// become `DualSparkline` widgets, drawn by
    /// [`crate::faces::layout::render_layout`].
    ///
    /// TOTAL across every config, including ANALOGUE time (which emits
    /// `Line`/`Arc`/`Circle` widgets from `mini_analog_clock_draws`).
    fn build_layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        complications: &EnabledComplications,
    ) -> Layout {
        let colors = FaceColors::from_theme(theme);
        let (width, _height) = canvas.dimensions();
        let portrait = width < 200;
        let margin = 8;
        let mut y = margin;
        let mut layout = Layout::new();

        // Helper to check if a complication is enabled
        let is_enabled = |id: &str| -> bool { complications.is_enabled(self.name(), id, true) };

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
            // Portrait layout - full width bars on own lines, stacked text
            let bar_width = (width - (margin * 2) as u32).min(200);
            let tall_bar_height = 14_u32; // Taller bars for CPU/RAM
            let section_spacing = 6; // Extra spacing between sections
            let line_height = canvas.line_height(FONT_SMALL);

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
                    // Draw small analog clock on the right.
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
            y += canvas.line_height(FONT_LARGE) + 2;

            // Complication: Date (right-aligned, under time)
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
                    y += line_height;
                }
            }

            // Two lines lower before Uptime
            y += line_height * 2;

            // Base element: Uptime (always shown)
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

            // Complication: IP address with label on its own line
            if is_enabled(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    // IP label on its own line
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
                    // IP address on next line
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

            // Complication: CPU temperature
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
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
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
                        id: "temp_value",
                        rect: Rect {
                            x: tx,
                            y,
                            w: temp_w.max(0) as u32,
                            h: line_height.max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::OnChange,
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

            // Base element: CPU label on its own line, then bar below
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
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: cpu_label,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });
            y += line_height;
            layout.push(Widget {
                id: "cpu_bar",
                rect: Rect {
                    x: margin,
                    y,
                    w: bar_width,
                    h: tall_bar_height,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::OnChange,
                content: WidgetContent::Bar {
                    x: margin,
                    y,
                    w: bar_width,
                    h: tall_bar_height,
                    percent: data.cpu_percent,
                    fill: colors.bar_cpu,
                    bg: colors.bar_bg,
                },
            });
            y += tall_bar_height as i32 + section_spacing;

            // Base element: RAM label on its own line, then bar below
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
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: ram_label,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });
            y += line_height;
            layout.push(Widget {
                id: "ram_bar",
                rect: Rect {
                    x: margin,
                    y,
                    w: bar_width,
                    h: tall_bar_height,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::OnChange,
                content: WidgetContent::Bar {
                    x: margin,
                    y,
                    w: bar_width,
                    h: tall_bar_height,
                    percent: data.ram_percent,
                    fill: colors.bar_ram,
                    bg: colors.bar_bg,
                },
            });
            y += tall_bar_height as i32 + section_spacing;

            // Complication: Disk I/O graph
            if is_enabled(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                layout.push(Widget {
                    id: "disk_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("DSK:", FONT_SMALL).max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "DSK:".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.dim,
                    },
                });
                // Draw R: and W: in their respective colors
                let r_text = format!("R:{}", disk_r);
                let w_text = format!(" W:{}", disk_w);
                let w_text_w = canvas.text_width(&w_text, FONT_SMALL);
                let r_text_w = canvas.text_width(&r_text, FONT_SMALL);
                let r_x = width as i32 - margin - w_text_w - r_text_w;
                layout.push(Widget {
                    id: "disk_read_value",
                    rect: Rect {
                        x: r_x,
                        y,
                        w: r_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: r_text,
                        x: r_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_disk_read,
                    },
                });
                layout.push(Widget {
                    id: "disk_write_value",
                    rect: Rect {
                        x: r_x + r_text_w,
                        y,
                        w: w_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: w_text,
                        x: r_x + r_text_w,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_disk_write,
                    },
                });
                y += line_height;
                layout.push(Widget {
                    id: "disk_graph",
                    rect: Rect {
                        x: margin,
                        y,
                        w: bar_width,
                        h: GRAPH_HEIGHT,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::DualSparkline {
                        x: margin,
                        y,
                        w: bar_width,
                        h: GRAPH_HEIGHT,
                        a: data.disk_read_history.iter().copied().collect(),
                        b: data.disk_write_history.iter().copied().collect(),
                        scale: SystemData::compute_graph_scale(&data.disk_history),
                        color_a: colors.bar_disk_read,
                        color_b: colors.bar_disk_write,
                        bg: colors.bar_bg,
                        wrap_around: false,
                    },
                });
                y += GRAPH_HEIGHT as i32 + section_spacing;
            }

            // Complication: Network I/O graph
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
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "NET:".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.dim,
                    },
                });
                // Draw ↓: and ↑: in their respective colors
                let rx_text = format!("\u{2193}:{}", net_rx);
                let tx_text = format!(" \u{2191}:{}", net_tx);
                let tx_text_w = canvas.text_width(&tx_text, FONT_SMALL);
                let rx_text_w = canvas.text_width(&rx_text, FONT_SMALL);
                let rx_x = width as i32 - margin - tx_text_w - rx_text_w;
                layout.push(Widget {
                    id: "net_rx_value",
                    rect: Rect {
                        x: rx_x,
                        y,
                        w: rx_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: rx_text,
                        x: rx_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_net_rx,
                    },
                });
                layout.push(Widget {
                    id: "net_tx_value",
                    rect: Rect {
                        x: rx_x + rx_text_w,
                        y,
                        w: tx_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: tx_text,
                        x: rx_x + rx_text_w,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_net_tx,
                    },
                });
                y += line_height;
                layout.push(Widget {
                    id: "net_graph",
                    rect: Rect {
                        x: margin,
                        y,
                        w: bar_width,
                        h: GRAPH_HEIGHT,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::DualSparkline {
                        x: margin,
                        y,
                        w: bar_width,
                        h: GRAPH_HEIGHT,
                        a: data.net_rx_history.iter().copied().collect(),
                        b: data.net_tx_history.iter().copied().collect(),
                        scale: SystemData::compute_graph_scale(&data.net_history),
                        color_a: colors.bar_net_rx,
                        color_b: colors.bar_net_tx,
                        bg: colors.bar_bg,
                        wrap_around: false,
                    },
                });
            }
        } else {
            // Landscape layout - compact with bars on same line as labels
            let line_height = canvas.line_height(FONT_SMALL);
            let label_width = 70_i32; // Space for "CPU: 99%" or "RAM: 99%"
            let bar_x = margin + label_width;
            let bar_width = (width as i32 - bar_x - margin - 40) as u32; // Leave room for temp

            // Hostname (always shown)
            y = 1;
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
                    // Draw small analog clock on the right.
                    let clock_radius = 10_u32;
                    let clock_cx = width as i32 - margin - clock_radius as i32;
                    let clock_cy = y + clock_radius as i32 + 2;
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

            // Up: on left side
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
            y += line_height + 1;

            // IP: label and address on same line, left aligned
            if is_enabled(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    let ip_text = format!("IP: {}", ip);
                    layout.push(Widget {
                        id: "ip",
                        rect: Rect {
                            x: margin,
                            y,
                            w: canvas.text_width(&ip_text, FONT_SMALL).max(0) as u32,
                            h: line_height.max(0) as u32,
                        },
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: ip_text,
                            x: margin,
                            y,
                            size: FONT_SMALL,
                            color: colors.dim,
                        },
                    });
                    y += line_height + 2;
                }
            }

            // CPU: label, bar, and temp all on same line
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
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: cpu_label,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });
            layout.push(Widget {
                id: "cpu_bar",
                rect: Rect {
                    x: bar_x,
                    y: y + 2,
                    w: bar_width,
                    h: BAR_HEIGHT,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::OnChange,
                content: WidgetContent::Bar {
                    x: bar_x,
                    y: y + 2,
                    w: bar_width,
                    h: BAR_HEIGHT,
                    percent: data.cpu_percent,
                    fill: colors.bar_cpu,
                    bg: colors.bar_bg,
                },
            });
            // CPU temp on same line (no label)
            if is_enabled(complication_names::CPU_TEMP) {
                if let Some(temp) = data.cpu_temp {
                    let temp_val = format!("{:.0}°C", temp);
                    let temp_w = canvas.text_width(&temp_val, FONT_SMALL);
                    let tx = width as i32 - margin - temp_w;
                    layout.push(Widget {
                        id: "temp_value",
                        rect: Rect {
                            x: tx,
                            y,
                            w: temp_w.max(0) as u32,
                            h: line_height.max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: temp_val,
                            x: tx,
                            y,
                            size: FONT_SMALL,
                            color: colors.text,
                        },
                    });
                }
            }
            y += line_height + 2;

            // RAM: label and bar on same line
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
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: ram_label,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });
            layout.push(Widget {
                id: "ram_bar",
                rect: Rect {
                    x: bar_x,
                    y: y + 2,
                    w: bar_width,
                    h: BAR_HEIGHT,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::OnChange,
                content: WidgetContent::Bar {
                    x: bar_x,
                    y: y + 2,
                    w: bar_width,
                    h: BAR_HEIGHT,
                    percent: data.ram_percent,
                    fill: colors.bar_ram,
                    bg: colors.bar_bg,
                },
            });
            y += line_height + 8;

            // DSK: label line, then graph on next line
            if is_enabled(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                layout.push(Widget {
                    id: "disk_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("DSK:", FONT_SMALL).max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "DSK:".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.dim,
                    },
                });
                // Draw R: and W: in their respective colors
                let r_text = format!("R:{}", disk_r);
                let w_text = format!(" W:{}", disk_w);
                let w_text_w = canvas.text_width(&w_text, FONT_SMALL);
                let r_text_w = canvas.text_width(&r_text, FONT_SMALL);
                let r_x = width as i32 - margin - w_text_w - r_text_w;
                layout.push(Widget {
                    id: "disk_read_value",
                    rect: Rect {
                        x: r_x,
                        y,
                        w: r_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: r_text,
                        x: r_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_disk_read,
                    },
                });
                layout.push(Widget {
                    id: "disk_write_value",
                    rect: Rect {
                        x: r_x + r_text_w,
                        y,
                        w: w_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: w_text,
                        x: r_x + r_text_w,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_disk_write,
                    },
                });
                y += line_height + 4;
                layout.push(Widget {
                    id: "disk_graph",
                    rect: Rect {
                        x: margin,
                        y,
                        w: width - (margin * 2) as u32,
                        h: GRAPH_HEIGHT,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::DualSparkline {
                        x: margin,
                        y,
                        w: width - (margin * 2) as u32,
                        h: GRAPH_HEIGHT,
                        a: data.disk_read_history.iter().copied().collect(),
                        b: data.disk_write_history.iter().copied().collect(),
                        scale: SystemData::compute_graph_scale(&data.disk_history),
                        color_a: colors.bar_disk_read,
                        color_b: colors.bar_disk_write,
                        bg: colors.bar_bg,
                        wrap_around: false,
                    },
                });
                y += GRAPH_HEIGHT as i32 + 4;
            }

            // NET: label line, then graph on next line
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
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "NET:".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.dim,
                    },
                });
                // Draw ↓: and ↑: in their respective colors
                let rx_text = format!("\u{2193}:{}", net_rx);
                let tx_text = format!(" \u{2191}:{}", net_tx);
                let tx_text_w = canvas.text_width(&tx_text, FONT_SMALL);
                let rx_text_w = canvas.text_width(&rx_text, FONT_SMALL);
                let rx_x = width as i32 - margin - tx_text_w - rx_text_w;
                layout.push(Widget {
                    id: "net_rx_value",
                    rect: Rect {
                        x: rx_x,
                        y,
                        w: rx_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: rx_text,
                        x: rx_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_net_rx,
                    },
                });
                layout.push(Widget {
                    id: "net_tx_value",
                    rect: Rect {
                        x: rx_x + rx_text_w,
                        y,
                        w: tx_text_w.max(0) as u32,
                        h: line_height.max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: tx_text,
                        x: rx_x + rx_text_w,
                        y,
                        size: FONT_SMALL,
                        color: colors.bar_net_tx,
                    },
                });
                y += line_height + 4;
                layout.push(Widget {
                    id: "net_graph",
                    rect: Rect {
                        x: margin,
                        y,
                        w: width - (margin * 2) as u32,
                        h: GRAPH_HEIGHT,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::DualSparkline {
                        x: margin,
                        y,
                        w: width - (margin * 2) as u32,
                        h: GRAPH_HEIGHT,
                        a: data.net_rx_history.iter().copied().collect(),
                        b: data.net_tx_history.iter().copied().collect(),
                        scale: SystemData::compute_graph_scale(&data.net_history),
                        color_a: colors.bar_net_rx,
                        color_b: colors.bar_net_tx,
                        bg: colors.bar_bg,
                        wrap_around: false,
                    },
                });
            }
        }
        // Suppress unused variable warning when all complications are disabled
        let _ = y;
        layout
    }
}

impl Default for ProfessionalFace {
    fn default() -> Self {
        Self::new()
    }
}

impl Face for ProfessionalFace {
    fn name(&self) -> &str {
        "professional"
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

    fn layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        complications: &EnabledComplications,
    ) -> Layout {
        self.build_layout(canvas, data, theme, complications)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::layout::{pixel_hash, render_layout};
    use crate::faces::Theme;
    use crate::faces::{EnabledComplications, Face};
    use crate::rendering::Canvas;
    use crate::sensors::data::SystemData;

    // Deterministic sample data so both render paths see identical input.
    // Field-by-field reassignment mirrors the task brief's harness verbatim.
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
        d.disk_read_history = vec![0.1, 0.5, 0.9, 0.3].into();
        d.disk_write_history = vec![0.2, 0.4, 0.1, 0.6].into();
        d.net_rx_history = vec![0.3, 0.7, 0.2, 0.8].into();
        d.net_tx_history = vec![0.1, 0.2, 0.5, 0.4].into();
        d
    }

    fn render_both(width: u32, height: u32) -> Vec<u8> {
        render_both_with(width, height, sample())
    }

    fn render_both_with(width: u32, height: u32, data: SystemData) -> Vec<u8> {
        let face = ProfessionalFace::new();
        let theme = Theme::from_preset("default");
        let comps = EnabledComplications::new();

        let mut via_layout = Canvas::new(width, height);
        let lay = face.layout(&via_layout, &data, &theme, &comps);
        via_layout.clear();
        render_layout(&mut via_layout, &lay);

        via_layout.pixels().to_vec()
    }

    #[test]
    fn layout_matches_legacy_render_landscape() {
        let via_layout = render_both(320, 170);
        assert_eq!(
            pixel_hash(&via_layout),
            10701061824176020197,
            "golden drift: layout_matches_legacy_render_landscape"
        );
    }

    #[test]
    fn layout_matches_legacy_render_portrait() {
        let via_layout = render_both(170, 320);
        assert_eq!(
            pixel_hash(&via_layout),
            14665955793822149985,
            "golden drift: layout_matches_legacy_render_portrait"
        );
    }

    // Returns a SystemData with display_ip set to the given string, all other
    // fields identical to sample().
    #[allow(clippy::field_reassign_with_default)]
    fn sample_with_ip(ip: &str) -> SystemData {
        let mut d = sample();
        d.display_ip = Some(ip.to_string());
        d
    }

    /// Portrait + long IPv6 → exercises the `rfind(':')`/`split_at` two-line path.
    #[test]
    fn layout_matches_legacy_render_portrait_ipv6_wrap() {
        let data = sample_with_ip("2001:db8::dead:beef:1:2");
        let via_layout = render_both_with(170, 320, data);
        assert_eq!(
            pixel_hash(&via_layout),
            15172485710738808832,
            "golden drift: layout_matches_legacy_render_portrait_ipv6_wrap"
        );
    }

    /// Landscape + short IPv4 → exercises the single-line landscape IP path.
    #[test]
    fn layout_matches_legacy_render_landscape_ipv4() {
        let data = sample_with_ip("192.168.1.100");
        let via_layout = render_both_with(320, 170, data);
        assert_eq!(
            pixel_hash(&via_layout),
            5058213149468129392,
            "golden drift: layout_matches_legacy_render_landscape_ipv4"
        );
    }

    /// Portrait + short IPv4 → exercises the portrait single-line IP path.
    #[test]
    fn layout_matches_legacy_render_portrait_ipv4() {
        let data = sample_with_ip("192.168.1.100");
        let via_layout = render_both_with(170, 320, data);
        assert_eq!(
            pixel_hash(&via_layout),
            14830570311864835861,
            "golden drift: layout_matches_legacy_render_portrait_ipv4"
        );
    }

    fn analogue_comps() -> EnabledComplications {
        use crate::faces::{complication_names, complication_options, time_formats};
        let mut comps = EnabledComplications::new();
        comps.set_enabled("professional", complication_names::TIME, true);
        comps.set_option(
            "professional",
            complication_names::TIME,
            complication_options::TIME_FORMAT,
            time_formats::ANALOGUE.to_string(),
        );
        comps
    }

    #[test]
    fn analogue_layout_matches_legacy_render_landscape() {
        let comps = analogue_comps();
        let face = ProfessionalFace::new();
        let data = sample();
        let theme = Theme::from_preset("default");

        let mut via_layout = Canvas::new(320, 170);
        let lay = face.layout(&via_layout, &data, &theme, &comps);
        via_layout.clear();
        crate::faces::layout::render_layout(&mut via_layout, &lay);

        assert_eq!(
            pixel_hash(via_layout.pixels()),
            12892245314711865167,
            "golden drift: analogue_layout_matches_legacy_render_landscape"
        );
    }

    #[test]
    fn analogue_layout_matches_legacy_render_portrait() {
        let comps = analogue_comps();
        let face = ProfessionalFace::new();
        let data = sample();
        let theme = Theme::from_preset("default");

        let mut via_layout = Canvas::new(170, 320);
        let lay = face.layout(&via_layout, &data, &theme, &comps);
        via_layout.clear();
        crate::faces::layout::render_layout(&mut via_layout, &lay);

        assert_eq!(
            pixel_hash(via_layout.pixels()),
            9580359624296381367,
            "golden drift: analogue_layout_matches_legacy_render_portrait"
        );
    }
}
