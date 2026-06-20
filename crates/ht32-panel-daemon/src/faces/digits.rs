//! Digits face inspired by Casio digital watches.
//!
//! Features a retro LCD aesthetic with large time display and
//! segmented areas for system metrics.

use super::{
    complication_names, complication_options, complications, date_formats, draw_mini_analog_clock,
    mini_analog_clock_draws, mini_clock_draw_to_widget, time_formats, Complication,
    EnabledComplications, Face, Theme,
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

/// Derive colors from theme for the digits face.
struct FaceColors {
    /// LCD segment "on" color
    segment_on: u32,
    /// LCD segment "off" color (ghost segments)
    segment_off: u32,
    /// Label text color
    label: u32,
    /// Divider line color
    divider: u32,
}

impl FaceColors {
    fn from_theme(theme: &Theme) -> Self {
        Self {
            segment_on: theme.primary,
            segment_off: dim_color(theme.primary, theme.background, 0.2),
            label: dim_color(theme.text, theme.background, 0.7), // Higher for better contrast
            divider: dim_color(theme.primary, theme.background, 0.35),
        }
    }
}

/// Font sizes.
const FONT_TIME: f32 = 32.0;
const FONT_LARGE: f32 = 20.0;
const FONT_MEDIUM: f32 = 14.0;
const FONT_SMALL: f32 = 11.0;

/// A Casio-inspired digital watch face.
pub struct DigitsFace;

impl DigitsFace {
    /// Creates a new digits face.
    pub fn new() -> Self {
        Self
    }

    /// Draws a horizontal divider line.
    fn draw_divider(canvas: &mut Canvas, y: i32, width: u32, margin: i32, color: u32) {
        canvas.fill_rect(margin, y, width - (margin * 2) as u32, 1, color);
    }

    /// Draws a labeled value in the segmented LCD style.
    fn draw_segment_value(
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        label: &str,
        value: &str,
        label_color: u32,
        value_color: u32,
    ) {
        canvas.draw_text(x, y, label, FONT_SMALL, label_color);
        canvas.draw_text(x, y + 10, value, FONT_LARGE, value_color);
    }

    /// Draws a labeled value with medium fonts for landscape CPU/RAM row.
    fn draw_segment_value_medium(
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        label: &str,
        value: &str,
        label_color: u32,
        value_color: u32,
    ) {
        canvas.draw_text(x, y, label, FONT_SMALL, label_color);
        canvas.draw_text(x, y + 12, value, 26.0, value_color); // Between FONT_LARGE (20) and FONT_TIME (32)
    }

    /// Pushes a divider Bar widget (100%-filled = solid fill_rect, byte-identical).
    fn push_divider(layout: &mut Layout, y: i32, width: u32, margin: i32, color: u32) {
        layout.push(Widget {
            id: "divider",
            rect: Rect {
                x: margin,
                y,
                w: width - (margin * 2) as u32,
                h: 1,
            },
            kind: ZoneKind::Static,
            cadence: Cadence::OnChange,
            content: WidgetContent::Bar {
                x: margin,
                y,
                w: width - (margin * 2) as u32,
                h: 1,
                percent: 100.0,
                fill: color,
                bg: color,
            },
        });
    }

    /// Builds the typed-widget layout, covering ALL configs including ANALOGUE
    /// time (which emits `Line`/`Arc`/`Circle` widgets from `mini_analog_clock_draws`).
    fn build_layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        comp: &EnabledComplications,
    ) -> Option<Layout> {
        let colors = FaceColors::from_theme(theme);
        let (width, _height) = canvas.dimensions();
        let portrait = width < 200;
        let margin = 6;
        let mut y = margin;
        let mut layout = Layout::new();

        let is_on = |id: &str| comp.is_enabled(self.name(), id, true);

        let time_format = comp
            .get_option(
                self.name(),
                complication_names::TIME,
                complication_options::TIME_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(time_formats::DIGITAL_24H);

        let date_format = comp
            .get_option(
                self.name(),
                complication_names::DATE,
                complication_options::DATE_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(date_formats::ISO);

        if portrait {
            // Portrait layout
            let col_width = (width as i32 - margin * 3) / 2;

            // Hostname at top (always shown)
            let host_width = canvas.text_width(&data.hostname, FONT_MEDIUM);
            let host_x = (width as i32 - host_width) / 2;
            layout.push(Widget {
                id: "hostname",
                rect: Rect {
                    x: host_x,
                    y,
                    w: host_width.max(0) as u32,
                    h: canvas.line_height(FONT_MEDIUM).max(0) as u32,
                },
                kind: ZoneKind::Static,
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: data.hostname.clone(),
                    x: host_x,
                    y,
                    size: FONT_MEDIUM,
                    color: colors.label,
                },
            });
            y += canvas.line_height(FONT_MEDIUM) + 2;

            // Complication: Time
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    let clock_radius = 18_u32;
                    let clock_cx = width as i32 / 2;
                    let clock_cy = y + clock_radius as i32 + 2;
                    for (i, draw) in mini_analog_clock_draws(
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.segment_on,
                        colors.segment_on,
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
                    y += (clock_radius * 2) as i32 + 6;
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_TIME);
                    let time_x = (width as i32 - time_width) / 2;
                    layout.push(Widget {
                        id: "time",
                        rect: Rect {
                            x: time_x,
                            y,
                            w: time_width.max(0) as u32,
                            h: canvas.line_height(FONT_TIME).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: time_str,
                            x: time_x,
                            y,
                            size: FONT_TIME,
                            color: colors.segment_on,
                        },
                    });
                    y += canvas.line_height(FONT_TIME) + 2;
                }
            }

            // Complication: Date (centered, if not hidden)
            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_MEDIUM);
                    let date_x = (width as i32 - date_width) / 2;
                    layout.push(Widget {
                        id: "date",
                        rect: Rect {
                            x: date_x,
                            y,
                            w: date_width.max(0) as u32,
                            h: canvas.line_height(FONT_MEDIUM).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: date_str,
                            x: date_x,
                            y,
                            size: FONT_MEDIUM,
                            color: colors.label,
                        },
                    });
                    y += canvas.line_height(FONT_MEDIUM) + 4;
                }
            }

            // CPU on its own line with bigger number
            Self::push_divider(&mut layout, y, width, margin, colors.divider);
            y += 6;
            layout.push(Widget {
                id: "cpu_label",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width("CPU", FONT_SMALL).max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "CPU".to_string(),
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.label,
                },
            });
            let cpu_val = format!("{:.0}%", data.cpu_percent);
            let cpu_val_w = canvas.text_width(&cpu_val, FONT_TIME);
            layout.push(Widget {
                id: "cpu_val",
                rect: Rect {
                    x: width as i32 - margin - cpu_val_w,
                    y: y - 4,
                    w: cpu_val_w.max(0) as u32,
                    h: canvas.line_height(FONT_TIME).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: cpu_val,
                    x: width as i32 - margin - cpu_val_w,
                    y: y - 4,
                    size: FONT_TIME,
                    color: colors.segment_on,
                },
            });
            y += canvas.line_height(FONT_TIME);

            // RAM on its own line with bigger number
            Self::push_divider(&mut layout, y, width, margin, colors.divider);
            y += 6;
            layout.push(Widget {
                id: "ram_label",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width("RAM", FONT_SMALL).max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "RAM".to_string(),
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.label,
                },
            });
            let ram_val = format!("{:.0}%", data.ram_percent);
            let ram_val_w = canvas.text_width(&ram_val, FONT_TIME);
            layout.push(Widget {
                id: "ram_val",
                rect: Rect {
                    x: width as i32 - margin - ram_val_w,
                    y: y - 4,
                    w: ram_val_w.max(0) as u32,
                    h: canvas.line_height(FONT_TIME).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: ram_val,
                    x: width as i32 - margin - ram_val_w,
                    y: y - 4,
                    size: FONT_TIME,
                    color: colors.segment_on,
                },
            });
            y += canvas.line_height(FONT_TIME);

            // Complication: Disk I/O
            if is_on(complication_names::DISK_IO) {
                Self::push_divider(&mut layout, y, width, margin, colors.divider);
                y += 6;
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                // DSK R label + value
                layout.push(Widget {
                    id: "dsk_r_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("DSK R", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "DSK R".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "dsk_r_val",
                    rect: Rect {
                        x: margin,
                        y: y + 10,
                        w: canvas.text_width(&disk_r, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_r,
                        x: margin,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
                // DSK W label + value
                let dsk_w_x = margin + col_width + margin;
                layout.push(Widget {
                    id: "dsk_w_label",
                    rect: Rect {
                        x: dsk_w_x,
                        y,
                        w: canvas.text_width("DSK W", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "DSK W".to_string(),
                        x: dsk_w_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "dsk_w_val",
                    rect: Rect {
                        x: dsk_w_x,
                        y: y + 10,
                        w: canvas.text_width(&disk_w, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_w,
                        x: dsk_w_x,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
                y += 32;
            }

            // Complication: Network
            if is_on(complication_names::NETWORK) {
                Self::push_divider(&mut layout, y, width, margin, colors.divider);
                y += 6;
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                // NET ↓ label + value
                layout.push(Widget {
                    id: "net_rx_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("NET \u{2193}", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "NET \u{2193}".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "net_rx_val",
                    rect: Rect {
                        x: margin,
                        y: y + 10,
                        w: canvas.text_width(&net_rx, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_rx,
                        x: margin,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
                // NET ↑ label + value
                let net_tx_x = margin + col_width + margin;
                layout.push(Widget {
                    id: "net_tx_label",
                    rect: Rect {
                        x: net_tx_x,
                        y,
                        w: canvas.text_width("NET \u{2191}", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "NET \u{2191}".to_string(),
                        x: net_tx_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "net_tx_val",
                    rect: Rect {
                        x: net_tx_x,
                        y: y + 10,
                        w: canvas.text_width(&net_tx, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_tx,
                        x: net_tx_x,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
                y += 32;
            }

            // Uptime and IP at bottom
            Self::push_divider(&mut layout, y, width, margin, colors.divider);
            y += 6;

            // Uptime (always shown)
            let uptime_str = format!("UP {}", data.uptime);
            layout.push(Widget {
                id: "uptime",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&uptime_str, FONT_SMALL).max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content: WidgetContent::Text {
                    text: uptime_str,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.label,
                },
            });
            y += canvas.line_height(FONT_SMALL) + 6;

            // Complication: IP address
            if is_on(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    layout.push(Widget {
                        id: "ip_label",
                        rect: Rect {
                            x: margin,
                            y,
                            w: canvas.text_width("IP:", FONT_SMALL).max(0) as u32,
                            h: canvas.line_height(FONT_SMALL).max(0) as u32,
                        },
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: "IP:".to_string(),
                            x: margin,
                            y,
                            size: FONT_SMALL,
                            color: colors.label,
                        },
                    });
                    y += canvas.line_height(FONT_SMALL);
                    layout.push(Widget {
                        id: "ip_addr",
                        rect: Rect {
                            x: margin,
                            y,
                            w: canvas.text_width(ip, FONT_SMALL).max(0) as u32,
                            h: canvas.line_height(FONT_SMALL).max(0) as u32,
                        },
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: ip.clone(),
                            x: margin,
                            y,
                            size: FONT_SMALL,
                            color: colors.label,
                        },
                    });
                }
            }
        } else {
            // Landscape layout - larger metrics to fill space
            let col_width = (width as i32 - margin * 5) / 4;

            // Row 1: Hostname on left, Time on right
            layout.push(Widget {
                id: "hostname",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&data.hostname, FONT_MEDIUM).max(0) as u32,
                    h: canvas.line_height(FONT_MEDIUM).max(0) as u32,
                },
                kind: ZoneKind::Static,
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: data.hostname.clone(),
                    x: margin,
                    y,
                    size: FONT_MEDIUM,
                    color: colors.label,
                },
            });
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    let clock_radius = 12_u32;
                    let clock_cx = width as i32 - margin - clock_radius as i32;
                    let clock_cy = y + clock_radius as i32;
                    for (i, draw) in mini_analog_clock_draws(
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.segment_on,
                        colors.segment_on,
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
                    let time_x = width as i32 - margin - time_width;
                    layout.push(Widget {
                        id: "time",
                        rect: Rect {
                            x: time_x,
                            y,
                            w: time_width.max(0) as u32,
                            h: canvas.line_height(FONT_LARGE).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: time_str,
                            x: time_x,
                            y,
                            size: FONT_LARGE,
                            color: colors.segment_on,
                        },
                    });
                }
            }
            y += canvas.line_height(FONT_LARGE);

            // Row 2: Uptime on left, Date on right (below time)
            let uptime_text = format!("Up: {}", data.uptime);
            layout.push(Widget {
                id: "uptime",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width(&uptime_text, FONT_SMALL).max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content: WidgetContent::Text {
                    text: uptime_text,
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.label,
                },
            });
            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_SMALL);
                    let date_x = width as i32 - margin - date_width;
                    layout.push(Widget {
                        id: "date",
                        rect: Rect {
                            x: date_x,
                            y,
                            w: date_width.max(0) as u32,
                            h: canvas.line_height(FONT_SMALL).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: date_str,
                            x: date_x,
                            y,
                            size: FONT_SMALL,
                            color: colors.label,
                        },
                    });
                }
            }
            y += canvas.line_height(FONT_SMALL) + 2;

            // Row 3: IP address on left with label
            if is_on(complication_names::IP_ADDRESS) {
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
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: ip_text,
                            x: margin,
                            y,
                            size: FONT_SMALL,
                            color: colors.label,
                        },
                    });
                }
            }
            y += canvas.line_height(FONT_SMALL) + 4;

            Self::push_divider(&mut layout, y, width, margin, colors.divider);
            y += 6;

            // Row 1: CPU (base), RAM (base), Temp (complication) - medium text
            // CPU label + value (draw_segment_value_medium inlined)
            layout.push(Widget {
                id: "cpu_label",
                rect: Rect {
                    x: margin,
                    y,
                    w: canvas.text_width("CPU", FONT_SMALL).max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "CPU".to_string(),
                    x: margin,
                    y,
                    size: FONT_SMALL,
                    color: colors.label,
                },
            });
            let cpu_val = format!("{:.0}%", data.cpu_percent);
            layout.push(Widget {
                id: "cpu_val",
                rect: Rect {
                    x: margin,
                    y: y + 12,
                    w: canvas.text_width(&cpu_val, 26.0).max(0) as u32,
                    h: canvas.line_height(26.0).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: cpu_val,
                    x: margin,
                    y: y + 12,
                    size: 26.0,
                    color: colors.segment_on,
                },
            });
            // RAM label + value
            let ram_x = margin + col_width + margin;
            layout.push(Widget {
                id: "ram_label",
                rect: Rect {
                    x: ram_x,
                    y,
                    w: canvas.text_width("RAM", FONT_SMALL).max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "RAM".to_string(),
                    x: ram_x,
                    y,
                    size: FONT_SMALL,
                    color: colors.label,
                },
            });
            let ram_val = format!("{:.0}%", data.ram_percent);
            layout.push(Widget {
                id: "ram_val",
                rect: Rect {
                    x: ram_x,
                    y: y + 12,
                    w: canvas.text_width(&ram_val, 26.0).max(0) as u32,
                    h: canvas.line_height(26.0).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: ram_val,
                    x: ram_x,
                    y: y + 12,
                    size: 26.0,
                    color: colors.segment_on,
                },
            });
            // Complication: CPU temperature
            if is_on(complication_names::CPU_TEMP) {
                if let Some(temp) = data.cpu_temp {
                    let temp_x = margin + (col_width + margin) * 2;
                    layout.push(Widget {
                        id: "temp_label",
                        rect: Rect {
                            x: temp_x,
                            y,
                            w: canvas.text_width("TEMP", FONT_SMALL).max(0) as u32,
                            h: canvas.line_height(FONT_SMALL).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::EveryFrame,
                        content: WidgetContent::Text {
                            text: "TEMP".to_string(),
                            x: temp_x,
                            y,
                            size: FONT_SMALL,
                            color: colors.label,
                        },
                    });
                    let temp_val = format!("{:.0}°", temp);
                    layout.push(Widget {
                        id: "temp_val",
                        rect: Rect {
                            x: temp_x,
                            y: y + 12,
                            w: canvas.text_width(&temp_val, 26.0).max(0) as u32,
                            h: canvas.line_height(26.0).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::EveryFrame,
                        content: WidgetContent::Text {
                            text: temp_val,
                            x: temp_x,
                            y: y + 12,
                            size: 26.0,
                            color: colors.segment_on,
                        },
                    });
                }
            }
            y += 42;

            Self::push_divider(&mut layout, y, width, margin, colors.divider);
            y += 6;

            // Row 2: Disk R, Disk W (complication), Net Down, Net Up (complication) - smaller text
            if is_on(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                // DSK R label + value (draw_segment_value inlined)
                layout.push(Widget {
                    id: "dsk_r_label",
                    rect: Rect {
                        x: margin,
                        y,
                        w: canvas.text_width("DSK R", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "DSK R".to_string(),
                        x: margin,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "dsk_r_val",
                    rect: Rect {
                        x: margin,
                        y: y + 10,
                        w: canvas.text_width(&disk_r, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_r,
                        x: margin,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
                // DSK W label + value
                let dsk_w_x = margin + col_width + margin;
                layout.push(Widget {
                    id: "dsk_w_label",
                    rect: Rect {
                        x: dsk_w_x,
                        y,
                        w: canvas.text_width("DSK W", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "DSK W".to_string(),
                        x: dsk_w_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "dsk_w_val",
                    rect: Rect {
                        x: dsk_w_x,
                        y: y + 10,
                        w: canvas.text_width(&disk_w, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_w,
                        x: dsk_w_x,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
            }
            if is_on(complication_names::NETWORK) {
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                // NET ↓ label + value
                let net_rx_x = margin + (col_width + margin) * 2;
                layout.push(Widget {
                    id: "net_rx_label",
                    rect: Rect {
                        x: net_rx_x,
                        y,
                        w: canvas.text_width("NET \u{2193}", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "NET \u{2193}".to_string(),
                        x: net_rx_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "net_rx_val",
                    rect: Rect {
                        x: net_rx_x,
                        y: y + 10,
                        w: canvas.text_width(&net_rx, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_rx,
                        x: net_rx_x,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
                // NET ↑ label + value
                let net_tx_x = margin + (col_width + margin) * 3;
                layout.push(Widget {
                    id: "net_tx_label",
                    rect: Rect {
                        x: net_tx_x,
                        y,
                        w: canvas.text_width("NET \u{2191}", FONT_SMALL).max(0) as u32,
                        h: canvas.line_height(FONT_SMALL).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: "NET \u{2191}".to_string(),
                        x: net_tx_x,
                        y,
                        size: FONT_SMALL,
                        color: colors.label,
                    },
                });
                layout.push(Widget {
                    id: "net_tx_val",
                    rect: Rect {
                        x: net_tx_x,
                        y: y + 10,
                        w: canvas.text_width(&net_tx, FONT_LARGE).max(0) as u32,
                        h: canvas.line_height(FONT_LARGE).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_tx,
                        x: net_tx_x,
                        y: y + 10,
                        size: FONT_LARGE,
                        color: colors.segment_on,
                    },
                });
            }
        }
        let _ = y;
        Some(layout)
    }
}

impl Default for DigitsFace {
    fn default() -> Self {
        Self::new()
    }
}

impl Face for DigitsFace {
    fn name(&self) -> &str {
        "digits"
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
        comp: &EnabledComplications,
    ) {
        let colors = FaceColors::from_theme(theme);
        let (width, _height) = canvas.dimensions();
        let portrait = width < 200;
        let margin = 6;
        let mut y = margin;

        let is_on = |id: &str| comp.is_enabled(self.name(), id, true);

        // Get time format option
        let time_format = comp
            .get_option(
                self.name(),
                complication_names::TIME,
                complication_options::TIME_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(time_formats::DIGITAL_24H);

        // Get date format option
        let date_format = comp
            .get_option(
                self.name(),
                complication_names::DATE,
                complication_options::DATE_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(date_formats::ISO);

        if portrait {
            // Portrait layout
            let col_width = (width as i32 - margin * 3) / 2;

            // Hostname at top (always shown)
            let host_width = canvas.text_width(&data.hostname, FONT_MEDIUM);
            let host_x = (width as i32 - host_width) / 2;
            canvas.draw_text(host_x, y, &data.hostname, FONT_MEDIUM, colors.label);
            y += canvas.line_height(FONT_MEDIUM) + 2;

            // Complication: Time
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock
                    let clock_radius = 18_u32;
                    let clock_cx = width as i32 / 2;
                    let clock_cy = y + clock_radius as i32 + 2;
                    draw_mini_analog_clock(
                        canvas,
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.segment_on,
                        colors.segment_on,
                    );
                    y += (clock_radius * 2) as i32 + 6;
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_TIME);
                    let time_x = (width as i32 - time_width) / 2;
                    canvas.draw_text(time_x, y, &time_str, FONT_TIME, colors.segment_on);
                    y += canvas.line_height(FONT_TIME) + 2;
                }
            }

            // Complication: Date (centered, if not hidden)
            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_MEDIUM);
                    let date_x = (width as i32 - date_width) / 2;
                    canvas.draw_text(date_x, y, &date_str, FONT_MEDIUM, colors.label);
                    y += canvas.line_height(FONT_MEDIUM) + 4;
                }
            }

            // CPU on its own line with bigger number
            Self::draw_divider(canvas, y, width, margin, colors.divider);
            y += 6;
            canvas.draw_text(margin, y, "CPU", FONT_SMALL, colors.label);
            let cpu_val = format!("{:.0}%", data.cpu_percent);
            let cpu_val_w = canvas.text_width(&cpu_val, FONT_TIME);
            canvas.draw_text(
                width as i32 - margin - cpu_val_w,
                y - 4,
                &cpu_val,
                FONT_TIME,
                colors.segment_on,
            );
            y += canvas.line_height(FONT_TIME);

            // RAM on its own line with bigger number
            Self::draw_divider(canvas, y, width, margin, colors.divider);
            y += 6;
            canvas.draw_text(margin, y, "RAM", FONT_SMALL, colors.label);
            let ram_val = format!("{:.0}%", data.ram_percent);
            let ram_val_w = canvas.text_width(&ram_val, FONT_TIME);
            canvas.draw_text(
                width as i32 - margin - ram_val_w,
                y - 4,
                &ram_val,
                FONT_TIME,
                colors.segment_on,
            );
            y += canvas.line_height(FONT_TIME);

            // Complication: Disk I/O
            if is_on(complication_names::DISK_IO) {
                Self::draw_divider(canvas, y, width, margin, colors.divider);
                y += 6;
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                Self::draw_segment_value(
                    canvas,
                    margin,
                    y,
                    "DSK R",
                    &disk_r,
                    colors.label,
                    colors.segment_on,
                );
                Self::draw_segment_value(
                    canvas,
                    margin + col_width + margin,
                    y,
                    "DSK W",
                    &disk_w,
                    colors.label,
                    colors.segment_on,
                );
                y += 32;
            }

            // Complication: Network
            if is_on(complication_names::NETWORK) {
                Self::draw_divider(canvas, y, width, margin, colors.divider);
                y += 6;
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                Self::draw_segment_value(
                    canvas,
                    margin,
                    y,
                    "NET \u{2193}",
                    &net_rx,
                    colors.label,
                    colors.segment_on,
                );
                Self::draw_segment_value(
                    canvas,
                    margin + col_width + margin,
                    y,
                    "NET \u{2191}",
                    &net_tx,
                    colors.label,
                    colors.segment_on,
                );
                y += 32;
            }

            // Uptime and IP at bottom (where hostname used to be)
            Self::draw_divider(canvas, y, width, margin, colors.divider);
            y += 6;

            // Uptime (always shown)
            canvas.draw_text(
                margin,
                y,
                &format!("UP {}", data.uptime),
                FONT_SMALL,
                colors.label,
            );
            y += canvas.line_height(FONT_SMALL) + 6;

            // Complication: IP address
            if is_on(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    // IP label
                    canvas.draw_text(margin, y, "IP:", FONT_SMALL, colors.label);
                    y += canvas.line_height(FONT_SMALL);
                    // IP address on next line, smaller font to fit
                    canvas.draw_text(margin, y, ip, FONT_SMALL, colors.label);
                }
            }
        } else {
            // Landscape layout - larger metrics to fill space
            let col_width = (width as i32 - margin * 5) / 4;

            // Row 1: Hostname on left, Time on right
            canvas.draw_text(margin, y, &data.hostname, FONT_MEDIUM, colors.label);
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock on the right
                    let clock_radius = 12_u32;
                    let clock_cx = width as i32 - margin - clock_radius as i32;
                    let clock_cy = y + clock_radius as i32;
                    draw_mini_analog_clock(
                        canvas,
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.segment_on,
                        colors.segment_on,
                    );
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_LARGE);
                    canvas.draw_text(
                        width as i32 - margin - time_width,
                        y,
                        &time_str,
                        FONT_LARGE,
                        colors.segment_on,
                    );
                }
            }
            y += canvas.line_height(FONT_LARGE);

            // Row 2: Uptime on left, Date on right (below time)
            let uptime_text = format!("Up: {}", data.uptime);
            canvas.draw_text(margin, y, &uptime_text, FONT_SMALL, colors.label);
            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_SMALL);
                    canvas.draw_text(
                        width as i32 - margin - date_width,
                        y,
                        &date_str,
                        FONT_SMALL,
                        colors.label,
                    );
                }
            }
            y += canvas.line_height(FONT_SMALL) + 2;

            // Row 3: IP address on left with label
            if is_on(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    let ip_text = format!("IP: {}", ip);
                    canvas.draw_text(margin, y, &ip_text, FONT_SMALL, colors.label);
                }
            }
            y += canvas.line_height(FONT_SMALL) + 4;

            Self::draw_divider(canvas, y, width, margin, colors.divider);
            y += 6;

            // Row 1: CPU (base), RAM (base), Temp (complication) - medium text
            Self::draw_segment_value_medium(
                canvas,
                margin,
                y,
                "CPU",
                &format!("{:.0}%", data.cpu_percent),
                colors.label,
                colors.segment_on,
            );
            Self::draw_segment_value_medium(
                canvas,
                margin + col_width + margin,
                y,
                "RAM",
                &format!("{:.0}%", data.ram_percent),
                colors.label,
                colors.segment_on,
            );
            // Complication: CPU temperature
            if is_on(complication_names::CPU_TEMP) {
                if let Some(temp) = data.cpu_temp {
                    Self::draw_segment_value_medium(
                        canvas,
                        margin + (col_width + margin) * 2,
                        y,
                        "TEMP",
                        &format!("{:.0}°", temp),
                        colors.label,
                        colors.segment_on,
                    );
                }
            }
            y += 42;

            Self::draw_divider(canvas, y, width, margin, colors.divider);
            y += 6;

            // Row 2: Disk R, Disk W (complication), Net Down, Net Up (complication) - smaller text
            if is_on(complication_names::DISK_IO) {
                let disk_r = SystemData::format_rate_compact(data.disk_read_rate);
                let disk_w = SystemData::format_rate_compact(data.disk_write_rate);
                Self::draw_segment_value(
                    canvas,
                    margin,
                    y,
                    "DSK R",
                    &disk_r,
                    colors.label,
                    colors.segment_on,
                );
                Self::draw_segment_value(
                    canvas,
                    margin + col_width + margin,
                    y,
                    "DSK W",
                    &disk_w,
                    colors.label,
                    colors.segment_on,
                );
            }
            if is_on(complication_names::NETWORK) {
                let net_rx = SystemData::format_rate_compact(data.net_rx_rate);
                let net_tx = SystemData::format_rate_compact(data.net_tx_rate);
                Self::draw_segment_value(
                    canvas,
                    margin + (col_width + margin) * 2,
                    y,
                    "NET \u{2193}",
                    &net_rx,
                    colors.label,
                    colors.segment_on,
                );
                Self::draw_segment_value(
                    canvas,
                    margin + (col_width + margin) * 3,
                    y,
                    "NET \u{2191}",
                    &net_tx,
                    colors.label,
                    colors.segment_on,
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
        comp: &EnabledComplications,
    ) -> Option<Layout> {
        self.build_layout(canvas, data, theme, comp)
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

    #[allow(clippy::field_reassign_with_default)]
    fn sample() -> SystemData {
        let mut d = SystemData::default();
        d.hostname = "node01".into();
        d.uptime = "3d 8h".into();
        d.cpu_percent = 55.0;
        d.ram_percent = 72.0;
        d.cpu_temp = Some(52.0);
        d.hour = 14;
        d.minute = 30;
        d.disk_read_rate = 1024.0 * 1024.0 * 5.0;
        d.disk_write_rate = 1024.0 * 1024.0 * 2.0;
        d.net_rx_rate = 1024.0 * 1024.0 * 1.5;
        d.net_tx_rate = 1024.0 * 512.0;
        d
    }

    #[allow(clippy::field_reassign_with_default)]
    fn sample_with_ip(ip: &str) -> SystemData {
        let mut d = sample();
        d.display_ip = Some(ip.to_string());
        d
    }

    fn analogue_comps() -> EnabledComplications {
        let mut comps = EnabledComplications::new();
        comps.set_enabled("digits", complication_names::TIME, true);
        comps.set_option(
            "digits",
            complication_names::TIME,
            complication_options::TIME_FORMAT,
            time_formats::ANALOGUE.to_string(),
        );
        comps
    }

    fn render_both_comps(
        width: u32,
        height: u32,
        data: SystemData,
        comps: EnabledComplications,
    ) -> (Vec<u8>, Vec<u8>) {
        let face = DigitsFace::new();
        let theme = Theme::from_preset("default");

        let mut legacy = Canvas::new(width, height);
        legacy.set_background(0);
        legacy.clear();
        face.render(&mut legacy, &data, &theme, &comps);

        let mut via_layout = Canvas::new(width, height);
        let lay = face
            .layout(&via_layout, &data, &theme, &comps)
            .expect("layout() must return Some");
        via_layout.set_background(0);
        via_layout.clear();
        render_layout(&mut via_layout, &lay);

        (legacy.pixels().to_vec(), via_layout.pixels().to_vec())
    }

    fn render_both(width: u32, height: u32) -> (Vec<u8>, Vec<u8>) {
        render_both_comps(width, height, sample(), EnabledComplications::new())
    }

    fn render_both_with(width: u32, height: u32, data: SystemData) -> (Vec<u8>, Vec<u8>) {
        render_both_comps(width, height, data, EnabledComplications::new())
    }

    #[test]
    fn digits_layout_matches_render_landscape() {
        let (legacy, via_layout) = render_both(320, 170);
        assert_eq!(
            legacy, via_layout,
            "digits landscape mismatch (default comps)"
        );
    }

    #[test]
    fn digits_layout_matches_render_landscape_with_ip() {
        let (legacy, via_layout) = render_both_with(320, 170, sample_with_ip("192.168.1.100"));
        assert_eq!(legacy, via_layout, "digits landscape mismatch (IPv4)");
    }

    #[test]
    fn digits_layout_matches_render_portrait() {
        let (legacy, via_layout) = render_both(170, 320);
        assert_eq!(
            legacy, via_layout,
            "digits portrait mismatch (default comps)"
        );
    }

    #[test]
    fn digits_layout_matches_render_portrait_with_ip() {
        let (legacy, via_layout) = render_both_with(170, 320, sample_with_ip("192.168.1.100"));
        assert_eq!(legacy, via_layout, "digits portrait mismatch (IPv4)");
    }

    #[test]
    fn digits_layout_matches_render_landscape_analogue() {
        let (legacy, via_layout) = render_both_comps(320, 170, sample(), analogue_comps());
        assert_eq!(legacy, via_layout, "digits landscape analogue mismatch");
    }

    #[test]
    fn digits_layout_matches_render_portrait_analogue() {
        let (legacy, via_layout) = render_both_comps(170, 320, sample(), analogue_comps());
        assert_eq!(legacy, via_layout, "digits portrait analogue mismatch");
    }

    /// Portrait + IPv6 (compressed): pixel-identical. Realistic addresses (<=~27 chars at
    /// FONT_SMALL) fit within the 170px portrait width. NOTE: a fully-expanded 39-char IPv6
    /// overflows `draw_text` (pre-existing in render(); digits has no IP-wrap branch) — both
    /// paths overflow identically, so the migration is faithful; wrapping is a separate feature.
    #[test]
    fn digits_layout_matches_render_portrait_ipv6() {
        let (legacy, via_layout) =
            render_both_with(170, 320, sample_with_ip("2001:db8::dead:beef:1:2"));
        assert_eq!(legacy, via_layout, "digits portrait IPv6 mismatch");
    }
}
