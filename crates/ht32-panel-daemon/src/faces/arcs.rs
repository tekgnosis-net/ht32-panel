//! Arcs face using only circles and arcs for data display.
//!
//! All metrics are shown as circular arc gauges rather than
//! traditional bars or graphs.

use std::f32::consts::PI;

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

/// Derive colors from theme for the arcs face.
struct FaceColors {
    /// Primary arc color (CPU)
    primary: u32,
    /// Secondary arc color (RAM)
    secondary: u32,
    /// Arc background (unfilled portion)
    arc_bg: u32,
    /// Text color
    text: u32,
    /// Dimmed text color
    dim: u32,
}

impl FaceColors {
    fn from_theme(theme: &Theme) -> Self {
        Self {
            primary: theme.primary,
            secondary: theme.secondary,
            arc_bg: dim_color(theme.primary, theme.background, 0.25),
            text: theme.text,
            dim: dim_color(theme.text, theme.background, 0.7), // Higher factor for better contrast
        }
    }
}

/// Font sizes.
const FONT_LARGE: f32 = 18.0;
const FONT_NORMAL: f32 = 14.0;
const FONT_SMALL: f32 = 12.0;
const FONT_TINY: f32 = 11.0;

/// Formats a byte rate compactly with max 4 characters (e.g., "1.2M", "12M", "999K").
fn format_rate_short(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_000_000_000.0 {
        let val = bytes_per_sec / 1_000_000_000.0;
        if val >= 10.0 {
            format!("{:.0}G", val)
        } else {
            format!("{:.1}G", val)
        }
    } else if bytes_per_sec >= 1_000_000.0 {
        let val = bytes_per_sec / 1_000_000.0;
        if val >= 10.0 {
            format!("{:.0}M", val)
        } else {
            format!("{:.1}M", val)
        }
    } else if bytes_per_sec >= 1_000.0 {
        let val = bytes_per_sec / 1_000.0;
        if val >= 10.0 {
            format!("{:.0}K", val)
        } else {
            format!("{:.1}K", val)
        }
    } else {
        format!("{:.0}", bytes_per_sec)
    }
}

/// A single arc draw spec: `(cx, cy, r, start_angle, end_angle, stroke, color)`.
///
/// Produced by [`ArcsFace::arc_gauge_specs`] and [`ArcsFace::activity_arc_specs`] so
/// both `render()` (via `canvas.draw_arc`) and `layout()` (via `WidgetContent::Arc`)
/// consume the identical floating-point computation, guaranteeing byte-identical output.
type ArcSpec = (i32, i32, u32, f32, f32, f32, u32);

/// A face using circular arc gauges for all metrics.
pub struct ArcsFace;

impl ArcsFace {
    /// Creates a new arcs face.
    pub fn new() -> Self {
        Self
    }

    /// Computes the draw specs for a circular arc gauge without touching a canvas.
    ///
    /// Returns the background arc followed by the foreground fill arc (if percent > 0).
    /// The gauge spans 135° → 405° (270° sweep, starting bottom-left).
    #[allow(clippy::too_many_arguments)]
    fn arc_gauge_specs(
        cx: i32,
        cy: i32,
        radius: u32,
        stroke_width: f32,
        percent: f64,
        fg_color: u32,
        bg_color: u32,
    ) -> Vec<ArcSpec> {
        let start_angle = 135.0 * PI / 180.0;
        let end_angle = 405.0 * PI / 180.0;
        let sweep = end_angle - start_angle;

        let mut specs = vec![(
            cx,
            cy,
            radius,
            start_angle,
            end_angle,
            stroke_width,
            bg_color,
        )];

        if percent > 0.0 {
            let fill_angle = start_angle + sweep * (percent.min(100.0) / 100.0) as f32;
            specs.push((
                cx,
                cy,
                radius,
                start_angle,
                fill_angle,
                stroke_width,
                fg_color,
            ));
        }

        specs
    }

    /// Computes the draw specs for a small activity indicator arc without touching a canvas.
    ///
    /// Uses logarithmic scaling for better visualization of varying rates.
    /// Returns the background arc followed by the foreground fill arc (if value > 0 and max > 0).
    #[allow(clippy::too_many_arguments)]
    fn activity_arc_specs(
        cx: i32,
        cy: i32,
        radius: u32,
        stroke_width: f32,
        value: f64,
        max_value: f64,
        fg_color: u32,
        bg_color: u32,
    ) -> Vec<ArcSpec> {
        let start_angle = 135.0 * PI / 180.0;
        let end_angle = 405.0 * PI / 180.0;
        let sweep = end_angle - start_angle;

        let mut specs = vec![(
            cx,
            cy,
            radius,
            start_angle,
            end_angle,
            stroke_width,
            bg_color,
        )];

        if value > 0.0 && max_value > 0.0 {
            let log_value = (1.0 + value).ln();
            let log_max = (1.0 + max_value).ln();
            let normalized = (log_value / log_max).min(1.0);
            let fill_angle = start_angle + sweep * normalized as f32;
            specs.push((
                cx,
                cy,
                radius,
                start_angle,
                fill_angle,
                stroke_width,
                fg_color,
            ));
        }

        specs
    }

    /// Draws a circular arc gauge using specs from [`Self::arc_gauge_specs`].
    #[allow(clippy::too_many_arguments)]
    fn draw_arc_gauge(
        canvas: &mut Canvas,
        cx: i32,
        cy: i32,
        radius: u32,
        stroke_width: f32,
        percent: f64,
        fg_color: u32,
        bg_color: u32,
    ) {
        for (cx, cy, r, sa, ea, sw, col) in
            Self::arc_gauge_specs(cx, cy, radius, stroke_width, percent, fg_color, bg_color)
        {
            canvas.draw_arc(cx, cy, r, sa, ea, sw, col);
        }
    }

    /// Draws a small activity indicator arc using specs from [`Self::activity_arc_specs`].
    #[allow(clippy::too_many_arguments)]
    fn draw_activity_arc(
        canvas: &mut Canvas,
        cx: i32,
        cy: i32,
        radius: u32,
        stroke_width: f32,
        value: f64,
        max_value: f64,
        fg_color: u32,
        bg_color: u32,
    ) {
        for (cx, cy, r, sa, ea, sw, col) in Self::activity_arc_specs(
            cx,
            cy,
            radius,
            stroke_width,
            value,
            max_value,
            fg_color,
            bg_color,
        ) {
            canvas.draw_arc(cx, cy, r, sa, ea, sw, col);
        }
    }

    /// Pushes arc specs as `WidgetContent::Arc` widgets into the layout.
    ///
    /// The `gauge_id` selects stable static widget IDs for the background (`_bg`) and
    /// foreground (`_fg`) arcs.  The rect uses the gauge radius as bounding box.
    fn push_arc_specs(
        layout: &mut Layout,
        gauge_id: &'static str,
        specs: Vec<ArcSpec>,
        radius: u32,
    ) {
        for (i, (cx, cy, r, start_angle, end_angle, stroke, color)) in specs.into_iter().enumerate()
        {
            let widget_id: &'static str = if i == 0 {
                match gauge_id {
                    "cpu_gauge" => "cpu_gauge_bg",
                    "ram_gauge" => "ram_gauge_bg",
                    "disk_r" => "disk_r_bg",
                    "disk_w" => "disk_w_bg",
                    "net_rx" => "net_rx_bg",
                    "net_tx" => "net_tx_bg",
                    _ => "arc_bg",
                }
            } else {
                match gauge_id {
                    "cpu_gauge" => "cpu_gauge_fg",
                    "ram_gauge" => "ram_gauge_fg",
                    "disk_r" => "disk_r_fg",
                    "disk_w" => "disk_w_fg",
                    "net_rx" => "net_rx_fg",
                    "net_tx" => "net_tx_fg",
                    _ => "arc_fg",
                }
            };
            layout.push(Widget {
                id: widget_id,
                rect: Rect {
                    x: cx - radius as i32,
                    y: cy - radius as i32,
                    w: radius * 2,
                    h: radius * 2,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Arc {
                    cx,
                    cy,
                    r,
                    start_angle,
                    end_angle,
                    stroke,
                    color,
                },
            });
        }
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
        let (width, height) = canvas.dimensions();
        let portrait = width < 200;
        let mut layout = Layout::new();

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
            // Portrait layout: CPU, RAM stacked vertically, disk/net on separate rows at bottom
            let margin = 6;
            let center_x = width as i32 / 2;

            // Calculate vertical layout to use full height
            // Reserve space: top text (~30px), bottom text (~36px for 3 lines), remaining for arcs
            let top_text_height = 28_i32;
            let bottom_text_height = 36_i32;
            let available_height =
                height as i32 - margin * 2 - top_text_height - bottom_text_height;

            // Large arcs (CPU, RAM) get 30% each, two small arc rows get 20% each
            let large_arc_height = (available_height * 30) / 100;
            let small_arc_height = (available_height * 20) / 100;

            let large_radius = ((large_arc_height - 8) / 2).min(34) as u32;
            let large_stroke = 6.0;
            let small_radius = ((small_arc_height - 4) / 2).min(18) as u32;
            let small_stroke = 3.5;

            let mut y = margin;

            // Top section: Time and date
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock centered
                    let clock_radius = 10_u32;
                    let clock_cx = width as i32 / 2;
                    let clock_cy = y + clock_radius as i32;
                    for (i, draw) in mini_analog_clock_draws(
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.primary,
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
                    y += (clock_radius * 2) as i32 + 2;
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_NORMAL);
                    let time_x = (width as i32 - time_width) / 2;
                    layout.push(Widget {
                        id: "time",
                        rect: Rect {
                            x: time_x,
                            y,
                            w: time_width.max(0) as u32,
                            h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: time_str,
                            x: time_x,
                            y,
                            size: FONT_NORMAL,
                            color: colors.text,
                        },
                    });
                    y += canvas.line_height(FONT_NORMAL);
                }
            }

            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_TINY);
                    let date_x = (width as i32 - date_width) / 2;
                    layout.push(Widget {
                        id: "date",
                        rect: Rect {
                            x: date_x,
                            y,
                            w: date_width.max(0) as u32,
                            h: canvas.line_height(FONT_TINY).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: date_str,
                            x: date_x,
                            y,
                            size: FONT_TINY,
                            color: colors.dim,
                        },
                    });
                }
            }
            y = margin + top_text_height;

            // CPU arc (centered)
            let cpu_cy = y + large_radius as i32 + 2;
            Self::push_arc_specs(
                &mut layout,
                "cpu_gauge",
                Self::arc_gauge_specs(
                    center_x,
                    cpu_cy,
                    large_radius,
                    large_stroke,
                    data.cpu_percent,
                    colors.primary,
                    colors.arc_bg,
                ),
                large_radius,
            );
            layout.push(Widget {
                id: "cpu_label",
                rect: Rect {
                    x: center_x - 10,
                    y: cpu_cy - 6,
                    w: canvas.text_width("CPU", FONT_TINY).max(0) as u32,
                    h: canvas.line_height(FONT_TINY).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "CPU".to_string(),
                    x: center_x - 10,
                    y: cpu_cy - 6,
                    size: FONT_TINY,
                    color: colors.dim,
                },
            });
            let cpu_text = format!("{:.0}%", data.cpu_percent);
            let cpu_w = canvas.text_width(&cpu_text, FONT_SMALL);
            layout.push(Widget {
                id: "cpu_val",
                rect: Rect {
                    x: center_x - cpu_w / 2,
                    y: cpu_cy + 4,
                    w: cpu_w.max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: cpu_text,
                    x: center_x - cpu_w / 2,
                    y: cpu_cy + 4,
                    size: FONT_SMALL,
                    color: colors.text,
                },
            });
            y += large_arc_height;

            // RAM arc (centered)
            let ram_cy = y + large_radius as i32 + 2;
            Self::push_arc_specs(
                &mut layout,
                "ram_gauge",
                Self::arc_gauge_specs(
                    center_x,
                    ram_cy,
                    large_radius,
                    large_stroke,
                    data.ram_percent,
                    colors.secondary,
                    colors.arc_bg,
                ),
                large_radius,
            );
            layout.push(Widget {
                id: "ram_label",
                rect: Rect {
                    x: center_x - 12,
                    y: ram_cy - 6,
                    w: canvas.text_width("RAM", FONT_TINY).max(0) as u32,
                    h: canvas.line_height(FONT_TINY).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "RAM".to_string(),
                    x: center_x - 12,
                    y: ram_cy - 6,
                    size: FONT_TINY,
                    color: colors.dim,
                },
            });
            let ram_text = format!("{:.0}%", data.ram_percent);
            let ram_w = canvas.text_width(&ram_text, FONT_SMALL);
            layout.push(Widget {
                id: "ram_val",
                rect: Rect {
                    x: center_x - ram_w / 2,
                    y: ram_cy + 4,
                    w: ram_w.max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: ram_text,
                    x: center_x - ram_w / 2,
                    y: ram_cy + 4,
                    size: FONT_SMALL,
                    color: colors.text,
                },
            });
            y += large_arc_height;

            let io_max = 100_000_000.0;

            // Disk row: Read and Write arcs centered
            if is_on(complication_names::DISK_IO) {
                let disk_cy = y + small_radius as i32 + 2;
                let disk_r_cx = center_x - small_radius as i32 - 6;
                Self::push_arc_specs(
                    &mut layout,
                    "disk_r",
                    Self::activity_arc_specs(
                        disk_r_cx,
                        disk_cy,
                        small_radius,
                        small_stroke,
                        data.disk_read_rate,
                        io_max,
                        colors.primary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                let disk_r_text = format_rate_short(data.disk_read_rate);
                let disk_r_w = canvas.text_width(&disk_r_text, FONT_TINY);
                layout.push(Widget {
                    id: "disk_r_val",
                    rect: Rect {
                        x: disk_r_cx - disk_r_w / 2,
                        y: disk_cy - 4,
                        w: disk_r_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_r_text,
                        x: disk_r_cx - disk_r_w / 2,
                        y: disk_cy - 4,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                layout.push(Widget {
                    id: "disk_r_lbl",
                    rect: Rect {
                        x: disk_r_cx - 3,
                        y: disk_cy + small_radius as i32 / 2,
                        w: canvas.text_width("R", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "R".to_string(),
                        x: disk_r_cx - 3,
                        y: disk_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });

                let disk_w_cx = center_x + small_radius as i32 + 6;
                Self::push_arc_specs(
                    &mut layout,
                    "disk_w",
                    Self::activity_arc_specs(
                        disk_w_cx,
                        disk_cy,
                        small_radius,
                        small_stroke,
                        data.disk_write_rate,
                        io_max,
                        colors.primary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                let disk_w_text = format_rate_short(data.disk_write_rate);
                let disk_w_w = canvas.text_width(&disk_w_text, FONT_TINY);
                layout.push(Widget {
                    id: "disk_w_val",
                    rect: Rect {
                        x: disk_w_cx - disk_w_w / 2,
                        y: disk_cy - 4,
                        w: disk_w_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_w_text,
                        x: disk_w_cx - disk_w_w / 2,
                        y: disk_cy - 4,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                layout.push(Widget {
                    id: "disk_w_lbl",
                    rect: Rect {
                        x: disk_w_cx - 4,
                        y: disk_cy + small_radius as i32 / 2,
                        w: canvas.text_width("W", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "W".to_string(),
                        x: disk_w_cx - 4,
                        y: disk_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });
                y += small_arc_height;
            }

            // Network row: RX and TX arcs centered
            if is_on(complication_names::NETWORK) {
                let net_cy = y + small_radius as i32 + 2;
                let net_rx_cx = center_x - small_radius as i32 - 6;
                Self::push_arc_specs(
                    &mut layout,
                    "net_rx",
                    Self::activity_arc_specs(
                        net_rx_cx,
                        net_cy,
                        small_radius,
                        small_stroke,
                        data.net_rx_rate,
                        io_max,
                        colors.secondary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                let net_rx_text = format_rate_short(data.net_rx_rate);
                let net_rx_w = canvas.text_width(&net_rx_text, FONT_TINY);
                layout.push(Widget {
                    id: "net_rx_val",
                    rect: Rect {
                        x: net_rx_cx - net_rx_w / 2,
                        y: net_cy - 4,
                        w: net_rx_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_rx_text,
                        x: net_rx_cx - net_rx_w / 2,
                        y: net_cy - 4,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                layout.push(Widget {
                    id: "net_rx_lbl",
                    rect: Rect {
                        x: net_rx_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        w: canvas.text_width("\u{2193}", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "\u{2193}".to_string(),
                        x: net_rx_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });

                let net_tx_cx = center_x + small_radius as i32 + 6;
                Self::push_arc_specs(
                    &mut layout,
                    "net_tx",
                    Self::activity_arc_specs(
                        net_tx_cx,
                        net_cy,
                        small_radius,
                        small_stroke,
                        data.net_tx_rate,
                        io_max,
                        colors.secondary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                let net_tx_text = format_rate_short(data.net_tx_rate);
                let net_tx_w = canvas.text_width(&net_tx_text, FONT_TINY);
                layout.push(Widget {
                    id: "net_tx_val",
                    rect: Rect {
                        x: net_tx_cx - net_tx_w / 2,
                        y: net_cy - 4,
                        w: net_tx_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_tx_text,
                        x: net_tx_cx - net_tx_w / 2,
                        y: net_cy - 4,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                layout.push(Widget {
                    id: "net_tx_lbl",
                    rect: Rect {
                        x: net_tx_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        w: canvas.text_width("\u{2191}", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "\u{2191}".to_string(),
                        x: net_tx_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });
            }

            // Bottom text: hostname, uptime, and IP on separate lines
            let bottom_y = height as i32 - margin - 46;

            // Hostname centered on its own line
            let host_width = canvas.text_width(&data.hostname, FONT_TINY);
            let host_x = (width as i32 - host_width) / 2;
            layout.push(Widget {
                id: "hostname",
                rect: Rect {
                    x: host_x,
                    y: bottom_y,
                    w: host_width.max(0) as u32,
                    h: canvas.line_height(FONT_TINY).max(0) as u32,
                },
                kind: ZoneKind::Static,
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: data.hostname.clone(),
                    x: host_x,
                    y: bottom_y,
                    size: FONT_TINY,
                    color: colors.dim,
                },
            });

            // Uptime on its own line
            let uptime_text = format!("Up: {}", data.uptime);
            layout.push(Widget {
                id: "uptime",
                rect: Rect {
                    x: margin,
                    y: bottom_y + 12,
                    w: canvas.text_width(&uptime_text, FONT_TINY).max(0) as u32,
                    h: canvas.line_height(FONT_TINY).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content: WidgetContent::Text {
                    text: uptime_text,
                    x: margin,
                    y: bottom_y + 12,
                    size: FONT_TINY,
                    color: colors.dim,
                },
            });

            // IP on the next line
            if is_on(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    layout.push(Widget {
                        id: "ip_addr",
                        rect: Rect {
                            x: margin,
                            y: bottom_y + 24,
                            w: canvas.text_width(ip, FONT_TINY).max(0) as u32,
                            h: canvas.line_height(FONT_TINY).max(0) as u32,
                        },
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: ip.clone(),
                            x: margin,
                            y: bottom_y + 24,
                            size: FONT_TINY,
                            color: colors.dim,
                        },
                    });
                }
            }
        } else {
            // Landscape layout
            let margin = 10;
            let gauge_radius = 36_u32;
            let stroke = 8.0;
            let small_radius = 22_u32;
            let small_stroke = 5.0;

            let top_y = margin;

            // Complication: Time
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock on the left
                    let clock_radius = 12_u32;
                    let clock_cx = margin + clock_radius as i32;
                    let clock_cy = top_y + clock_radius as i32;
                    for (i, draw) in mini_analog_clock_draws(
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.primary,
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
                    layout.push(Widget {
                        id: "time",
                        rect: Rect {
                            x: margin,
                            y: top_y,
                            w: canvas.text_width(&time_str, FONT_LARGE).max(0) as u32,
                            h: canvas.line_height(FONT_LARGE).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: time_str,
                            x: margin,
                            y: top_y,
                            size: FONT_LARGE,
                            color: colors.text,
                        },
                    });
                }
            }

            // Hostname at top right (always shown)
            let host_width = canvas.text_width(&data.hostname, FONT_SMALL);
            layout.push(Widget {
                id: "hostname",
                rect: Rect {
                    x: width as i32 - margin - host_width,
                    y: top_y,
                    w: host_width.max(0) as u32,
                    h: canvas.line_height(FONT_SMALL).max(0) as u32,
                },
                kind: ZoneKind::Static,
                cadence: Cadence::OnChange,
                content: WidgetContent::Text {
                    text: data.hostname.clone(),
                    x: width as i32 - margin - host_width,
                    y: top_y,
                    size: FONT_SMALL,
                    color: colors.dim,
                },
            });

            // Complication: Date (below hostname if shown)
            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_TINY);
                    layout.push(Widget {
                        id: "date",
                        rect: Rect {
                            x: width as i32 - margin - date_width,
                            y: top_y + 14,
                            w: date_width.max(0) as u32,
                            h: canvas.line_height(FONT_TINY).max(0) as u32,
                        },
                        kind: ZoneKind::Dynamic,
                        cadence: Cadence::Seconds(60),
                        content: WidgetContent::Text {
                            text: date_str,
                            x: width as i32 - margin - date_width,
                            y: top_y + 14,
                            size: FONT_TINY,
                            color: colors.dim,
                        },
                    });
                }
            }

            let gauge_y = margin + 28 + gauge_radius as i32;
            let cpu_cx = margin + gauge_radius as i32 + 10;

            // Base element: CPU gauge (always shown)
            Self::push_arc_specs(
                &mut layout,
                "cpu_gauge",
                Self::arc_gauge_specs(
                    cpu_cx,
                    gauge_y,
                    gauge_radius,
                    stroke,
                    data.cpu_percent,
                    colors.primary,
                    colors.arc_bg,
                ),
                gauge_radius,
            );
            layout.push(Widget {
                id: "cpu_label",
                rect: Rect {
                    x: cpu_cx - 10,
                    y: gauge_y - 8,
                    w: canvas.text_width("CPU", FONT_TINY).max(0) as u32,
                    h: canvas.line_height(FONT_TINY).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "CPU".to_string(),
                    x: cpu_cx - 10,
                    y: gauge_y - 8,
                    size: FONT_TINY,
                    color: colors.dim,
                },
            });
            let cpu_text = format!("{:.0}%", data.cpu_percent);
            let cpu_w = canvas.text_width(&cpu_text, FONT_NORMAL);
            layout.push(Widget {
                id: "cpu_val",
                rect: Rect {
                    x: cpu_cx - cpu_w / 2,
                    y: gauge_y + 2,
                    w: cpu_w.max(0) as u32,
                    h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: cpu_text,
                    x: cpu_cx - cpu_w / 2,
                    y: gauge_y + 2,
                    size: FONT_NORMAL,
                    color: colors.text,
                },
            });

            // Base element: RAM gauge (always shown)
            let ram_cx = cpu_cx + gauge_radius as i32 * 2 + 30;
            Self::push_arc_specs(
                &mut layout,
                "ram_gauge",
                Self::arc_gauge_specs(
                    ram_cx,
                    gauge_y,
                    gauge_radius,
                    stroke,
                    data.ram_percent,
                    colors.secondary,
                    colors.arc_bg,
                ),
                gauge_radius,
            );
            layout.push(Widget {
                id: "ram_label",
                rect: Rect {
                    x: ram_cx - 12,
                    y: gauge_y - 8,
                    w: canvas.text_width("RAM", FONT_TINY).max(0) as u32,
                    h: canvas.line_height(FONT_TINY).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: "RAM".to_string(),
                    x: ram_cx - 12,
                    y: gauge_y - 8,
                    size: FONT_TINY,
                    color: colors.dim,
                },
            });
            let ram_text = format!("{:.0}%", data.ram_percent);
            let ram_w = canvas.text_width(&ram_text, FONT_NORMAL);
            layout.push(Widget {
                id: "ram_val",
                rect: Rect {
                    x: ram_cx - ram_w / 2,
                    y: gauge_y + 2,
                    w: ram_w.max(0) as u32,
                    h: canvas.line_height(FONT_NORMAL).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::EveryFrame,
                content: WidgetContent::Text {
                    text: ram_text,
                    x: ram_cx - ram_w / 2,
                    y: gauge_y + 2,
                    size: FONT_NORMAL,
                    color: colors.text,
                },
            });

            let io_x = ram_cx + gauge_radius as i32 + 40;
            let io_max = 100_000_000.0;
            let disk_r_cx = io_x;
            let disk_cy = margin + 28 + small_radius as i32;

            // Complication: Disk gauges
            if is_on(complication_names::DISK_IO) {
                Self::push_arc_specs(
                    &mut layout,
                    "disk_r",
                    Self::activity_arc_specs(
                        disk_r_cx,
                        disk_cy,
                        small_radius,
                        small_stroke,
                        data.disk_read_rate,
                        io_max,
                        colors.primary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                // Number centered in dial
                let disk_r_text = format_rate_short(data.disk_read_rate);
                let disk_r_w = canvas.text_width(&disk_r_text, FONT_TINY);
                layout.push(Widget {
                    id: "disk_r_val",
                    rect: Rect {
                        x: disk_r_cx - disk_r_w / 2,
                        y: disk_cy - 5,
                        w: disk_r_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_r_text,
                        x: disk_r_cx - disk_r_w / 2,
                        y: disk_cy - 5,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                // Letter in bottom open space
                layout.push(Widget {
                    id: "disk_r_lbl",
                    rect: Rect {
                        x: disk_r_cx - 3,
                        y: disk_cy + small_radius as i32 / 2,
                        w: canvas.text_width("R", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "R".to_string(),
                        x: disk_r_cx - 3,
                        y: disk_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });

                let disk_w_cx = disk_r_cx + small_radius as i32 * 2 + 12;
                Self::push_arc_specs(
                    &mut layout,
                    "disk_w",
                    Self::activity_arc_specs(
                        disk_w_cx,
                        disk_cy,
                        small_radius,
                        small_stroke,
                        data.disk_write_rate,
                        io_max,
                        colors.primary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                // Number centered in dial
                let disk_w_text = format_rate_short(data.disk_write_rate);
                let disk_w_w = canvas.text_width(&disk_w_text, FONT_TINY);
                layout.push(Widget {
                    id: "disk_w_val",
                    rect: Rect {
                        x: disk_w_cx - disk_w_w / 2,
                        y: disk_cy - 5,
                        w: disk_w_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: disk_w_text,
                        x: disk_w_cx - disk_w_w / 2,
                        y: disk_cy - 5,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                // Letter in bottom open space
                layout.push(Widget {
                    id: "disk_w_lbl",
                    rect: Rect {
                        x: disk_w_cx - 4,
                        y: disk_cy + small_radius as i32 / 2,
                        w: canvas.text_width("W", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "W".to_string(),
                        x: disk_w_cx - 4,
                        y: disk_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });
            }

            // Complication: Network gauges
            let net_cy = disk_cy + small_radius as i32 * 2 + 12;
            if is_on(complication_names::NETWORK) {
                Self::push_arc_specs(
                    &mut layout,
                    "net_rx",
                    Self::activity_arc_specs(
                        disk_r_cx,
                        net_cy,
                        small_radius,
                        small_stroke,
                        data.net_rx_rate,
                        io_max,
                        colors.secondary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                // Number centered in dial
                let net_rx_text = format_rate_short(data.net_rx_rate);
                let net_rx_w = canvas.text_width(&net_rx_text, FONT_TINY);
                layout.push(Widget {
                    id: "net_rx_val",
                    rect: Rect {
                        x: disk_r_cx - net_rx_w / 2,
                        y: net_cy - 5,
                        w: net_rx_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_rx_text,
                        x: disk_r_cx - net_rx_w / 2,
                        y: net_cy - 5,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                // Arrow in bottom open space
                layout.push(Widget {
                    id: "net_rx_lbl",
                    rect: Rect {
                        x: disk_r_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        w: canvas.text_width("\u{2193}", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "\u{2193}".to_string(),
                        x: disk_r_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });

                let net_w_cx = disk_r_cx + small_radius as i32 * 2 + 12;
                Self::push_arc_specs(
                    &mut layout,
                    "net_tx",
                    Self::activity_arc_specs(
                        net_w_cx,
                        net_cy,
                        small_radius,
                        small_stroke,
                        data.net_tx_rate,
                        io_max,
                        colors.secondary,
                        colors.arc_bg,
                    ),
                    small_radius,
                );
                // Number centered in dial
                let net_tx_text = format_rate_short(data.net_tx_rate);
                let net_tx_w = canvas.text_width(&net_tx_text, FONT_TINY);
                layout.push(Widget {
                    id: "net_tx_val",
                    rect: Rect {
                        x: net_w_cx - net_tx_w / 2,
                        y: net_cy - 5,
                        w: net_tx_w.max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: WidgetContent::Text {
                        text: net_tx_text,
                        x: net_w_cx - net_tx_w / 2,
                        y: net_cy - 5,
                        size: FONT_TINY,
                        color: colors.text,
                    },
                });
                // Arrow in bottom open space
                layout.push(Widget {
                    id: "net_tx_lbl",
                    rect: Rect {
                        x: net_w_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        w: canvas.text_width("\u{2191}", FONT_TINY).max(0) as u32,
                        h: canvas.line_height(FONT_TINY).max(0) as u32,
                    },
                    kind: ZoneKind::Static,
                    cadence: Cadence::OnChange,
                    content: WidgetContent::Text {
                        text: "\u{2191}".to_string(),
                        x: net_w_cx - 4,
                        y: net_cy + small_radius as i32 / 2,
                        size: FONT_TINY,
                        color: colors.dim,
                    },
                });
            }

            // Base element: Uptime at bottom (always shown)
            let bottom_y = height as i32 - margin - 14;
            let uptime_text = format!("Up: {}", data.uptime);
            layout.push(Widget {
                id: "uptime",
                rect: Rect {
                    x: margin,
                    y: bottom_y,
                    w: canvas.text_width(&uptime_text, FONT_TINY).max(0) as u32,
                    h: canvas.line_height(FONT_TINY).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content: WidgetContent::Text {
                    text: uptime_text,
                    x: margin,
                    y: bottom_y,
                    size: FONT_TINY,
                    color: colors.dim,
                },
            });

            // Complication: IP address
            if is_on(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    let ip_text = format!("IP: {}", ip);
                    let ip_width = canvas.text_width(&ip_text, FONT_TINY);
                    layout.push(Widget {
                        id: "ip_addr",
                        rect: Rect {
                            x: width as i32 - margin - ip_width,
                            y: bottom_y,
                            w: ip_width.max(0) as u32,
                            h: canvas.line_height(FONT_TINY).max(0) as u32,
                        },
                        kind: ZoneKind::Static,
                        cadence: Cadence::OnChange,
                        content: WidgetContent::Text {
                            text: ip_text,
                            x: width as i32 - margin - ip_width,
                            y: bottom_y,
                            size: FONT_TINY,
                            color: colors.dim,
                        },
                    });
                }
            }
        }

        Some(layout)
    }
}

impl Default for ArcsFace {
    fn default() -> Self {
        Self::new()
    }
}

impl Face for ArcsFace {
    fn name(&self) -> &str {
        "arcs"
    }

    fn available_complications(&self) -> Vec<Complication> {
        vec![
            complications::time(true),
            complications::date(true, date_formats::ISO),
            complications::ip_address(true),
            complications::network(true),
            complications::disk_io(true),
            complications::cpu_temp(false),
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
        let (width, height) = canvas.dimensions();
        let portrait = width < 200;

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
            // Portrait layout: CPU, RAM stacked vertically, disk/net on separate rows at bottom
            let margin = 6;
            let center_x = width as i32 / 2;

            // Calculate vertical layout to use full height
            // Reserve space: top text (~30px), bottom text (~36px for 3 lines), remaining for arcs
            let top_text_height = 28_i32;
            let bottom_text_height = 36_i32;
            let available_height =
                height as i32 - margin * 2 - top_text_height - bottom_text_height;

            // Large arcs (CPU, RAM) get 30% each, two small arc rows get 20% each
            let large_arc_height = (available_height * 30) / 100;
            let small_arc_height = (available_height * 20) / 100;

            let large_radius = ((large_arc_height - 8) / 2).min(34) as u32;
            let large_stroke = 6.0;
            let small_radius = ((small_arc_height - 4) / 2).min(18) as u32;
            let small_stroke = 3.5;

            let mut y = margin;

            // Top section: Time and date
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock centered
                    let clock_radius = 10_u32;
                    let clock_cx = width as i32 / 2;
                    let clock_cy = y + clock_radius as i32;
                    draw_mini_analog_clock(
                        canvas,
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.primary,
                        colors.text,
                    );
                    y += (clock_radius * 2) as i32 + 2;
                } else {
                    let time_str = data.format_time(time_format);
                    let time_width = canvas.text_width(&time_str, FONT_NORMAL);
                    canvas.draw_text(
                        (width as i32 - time_width) / 2,
                        y,
                        &time_str,
                        FONT_NORMAL,
                        colors.text,
                    );
                    y += canvas.line_height(FONT_NORMAL);
                }
            }

            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_TINY);
                    canvas.draw_text(
                        (width as i32 - date_width) / 2,
                        y,
                        &date_str,
                        FONT_TINY,
                        colors.dim,
                    );
                }
            }
            y = margin + top_text_height;

            // CPU arc (centered)
            let cpu_cy = y + large_radius as i32 + 2;
            Self::draw_arc_gauge(
                canvas,
                center_x,
                cpu_cy,
                large_radius,
                large_stroke,
                data.cpu_percent,
                colors.primary,
                colors.arc_bg,
            );
            canvas.draw_text(center_x - 10, cpu_cy - 6, "CPU", FONT_TINY, colors.dim);
            let cpu_text = format!("{:.0}%", data.cpu_percent);
            let cpu_w = canvas.text_width(&cpu_text, FONT_SMALL);
            canvas.draw_text(
                center_x - cpu_w / 2,
                cpu_cy + 4,
                &cpu_text,
                FONT_SMALL,
                colors.text,
            );
            y += large_arc_height;

            // RAM arc (centered)
            let ram_cy = y + large_radius as i32 + 2;
            Self::draw_arc_gauge(
                canvas,
                center_x,
                ram_cy,
                large_radius,
                large_stroke,
                data.ram_percent,
                colors.secondary,
                colors.arc_bg,
            );
            canvas.draw_text(center_x - 12, ram_cy - 6, "RAM", FONT_TINY, colors.dim);
            let ram_text = format!("{:.0}%", data.ram_percent);
            let ram_w = canvas.text_width(&ram_text, FONT_SMALL);
            canvas.draw_text(
                center_x - ram_w / 2,
                ram_cy + 4,
                &ram_text,
                FONT_SMALL,
                colors.text,
            );
            y += large_arc_height;

            let io_max = 100_000_000.0;

            // Disk row: Read and Write arcs centered
            if is_on(complication_names::DISK_IO) {
                let disk_cy = y + small_radius as i32 + 2;
                let disk_r_cx = center_x - small_radius as i32 - 6;
                Self::draw_activity_arc(
                    canvas,
                    disk_r_cx,
                    disk_cy,
                    small_radius,
                    small_stroke,
                    data.disk_read_rate,
                    io_max,
                    colors.primary,
                    colors.arc_bg,
                );
                let disk_r_text = format_rate_short(data.disk_read_rate);
                let disk_r_w = canvas.text_width(&disk_r_text, FONT_TINY);
                canvas.draw_text(
                    disk_r_cx - disk_r_w / 2,
                    disk_cy - 4,
                    &disk_r_text,
                    FONT_TINY,
                    colors.text,
                );
                canvas.draw_text(
                    disk_r_cx - 3,
                    disk_cy + small_radius as i32 / 2,
                    "R",
                    FONT_TINY,
                    colors.dim,
                );

                let disk_w_cx = center_x + small_radius as i32 + 6;
                Self::draw_activity_arc(
                    canvas,
                    disk_w_cx,
                    disk_cy,
                    small_radius,
                    small_stroke,
                    data.disk_write_rate,
                    io_max,
                    colors.primary,
                    colors.arc_bg,
                );
                let disk_w_text = format_rate_short(data.disk_write_rate);
                let disk_w_w = canvas.text_width(&disk_w_text, FONT_TINY);
                canvas.draw_text(
                    disk_w_cx - disk_w_w / 2,
                    disk_cy - 4,
                    &disk_w_text,
                    FONT_TINY,
                    colors.text,
                );
                canvas.draw_text(
                    disk_w_cx - 4,
                    disk_cy + small_radius as i32 / 2,
                    "W",
                    FONT_TINY,
                    colors.dim,
                );
                y += small_arc_height;
            }

            // Network row: RX and TX arcs centered
            if is_on(complication_names::NETWORK) {
                let net_cy = y + small_radius as i32 + 2;
                let net_rx_cx = center_x - small_radius as i32 - 6;
                Self::draw_activity_arc(
                    canvas,
                    net_rx_cx,
                    net_cy,
                    small_radius,
                    small_stroke,
                    data.net_rx_rate,
                    io_max,
                    colors.secondary,
                    colors.arc_bg,
                );
                let net_rx_text = format_rate_short(data.net_rx_rate);
                let net_rx_w = canvas.text_width(&net_rx_text, FONT_TINY);
                canvas.draw_text(
                    net_rx_cx - net_rx_w / 2,
                    net_cy - 4,
                    &net_rx_text,
                    FONT_TINY,
                    colors.text,
                );
                canvas.draw_text(
                    net_rx_cx - 4,
                    net_cy + small_radius as i32 / 2,
                    "\u{2193}",
                    FONT_TINY,
                    colors.dim,
                );

                let net_tx_cx = center_x + small_radius as i32 + 6;
                Self::draw_activity_arc(
                    canvas,
                    net_tx_cx,
                    net_cy,
                    small_radius,
                    small_stroke,
                    data.net_tx_rate,
                    io_max,
                    colors.secondary,
                    colors.arc_bg,
                );
                let net_tx_text = format_rate_short(data.net_tx_rate);
                let net_tx_w = canvas.text_width(&net_tx_text, FONT_TINY);
                canvas.draw_text(
                    net_tx_cx - net_tx_w / 2,
                    net_cy - 4,
                    &net_tx_text,
                    FONT_TINY,
                    colors.text,
                );
                canvas.draw_text(
                    net_tx_cx - 4,
                    net_cy + small_radius as i32 / 2,
                    "\u{2191}",
                    FONT_TINY,
                    colors.dim,
                );
            }

            // Bottom text: hostname, uptime, and IP on separate lines
            let bottom_y = height as i32 - margin - 46;

            // Hostname centered on its own line
            let host_width = canvas.text_width(&data.hostname, FONT_TINY);
            canvas.draw_text(
                (width as i32 - host_width) / 2,
                bottom_y,
                &data.hostname,
                FONT_TINY,
                colors.dim,
            );

            // Uptime on its own line
            let uptime_text = format!("Up: {}", data.uptime);
            canvas.draw_text(margin, bottom_y + 12, &uptime_text, FONT_TINY, colors.dim);

            // IP on the next line
            if is_on(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    canvas.draw_text(margin, bottom_y + 24, ip, FONT_TINY, colors.dim);
                }
            }
        } else {
            // Landscape layout
            let margin = 10;
            let gauge_radius = 36_u32;
            let stroke = 8.0;
            let small_radius = 22_u32;
            let small_stroke = 5.0;

            let top_y = margin;

            // Complication: Time
            if is_on(complication_names::TIME) {
                if time_format == time_formats::ANALOGUE {
                    // Draw small analog clock on the left
                    let clock_radius = 12_u32;
                    let clock_cx = margin + clock_radius as i32;
                    let clock_cy = top_y + clock_radius as i32;
                    draw_mini_analog_clock(
                        canvas,
                        clock_cx,
                        clock_cy,
                        clock_radius,
                        data.hour,
                        data.minute,
                        colors.primary,
                        colors.text,
                    );
                } else {
                    let time_str = data.format_time(time_format);
                    canvas.draw_text(margin, top_y, &time_str, FONT_LARGE, colors.text);
                }
            }

            // Hostname at top right (always shown)
            let host_width = canvas.text_width(&data.hostname, FONT_SMALL);
            canvas.draw_text(
                width as i32 - margin - host_width,
                top_y,
                &data.hostname,
                FONT_SMALL,
                colors.dim,
            );

            // Complication: Date (below hostname if shown)
            if is_on(complication_names::DATE) {
                if let Some(date_str) = data.format_date(date_format) {
                    let date_width = canvas.text_width(&date_str, FONT_TINY);
                    canvas.draw_text(
                        width as i32 - margin - date_width,
                        top_y + 14,
                        &date_str,
                        FONT_TINY,
                        colors.dim,
                    );
                }
            }

            let gauge_y = margin + 28 + gauge_radius as i32;
            let cpu_cx = margin + gauge_radius as i32 + 10;

            // Base element: CPU gauge (always shown)
            Self::draw_arc_gauge(
                canvas,
                cpu_cx,
                gauge_y,
                gauge_radius,
                stroke,
                data.cpu_percent,
                colors.primary,
                colors.arc_bg,
            );
            canvas.draw_text(cpu_cx - 10, gauge_y - 8, "CPU", FONT_TINY, colors.dim);
            let cpu_text = format!("{:.0}%", data.cpu_percent);
            let cpu_w = canvas.text_width(&cpu_text, FONT_NORMAL);
            canvas.draw_text(
                cpu_cx - cpu_w / 2,
                gauge_y + 2,
                &cpu_text,
                FONT_NORMAL,
                colors.text,
            );

            // Base element: RAM gauge (always shown)
            let ram_cx = cpu_cx + gauge_radius as i32 * 2 + 30;
            Self::draw_arc_gauge(
                canvas,
                ram_cx,
                gauge_y,
                gauge_radius,
                stroke,
                data.ram_percent,
                colors.secondary,
                colors.arc_bg,
            );
            canvas.draw_text(ram_cx - 12, gauge_y - 8, "RAM", FONT_TINY, colors.dim);
            let ram_text = format!("{:.0}%", data.ram_percent);
            let ram_w = canvas.text_width(&ram_text, FONT_NORMAL);
            canvas.draw_text(
                ram_cx - ram_w / 2,
                gauge_y + 2,
                &ram_text,
                FONT_NORMAL,
                colors.text,
            );

            let io_x = ram_cx + gauge_radius as i32 + 40;
            let io_max = 100_000_000.0;
            let disk_r_cx = io_x;
            let disk_cy = margin + 28 + small_radius as i32;

            // Complication: Disk gauges
            if is_on(complication_names::DISK_IO) {
                Self::draw_activity_arc(
                    canvas,
                    disk_r_cx,
                    disk_cy,
                    small_radius,
                    small_stroke,
                    data.disk_read_rate,
                    io_max,
                    colors.primary,
                    colors.arc_bg,
                );
                // Number centered in dial
                let disk_r_text = format_rate_short(data.disk_read_rate);
                let disk_r_w = canvas.text_width(&disk_r_text, FONT_TINY);
                canvas.draw_text(
                    disk_r_cx - disk_r_w / 2,
                    disk_cy - 5,
                    &disk_r_text,
                    FONT_TINY,
                    colors.text,
                );
                // Letter in bottom open space
                canvas.draw_text(
                    disk_r_cx - 3,
                    disk_cy + small_radius as i32 / 2,
                    "R",
                    FONT_TINY,
                    colors.dim,
                );

                let disk_w_cx = disk_r_cx + small_radius as i32 * 2 + 12;
                Self::draw_activity_arc(
                    canvas,
                    disk_w_cx,
                    disk_cy,
                    small_radius,
                    small_stroke,
                    data.disk_write_rate,
                    io_max,
                    colors.primary,
                    colors.arc_bg,
                );
                // Number centered in dial
                let disk_w_text = format_rate_short(data.disk_write_rate);
                let disk_w_w = canvas.text_width(&disk_w_text, FONT_TINY);
                canvas.draw_text(
                    disk_w_cx - disk_w_w / 2,
                    disk_cy - 5,
                    &disk_w_text,
                    FONT_TINY,
                    colors.text,
                );
                // Letter in bottom open space
                canvas.draw_text(
                    disk_w_cx - 4,
                    disk_cy + small_radius as i32 / 2,
                    "W",
                    FONT_TINY,
                    colors.dim,
                );
            }

            // Complication: Network gauges
            let net_cy = disk_cy + small_radius as i32 * 2 + 12;
            if is_on(complication_names::NETWORK) {
                Self::draw_activity_arc(
                    canvas,
                    disk_r_cx,
                    net_cy,
                    small_radius,
                    small_stroke,
                    data.net_rx_rate,
                    io_max,
                    colors.secondary,
                    colors.arc_bg,
                );
                // Number centered in dial
                let net_rx_text = format_rate_short(data.net_rx_rate);
                let net_rx_w = canvas.text_width(&net_rx_text, FONT_TINY);
                canvas.draw_text(
                    disk_r_cx - net_rx_w / 2,
                    net_cy - 5,
                    &net_rx_text,
                    FONT_TINY,
                    colors.text,
                );
                // Arrow in bottom open space
                canvas.draw_text(
                    disk_r_cx - 4,
                    net_cy + small_radius as i32 / 2,
                    "\u{2193}",
                    FONT_TINY,
                    colors.dim,
                );

                let net_w_cx = disk_r_cx + small_radius as i32 * 2 + 12;
                Self::draw_activity_arc(
                    canvas,
                    net_w_cx,
                    net_cy,
                    small_radius,
                    small_stroke,
                    data.net_tx_rate,
                    io_max,
                    colors.secondary,
                    colors.arc_bg,
                );
                // Number centered in dial
                let net_tx_text = format_rate_short(data.net_tx_rate);
                let net_tx_w = canvas.text_width(&net_tx_text, FONT_TINY);
                canvas.draw_text(
                    net_w_cx - net_tx_w / 2,
                    net_cy - 5,
                    &net_tx_text,
                    FONT_TINY,
                    colors.text,
                );
                // Arrow in bottom open space
                canvas.draw_text(
                    net_w_cx - 4,
                    net_cy + small_radius as i32 / 2,
                    "\u{2191}",
                    FONT_TINY,
                    colors.dim,
                );
            }

            // Base element: Uptime at bottom (always shown)
            let bottom_y = height as i32 - margin - 14;
            let uptime_text = format!("Up: {}", data.uptime);
            canvas.draw_text(margin, bottom_y, &uptime_text, FONT_TINY, colors.dim);

            // Complication: IP address
            if is_on(complication_names::IP_ADDRESS) {
                if let Some(ref ip) = data.display_ip {
                    let ip_text = format!("IP: {}", ip);
                    let ip_width = canvas.text_width(&ip_text, FONT_TINY);
                    canvas.draw_text(
                        width as i32 - margin - ip_width,
                        bottom_y,
                        &ip_text,
                        FONT_TINY,
                        colors.dim,
                    );
                }
            }
        }
    }

    fn layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        comp: &EnabledComplications,
    ) -> Option<crate::faces::layout::Layout> {
        self.build_layout(canvas, data, theme, comp)
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

    /// Deterministic sample data so both render paths see identical input.
    #[allow(clippy::field_reassign_with_default)]
    fn sample() -> SystemData {
        let mut d = SystemData::default();
        d.hostname = "node01".into();
        d.uptime = "3d 8h".into();
        d.cpu_percent = 55.0;
        d.ram_percent = 72.0;
        d.hour = 14;
        d.minute = 30;
        d.disk_read_rate = 1024.0 * 1024.0 * 5.0;
        d.disk_write_rate = 1024.0 * 1024.0 * 2.0;
        d.net_rx_rate = 1024.0 * 1024.0 * 1.5;
        d.net_tx_rate = 1024.0 * 512.0;
        d
    }

    fn sample_with_ip(ip: &str) -> SystemData {
        let mut d = sample();
        d.display_ip = Some(ip.to_string());
        d
    }

    fn analogue_comps() -> EnabledComplications {
        let mut comps = EnabledComplications::new();
        comps.set_enabled("arcs", complication_names::TIME, true);
        comps.set_option(
            "arcs",
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
        let face = ArcsFace::new();
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
    fn arcs_layout_matches_render_landscape() {
        let (legacy, via_layout) = render_both(320, 170);
        assert_eq!(
            legacy, via_layout,
            "arcs landscape mismatch (default comps)"
        );
        assert_eq!(
            pixel_hash(&via_layout),
            14149716339363237421,
            "golden drift: arcs_layout_matches_render_landscape"
        );
    }

    #[test]
    fn arcs_layout_matches_render_landscape_with_ip() {
        let (legacy, via_layout) = render_both_with(320, 170, sample_with_ip("192.168.1.100"));
        assert_eq!(legacy, via_layout, "arcs landscape mismatch (IPv4)");
        assert_eq!(
            pixel_hash(&via_layout),
            18341083721031071558,
            "golden drift: arcs_layout_matches_render_landscape_with_ip"
        );
    }

    #[test]
    fn arcs_layout_matches_render_landscape_ipv6() {
        let (legacy, via_layout) =
            render_both_with(320, 170, sample_with_ip("2001:db8::dead:beef:1:2"));
        assert_eq!(legacy, via_layout, "arcs landscape mismatch (IPv6)");
        assert_eq!(
            pixel_hash(&via_layout),
            2326329289435487373,
            "golden drift: arcs_layout_matches_render_landscape_ipv6"
        );
    }

    #[test]
    fn arcs_layout_matches_render_portrait() {
        let (legacy, via_layout) = render_both(170, 320);
        assert_eq!(legacy, via_layout, "arcs portrait mismatch (default comps)");
        assert_eq!(
            pixel_hash(&via_layout),
            9954537746452454605,
            "golden drift: arcs_layout_matches_render_portrait"
        );
    }

    #[test]
    fn arcs_layout_matches_render_portrait_with_ip() {
        let (legacy, via_layout) = render_both_with(170, 320, sample_with_ip("192.168.1.100"));
        assert_eq!(legacy, via_layout, "arcs portrait mismatch (IPv4)");
        assert_eq!(
            pixel_hash(&via_layout),
            14163956190879489031,
            "golden drift: arcs_layout_matches_render_portrait_with_ip"
        );
    }

    #[test]
    fn arcs_layout_matches_render_portrait_ipv6() {
        let (legacy, via_layout) =
            render_both_with(170, 320, sample_with_ip("2001:db8::dead:beef:1:2"));
        assert_eq!(legacy, via_layout, "arcs portrait mismatch (IPv6)");
        assert_eq!(
            pixel_hash(&via_layout),
            15617094980113426996,
            "golden drift: arcs_layout_matches_render_portrait_ipv6"
        );
    }

    #[test]
    fn arcs_layout_matches_render_landscape_analogue() {
        let (legacy, via_layout) = render_both_comps(320, 170, sample(), analogue_comps());
        assert_eq!(legacy, via_layout, "arcs landscape analogue mismatch");
        assert_eq!(
            pixel_hash(&via_layout),
            4911362616083430275,
            "golden drift: arcs_layout_matches_render_landscape_analogue"
        );
    }

    #[test]
    fn arcs_layout_matches_render_portrait_analogue() {
        let (legacy, via_layout) = render_both_comps(170, 320, sample(), analogue_comps());
        assert_eq!(legacy, via_layout, "arcs portrait analogue mismatch");
        assert_eq!(
            pixel_hash(&via_layout),
            8763038846165929711,
            "golden drift: arcs_layout_matches_render_portrait_analogue"
        );
    }
}
