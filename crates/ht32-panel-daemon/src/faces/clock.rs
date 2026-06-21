//! Clock face displaying a clean analog clock.
//!
//! A minimalist watch face focused on time display with optional
//! date and hostname complications.

use std::f32::consts::PI;

use super::{
    complication_names, complication_options, complications, date_formats, Complication,
    EnabledComplications, Face, Theme,
};

/// Default font size for digital time.
const DEFAULT_TIME_SIZE: f32 = 32.0;
use crate::faces::layout::{Cadence, Layout, Rect, Widget, WidgetContent, ZoneKind};
use crate::rendering::Canvas;
use crate::sensors::data::SystemData;

/// A single primitive draw call for the full-size analog clock face.
///
/// Returned by [`ClockFace::analog_clock_draws`] and consumed by the
/// typed-widget Layout path ([`ClockFace::build_analog_clock`]), centralizing
/// the trigonometry so the geometry is computed in exactly one place.
enum AnalogDraw {
    Arc {
        cx: i32,
        cy: i32,
        r: u32,
        start_angle: f32,
        end_angle: f32,
        stroke: f32,
        color: u32,
    },
    Line {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        stroke: f32,
        color: u32,
    },
    Circle {
        cx: i32,
        cy: i32,
        r: u32,
        color: u32,
    },
}

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

/// Derive colors from theme for the clock face.
struct FaceColors {
    /// Clock face outline and markers
    outline: u32,
    /// Hour hand color
    hour_hand: u32,
    /// Minute hand color
    minute_hand: u32,
    /// Center dot color
    center: u32,
    /// Text color (for complications)
    text: u32,
    /// Dimmed text color
    dim: u32,
}

impl FaceColors {
    fn from_theme(theme: &Theme) -> Self {
        Self {
            outline: theme.primary,
            hour_hand: theme.text,
            minute_hand: theme.text,
            center: theme.primary,
            text: theme.text,
            dim: dim_color(theme.text, theme.background, 0.7),
        }
    }
}

/// Font sizes.
const FONT_NORMAL: f32 = 14.0;
const FONT_SMALL: f32 = 12.0;

/// A minimalist analog clock face.
pub struct ClockFace;

/// Display options for the clock face.
struct ClockLayout {
    show_hostname: bool,
    show_date: bool,
    hostname: String,
    date: Option<String>,
}

impl ClockFace {
    /// Creates a new clock face.
    pub fn new() -> Self {
        Self
    }

    /// Computes the draw-primitive specs for the analog clock face (circle,
    /// markers, and hands) without touching a canvas.
    ///
    /// Consumed by the typed-widget Layout path ([`Self::build_analog_clock`]),
    /// so the floating-point trigonometry is computed in exactly one place.
    ///
    /// Order:
    ///   bezel Arc → 12 tick `Line`s → hour-hand `Line` → minute-hand `Line` → hub `Circle`.
    fn analog_clock_draws(
        cx: i32,
        cy: i32,
        radius: u32,
        hour: u8,
        minute: u8,
        colors: &FaceColors,
    ) -> Vec<AnalogDraw> {
        let radius_f = radius as f32;
        let mut draws = Vec::new();

        // Clock face outline
        draws.push(AnalogDraw::Arc {
            cx,
            cy,
            r: radius,
            start_angle: 0.0,
            end_angle: 2.0 * PI,
            stroke: 2.0,
            color: colors.outline,
        });

        // Hour markers
        for i in 0..12 {
            let angle = (i as f32) * PI / 6.0 - PI / 2.0;
            let inner_r = radius_f * 0.85;
            let outer_r = radius_f * 0.95;

            let x1 = cx as f32 + inner_r * angle.cos();
            let y1 = cy as f32 + inner_r * angle.sin();
            let x2 = cx as f32 + outer_r * angle.cos();
            let y2 = cy as f32 + outer_r * angle.sin();

            let stroke = if i % 3 == 0 { 3.0 } else { 1.5 };
            draws.push(AnalogDraw::Line {
                x1: x1 as i32,
                y1: y1 as i32,
                x2: x2 as i32,
                y2: y2 as i32,
                stroke,
                color: colors.outline,
            });
        }

        // Calculate hand angles (12 o'clock = -PI/2)
        let minute_angle = (minute as f32) * PI / 30.0 - PI / 2.0;
        let hour_angle = ((hour % 12) as f32 + minute as f32 / 60.0) * PI / 6.0 - PI / 2.0;

        // Hour hand (shorter, thicker)
        let hour_length = radius_f * 0.5;
        let hour_x = cx as f32 + hour_length * hour_angle.cos();
        let hour_y = cy as f32 + hour_length * hour_angle.sin();
        draws.push(AnalogDraw::Line {
            x1: cx,
            y1: cy,
            x2: hour_x as i32,
            y2: hour_y as i32,
            stroke: 4.0,
            color: colors.hour_hand,
        });

        // Minute hand (longer, thinner)
        let minute_length = radius_f * 0.75;
        let minute_x = cx as f32 + minute_length * minute_angle.cos();
        let minute_y = cy as f32 + minute_length * minute_angle.sin();
        draws.push(AnalogDraw::Line {
            x1: cx,
            y1: cy,
            x2: minute_x as i32,
            y2: minute_y as i32,
            stroke: 2.5,
            color: colors.minute_hand,
        });

        // Center dot
        draws.push(AnalogDraw::Circle {
            cx,
            cy,
            r: 4,
            color: colors.center,
        });

        draws
    }

    /// Maps a single [`AnalogDraw`] spec to a stable widget id and a
    /// [`WidgetContent`] variant for use in `build_layout`.
    ///
    /// The `index` is the 0-based position in the slice returned by
    /// [`Self::analog_clock_draws`]:
    ///   0 → bezel Arc, 1..=12 → tick Lines, 13 → hour hand, 14 → minute hand, 15 → hub.
    fn analog_draw_to_widget(draw: AnalogDraw, index: usize) -> (&'static str, WidgetContent) {
        match draw {
            AnalogDraw::Arc {
                cx,
                cy,
                r,
                start_angle,
                end_angle,
                stroke,
                color,
            } => (
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
            AnalogDraw::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                color,
            } => {
                let id = match index {
                    13 => "clock_hour",
                    14 => "clock_minute",
                    _ => "clock_tick",
                };
                (
                    id,
                    WidgetContent::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        stroke,
                        color,
                    },
                )
            }
            AnalogDraw::Circle { cx, cy, r, color } => {
                ("clock_hub", WidgetContent::Circle { cx, cy, r, color })
            }
        }
    }

    /// Pushes a horizontally-centered text widget and returns its line height.
    ///
    /// The x position is computed as `(width - text_width) / 2`.
    fn push_centered_text(
        layout: &mut Layout,
        canvas: &Canvas,
        id: &'static str,
        y: i32,
        text: &str,
        font_size: f32,
        color: u32,
    ) -> i32 {
        let (width, _) = canvas.dimensions();
        let text_width = canvas.text_width(text, font_size);
        let x = (width as i32 - text_width) / 2;
        layout.push(Widget {
            id: id.into(),
            rect: Rect {
                x,
                y,
                w: text_width.max(0) as u32,
                h: canvas.line_height(font_size).max(0) as u32,
            },
            kind: ZoneKind::Dynamic,
            cadence: Cadence::Seconds(60),
            content: WidgetContent::Text {
                text: text.to_string(),
                x,
                y,
                size: font_size,
                color,
            },
        });
        canvas.line_height(font_size)
    }

    /// Builds the digital-time widgets, including the portrait font-scaling
    /// branch (size 56..=96 grows digits taller and scales horizontally).
    fn build_digital_time(
        layout: &mut Layout,
        canvas: &Canvas,
        hour: u8,
        minute: u8,
        clock: &ClockLayout,
        colors: &FaceColors,
        time_font_size: f32,
    ) {
        let (width, height) = canvas.dimensions();
        let time_str = format!("{:02}:{:02}", hour, minute);
        let portrait = width < 200;

        // In portrait mode with font sizes 56-96, grow digits taller to fill screen height
        // while scaling horizontally to prevent overflow
        let (effective_font_size, x_scale) =
            if portrait && time_font_size > 56.0 && time_font_size <= 96.0 {
                // Calculate space for hostname and date
                let hostname_space = if clock.show_hostname {
                    canvas.line_height(FONT_SMALL) + 8
                } else {
                    0
                };
                let date_space = if clock.show_date && clock.date.is_some() {
                    canvas.line_height(FONT_NORMAL) + 8
                } else {
                    0
                };

                // Available height for the time digits (with some margin)
                let margin_v = 8.0;
                let available_height =
                    height as f32 - hostname_space as f32 - date_space as f32 - margin_v * 2.0;

                // Calculate font size that would fill the available height
                // line_height is approximately font_size * 1.2
                let target_font_size = (available_height / 1.2).min(96.0);

                // Use the larger of requested size and calculated size, capped at 96
                let effective_size = time_font_size.max(target_font_size).min(96.0);

                // Calculate horizontal scale to fit within width
                let margin_h = 4.0;
                let available_width = width as f32 - margin_h * 2.0;
                let natural_width = canvas.text_width(&time_str, effective_size) as f32;
                let scale = if natural_width > available_width {
                    available_width / natural_width
                } else {
                    1.0
                };

                (effective_size, scale)
            } else if portrait && time_font_size > 96.0 {
                // For sizes > 96, just scale horizontally to fit
                let margin = 4.0;
                let available_width = width as f32 - margin * 2.0;
                let natural_width = canvas.text_width(&time_str, time_font_size) as f32;
                let scale = if natural_width > available_width {
                    available_width / natural_width
                } else {
                    1.0
                };
                (time_font_size, scale)
            } else {
                (time_font_size, 1.0)
            };

        // Calculate total height needed
        let time_height = canvas.line_height(effective_font_size);
        let mut total_height = time_height;
        if clock.show_hostname {
            total_height += canvas.line_height(FONT_SMALL) + 4;
        }
        if clock.show_date && clock.date.is_some() {
            total_height += canvas.line_height(FONT_NORMAL) + 4;
        }

        let mut y = (height as i32 - total_height) / 2;

        if clock.show_hostname {
            let h = Self::push_centered_text(
                layout,
                canvas,
                "hostname",
                y,
                &clock.hostname,
                FONT_SMALL,
                colors.dim,
            );
            y += h + 4;
        }

        // Draw time with scaling if needed
        if x_scale < 1.0 {
            let text_width = canvas.text_width_scaled(&time_str, effective_font_size, x_scale);
            let x = (width as i32 - text_width) / 2;
            layout.push(Widget {
                id: "time".into(),
                rect: Rect {
                    x,
                    y,
                    w: text_width.max(0) as u32,
                    h: canvas.line_height(effective_font_size).max(0) as u32,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content: WidgetContent::TextScaled {
                    x,
                    y,
                    text: time_str.clone(),
                    size: effective_font_size,
                    color: colors.text,
                    x_scale,
                },
            });
        } else {
            Self::push_centered_text(
                layout,
                canvas,
                "time",
                y,
                &time_str,
                effective_font_size,
                colors.text,
            );
        }
        y += time_height + 4;

        if clock.show_date {
            if let Some(date) = &clock.date {
                Self::push_centered_text(layout, canvas, "date", y, date, FONT_NORMAL, colors.dim);
            }
        }
    }

    /// Builds the analog-clock widgets (hostname, bezel + tick/hand geometry
    /// from [`Self::analog_clock_draws`], and the optional date).
    fn build_analog_clock(
        layout: &mut Layout,
        canvas: &Canvas,
        hour: u8,
        minute: u8,
        clock: &ClockLayout,
        colors: &FaceColors,
    ) {
        let (width, height) = canvas.dimensions();
        let margin = 10;

        // Calculate space for complications
        let hostname_height = if clock.show_hostname {
            canvas.line_height(FONT_SMALL) + 4
        } else {
            0
        };
        let date_height = if clock.show_date && clock.date.is_some() {
            canvas.line_height(FONT_NORMAL) + 8
        } else {
            0
        };

        // Calculate clock size based on available space
        let available_height = height as i32 - margin * 2 - hostname_height - date_height;
        let available_width = width as i32 - margin * 2;
        let radius = (available_height.min(available_width) / 2) as u32;

        // Calculate total content height and center vertically
        let total_height = hostname_height + (radius as i32 * 2) + date_height;
        let start_y = (height as i32 - total_height) / 2;

        let cx = width as i32 / 2;
        let mut y = start_y;

        // Draw hostname above
        if clock.show_hostname {
            Self::push_centered_text(
                layout,
                canvas,
                "hostname",
                y,
                &clock.hostname,
                FONT_SMALL,
                colors.dim,
            );
            y += hostname_height;
        }

        // Draw clock face (cy is center of clock)
        let cy = y + radius as i32;
        for (i, draw) in Self::analog_clock_draws(cx, cy, radius, hour, minute, colors)
            .into_iter()
            .enumerate()
        {
            let (id, content) = Self::analog_draw_to_widget(draw, i);
            layout.push(Widget {
                id: id.into(),
                rect: Rect {
                    x: cx - radius as i32,
                    y: cy - radius as i32,
                    w: radius * 2,
                    h: radius * 2,
                },
                kind: ZoneKind::Dynamic,
                cadence: Cadence::Seconds(60),
                content,
            });
        }

        // Draw date below
        if clock.show_date {
            if let Some(date) = &clock.date {
                let date_y = cy + radius as i32 + 8;
                Self::push_centered_text(
                    layout,
                    canvas,
                    "date",
                    date_y,
                    date,
                    FONT_NORMAL,
                    colors.text,
                );
            }
        }
    }

    /// Builds the typed-widget layout, covering ALL configs (digital and analog
    /// time modes), drawn by [`crate::faces::layout::render_layout`].
    fn build_layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        comp: &EnabledComplications,
    ) -> Layout {
        let colors = FaceColors::from_theme(theme);
        let is_on = |id: &str| comp.is_enabled(self.name(), id, false);

        // Get date format option
        let date_format = comp
            .get_option(
                self.name(),
                complication_names::DATE,
                complication_options::DATE_FORMAT,
            )
            .map(|s| s.as_str())
            .unwrap_or(date_formats::SHORT);

        // Get digital time size option
        let time_size = comp
            .get_option(self.name(), "digital_time", complication_options::SIZE)
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(DEFAULT_TIME_SIZE);

        // Build layout options
        let clock = ClockLayout {
            show_hostname: is_on("hostname"),
            show_date: is_on(complication_names::DATE),
            hostname: data.hostname.clone(),
            date: data.format_date(date_format),
        };

        let mut layout = Layout::new();
        if is_on("digital_time") {
            Self::build_digital_time(
                &mut layout,
                canvas,
                data.hour,
                data.minute,
                &clock,
                &colors,
                time_size,
            );
        } else {
            Self::build_analog_clock(&mut layout, canvas, data.hour, data.minute, &clock, &colors);
        }
        layout
    }
}

impl Default for ClockFace {
    fn default() -> Self {
        Self::new()
    }
}

impl Face for ClockFace {
    fn name(&self) -> &str {
        "clock"
    }

    fn available_complications(&self) -> Vec<Complication> {
        vec![
            complications::hostname(false),
            complications::digital_time(false),
            complications::date(false, date_formats::SHORT),
        ]
    }

    fn layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        comp: &EnabledComplications,
    ) -> Layout {
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

    // Deterministic sample data so both render paths see identical input.
    // A fixed 10:08 places the analog hands off-axis (deterministic trig).
    #[allow(clippy::field_reassign_with_default)]
    fn sample() -> SystemData {
        let mut d = SystemData::default();
        d.hostname = "endeavour".into();
        d.hour = 10;
        d.minute = 8;
        d
    }

    /// Default complications: digital_time OFF (analog), hostname OFF, date OFF.
    fn render_both(width: u32, height: u32) -> Vec<u8> {
        render_both_comps(width, height, sample(), EnabledComplications::new())
    }

    fn render_both_comps(
        width: u32,
        height: u32,
        data: SystemData,
        comps: EnabledComplications,
    ) -> Vec<u8> {
        let face = ClockFace::new();
        let theme = Theme::from_preset("default");

        let mut via_layout = Canvas::new(width, height);
        let lay = face.layout(&via_layout, &data, &theme, &comps);
        via_layout.set_background(0);
        via_layout.clear();
        render_layout(&mut via_layout, &lay);

        via_layout.pixels().to_vec()
    }

    /// Complications with the digital_time toggle enabled (digital mode).
    fn digital_comps() -> EnabledComplications {
        let mut comps = EnabledComplications::new();
        comps.set_enabled("clock", "digital_time", true);
        comps
    }

    /// Digital mode plus hostname and date complications enabled.
    fn digital_full_comps() -> EnabledComplications {
        let mut comps = digital_comps();
        comps.set_enabled("clock", "hostname", true);
        comps.set_enabled("clock", complication_names::DATE, true);
        comps
    }

    /// Analog mode plus hostname and date complications enabled.
    fn analog_full_comps() -> EnabledComplications {
        let mut comps = EnabledComplications::new();
        comps.set_enabled("clock", "hostname", true);
        comps.set_enabled("clock", complication_names::DATE, true);
        comps
    }

    /// Digital mode with a large font size to exercise the portrait font-scaling
    /// branch (size in 56..=96), plus hostname + date occupying vertical space.
    fn digital_scaled_comps() -> EnabledComplications {
        let mut comps = digital_full_comps();
        comps.set_option(
            "clock",
            "digital_time",
            complication_options::SIZE,
            "72".to_string(),
        );
        comps
    }

    /// Sample with a concrete date set. The clock's default date format is SHORT
    /// ("Jan 15"), so this exercises a realistic non-default date string.
    #[allow(clippy::field_reassign_with_default)]
    fn sample_with_date() -> SystemData {
        let mut d = sample();
        d.year = 2026;
        d.month = 1;
        d.day = 15;
        d.day_of_week = 3;
        d
    }

    // --- Analog mode (default), both orientations ---

    #[test]
    fn clock_layout_matches_render_landscape_analog() {
        let via_layout = render_both(320, 170);
        assert_eq!(
            pixel_hash(&via_layout),
            13417756890114301721,
            "golden drift: clock_layout_matches_render_landscape_analog"
        );
    }

    #[test]
    fn clock_layout_matches_render_portrait_analog() {
        let via_layout = render_both(170, 320);
        assert_eq!(
            pixel_hash(&via_layout),
            479876432683237793,
            "golden drift: clock_layout_matches_render_portrait_analog"
        );
    }

    // --- Analog mode with hostname + date complications ---

    #[test]
    fn clock_layout_matches_render_landscape_analog_full() {
        let via_layout = render_both_comps(320, 170, sample_with_date(), analog_full_comps());
        assert_eq!(
            pixel_hash(&via_layout),
            3888933470548481487,
            "golden drift: clock_layout_matches_render_landscape_analog_full"
        );
    }

    #[test]
    fn clock_layout_matches_render_portrait_analog_full() {
        let via_layout = render_both_comps(170, 320, sample_with_date(), analog_full_comps());
        assert_eq!(
            pixel_hash(&via_layout),
            14494316605444689504,
            "golden drift: clock_layout_matches_render_portrait_analog_full"
        );
    }

    // --- Digital mode, both orientations ---

    #[test]
    fn clock_layout_matches_render_landscape_digital() {
        let via_layout = render_both_comps(320, 170, sample(), digital_comps());
        assert_eq!(
            pixel_hash(&via_layout),
            5840807577594969307,
            "golden drift: clock_layout_matches_render_landscape_digital"
        );
    }

    #[test]
    fn clock_layout_matches_render_portrait_digital() {
        let via_layout = render_both_comps(170, 320, sample(), digital_comps());
        assert_eq!(
            pixel_hash(&via_layout),
            15006201191712943095,
            "golden drift: clock_layout_matches_render_portrait_digital"
        );
    }

    // --- Digital mode with hostname + date complications ---

    #[test]
    fn clock_layout_matches_render_landscape_digital_full() {
        let via_layout = render_both_comps(320, 170, sample_with_date(), digital_full_comps());
        assert_eq!(
            pixel_hash(&via_layout),
            1161514971122122895,
            "golden drift: clock_layout_matches_render_landscape_digital_full"
        );
    }

    #[test]
    fn clock_layout_matches_render_portrait_digital_full() {
        let via_layout = render_both_comps(170, 320, sample_with_date(), digital_full_comps());
        assert_eq!(
            pixel_hash(&via_layout),
            3761932041278855023,
            "golden drift: clock_layout_matches_render_portrait_digital_full"
        );
    }

    // --- Portrait digital with large font size: exercises the TextScaled /
    //     font-scaling branch (size 56..=96). ---

    #[test]
    fn clock_layout_matches_render_portrait_digital_scaled() {
        let via_layout = render_both_comps(170, 320, sample_with_date(), digital_scaled_comps());
        assert_eq!(
            pixel_hash(&via_layout),
            4076598659137079727,
            "golden drift: clock_layout_matches_render_portrait_digital_scaled"
        );
    }
}
