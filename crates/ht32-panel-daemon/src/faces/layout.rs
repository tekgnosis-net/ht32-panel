//! Typed-widget Layout model for display faces.
//!
//! A face produces a `Layout` (a list of `Widget`s). `render_layout` interprets
//! the widgets into `Canvas` drawing calls. `rect`/`kind`/`cadence` describe each
//! widget's screen region and update policy; they are unused in this phase and
//! consumed by the per-zone scheduler and partial-update transport in later phases.

use crate::rendering::Canvas;
use serde::{Deserialize, Serialize};

/// Bounding box of a widget, in canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Whether a widget is drawn once (static) or updated over time (dynamic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneKind {
    Static,
    Dynamic,
}

/// How often a dynamic widget refreshes (consumed by the Phase 3 scheduler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    EveryFrame,
    Seconds(u32),
    OnChange,
}

/// The drawable content of a widget. Only the kinds the current faces need.
#[derive(Debug, Clone)]
pub enum WidgetContent {
    Text {
        text: String,
        x: i32,
        y: i32,
        size: f32,
        color: u32,
    },
    TextScaled {
        x: i32,
        y: i32,
        text: String,
        size: f32,
        color: u32,
        x_scale: f32,
    },
    Bar {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        percent: f64,
        fill: u32,
        bg: u32,
    },
    DualSparkline {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        a: Vec<f64>,
        b: Vec<f64>,
        scale: f64,
        color_a: u32,
        color_b: u32,
        bg: u32,
        /// When true, routes to the oscilloscope wrap renderer; false uses the legacy scroller.
        wrap_around: bool,
        /// Total samples ever pushed; passed to `draw_dual_graph_wrap` as the write-head index.
        /// Unused on the legacy path (`wrap_around: false`); set to 0 until Task 4 wires it up.
        count: u64,
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
    Arc {
        cx: i32,
        cy: i32,
        r: u32,
        start_angle: f32,
        end_angle: f32,
        stroke: f32,
        color: u32,
    },
}

/// A positioned, typed widget with update metadata.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Widget {
    pub id: &'static str,
    pub rect: Rect,
    pub kind: ZoneKind,
    pub cadence: Cadence,
    pub content: WidgetContent,
}

/// An ordered list of widgets composing a face.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub widgets: Vec<Widget>,
}

impl Layout {
    pub fn new() -> Self {
        Self {
            widgets: Vec::new(),
        }
    }
    pub fn push(&mut self, widget: Widget) {
        self.widgets.push(widget);
    }
}

/// Draws every widget of `layout` onto `canvas`, in order.
pub fn render_layout(canvas: &mut Canvas, layout: &Layout) {
    for w in &layout.widgets {
        match &w.content {
            WidgetContent::Text {
                text,
                x,
                y,
                size,
                color,
            } => {
                canvas.draw_text(*x, *y, text, *size, *color);
            }
            WidgetContent::TextScaled {
                x,
                y,
                text,
                size,
                color,
                x_scale,
            } => {
                canvas.draw_text_scaled(*x, *y, text, *size, *color, *x_scale);
            }
            WidgetContent::Bar {
                x,
                y,
                w: bw,
                h: bh,
                percent,
                fill,
                bg,
            } => {
                canvas.fill_rect(*x, *y, *bw, *bh, *bg);
                let fill_w = ((*bw as f64 * (percent / 100.0)) as u32).min(*bw);
                if fill_w > 0 {
                    canvas.fill_rect(*x, *y, fill_w, *bh, *fill);
                }
            }
            WidgetContent::DualSparkline {
                x,
                y,
                w: gw,
                h: gh,
                a,
                b,
                scale,
                color_a,
                color_b,
                bg,
                wrap_around,
                count,
            } => {
                if *wrap_around {
                    canvas.draw_dual_graph_wrap(
                        *x, *y, *gw, *gh, a, b, *count, *scale, *color_a, *color_b, *bg,
                    );
                } else {
                    canvas.draw_dual_graph(*x, *y, *gw, *gh, a, b, *scale, *color_a, *color_b, *bg);
                }
            }
            WidgetContent::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                color,
            } => canvas.draw_line(*x1, *y1, *x2, *y2, *stroke, *color),
            WidgetContent::Circle { cx, cy, r, color } => canvas.fill_circle(*cx, *cy, *r, *color),
            WidgetContent::Arc {
                cx,
                cy,
                r,
                start_angle,
                end_angle,
                stroke,
                color,
            } => canvas.draw_arc(*cx, *cy, *r, *start_angle, *end_angle, *stroke, *color),
        }
    }
}

/// Deterministic, dependency-free FNV-1a hash over RGBA pixel bytes.
///
/// Shared by the per-face golden-snapshot tests so a single pinned `u64`
/// per (face, config) guards against rendering regressions without needing
/// the retired `render()` path as a reference.
#[cfg(test)]
pub(crate) fn pixel_hash(px: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in px {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::Canvas;

    #[test]
    fn render_layout_draws_line_circle_arc() {
        let mut canvas = Canvas::new(60, 60);
        canvas.set_background(0x000000);
        canvas.clear();
        let mut layout = Layout::new();
        layout.push(Widget {
            id: "ln",
            rect: Rect {
                x: 0,
                y: 0,
                w: 60,
                h: 60,
            },
            kind: ZoneKind::Static,
            cadence: Cadence::OnChange,
            content: WidgetContent::Line {
                x1: 5,
                y1: 30,
                x2: 55,
                y2: 30,
                stroke: 3.0,
                color: 0xFFFFFF,
            },
        });
        layout.push(Widget {
            id: "ci",
            rect: Rect {
                x: 0,
                y: 0,
                w: 60,
                h: 60,
            },
            kind: ZoneKind::Static,
            cadence: Cadence::OnChange,
            content: WidgetContent::Circle {
                cx: 30,
                cy: 30,
                r: 8,
                color: 0xFF0000,
            },
        });
        render_layout(&mut canvas, &layout);
        let px = canvas.pixels();
        let at = |x: usize, y: usize| {
            let i = (y * 60 + x) * 4;
            (px[i], px[i + 1], px[i + 2])
        };
        assert_eq!(at(30, 30), (255, 0, 0), "circle center red");
        // Positive line check: a pixel on the line but left of the circle (which
        // overdraws the shared center at x=30) proves draw_line actually painted.
        assert_eq!(
            at(10, 30),
            (255, 255, 255),
            "line pixel left of circle is white"
        );
        assert_eq!(at(30, 5), (0, 0, 0), "above the line stays bg");
    }

    #[test]
    fn render_layout_draws_text_scaled() {
        // A TextScaled widget must forward to canvas.draw_text_scaled and paint
        // non-background pixels. Use a wide-enough canvas so the scaled glyphs fit
        // within the debug_assert bounds.
        let mut canvas = Canvas::new(120, 40);
        canvas.set_background(0x000000);
        canvas.clear();
        let mut layout = Layout::new();
        layout.push(Widget {
            id: "ts",
            rect: Rect {
                x: 0,
                y: 0,
                w: 120,
                h: 40,
            },
            kind: ZoneKind::Static,
            cadence: Cadence::OnChange,
            content: WidgetContent::TextScaled {
                x: 4,
                y: 4,
                text: "88:88".to_string(),
                size: 24.0,
                color: 0xFFFFFF,
                x_scale: 0.5,
            },
        });
        render_layout(&mut canvas, &layout);
        let px = canvas.pixels();
        // At least one painted pixel must be non-background, proving the scaled
        // text was drawn into the canvas.
        let any_painted = px
            .chunks_exact(4)
            .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0);
        assert!(
            any_painted,
            "TextScaled widget should paint non-background pixels"
        );
    }

    #[test]
    fn render_layout_draws_arc() {
        // Arc: center (30,30), radius 10, stroke 3. Total extent = 10 + ceil(1.5) = 12.
        // Canvas 60x60 ensures the arc fits entirely on-screen (30-12=18 >= 0).
        // We draw a quarter-arc from 0 to PI/2 (right to bottom in screen coords).
        // A pixel at (40, 30) — i.e. at (cx+r, cy) — should be on the arc path.
        // The center (30, 30) itself should remain background (draw_arc is stroke-only).
        use std::f32::consts::PI;
        let mut canvas = Canvas::new(60, 60);
        canvas.set_background(0x000000);
        canvas.clear();
        let mut layout = Layout::new();
        layout.push(Widget {
            id: "arc",
            rect: Rect {
                x: 0,
                y: 0,
                w: 60,
                h: 60,
            },
            kind: ZoneKind::Static,
            cadence: Cadence::OnChange,
            content: WidgetContent::Arc {
                cx: 30,
                cy: 30,
                r: 10,
                start_angle: 0.0,
                end_angle: 2.0 * PI,
                stroke: 3.0,
                color: 0x00FF00,
            },
        });
        render_layout(&mut canvas, &layout);
        let px = canvas.pixels();
        let at = |x: usize, y: usize| {
            let i = (y * 60 + x) * 4;
            (px[i], px[i + 1], px[i + 2])
        };
        // A pixel on the rightmost arc edge should be green (non-background)
        assert_ne!(
            at(40, 30),
            (0, 0, 0),
            "arc path pixel should be non-background"
        );
        // The center should remain background (arc is stroke-only, not filled)
        assert_eq!(at(30, 30), (0, 0, 0), "arc center stays background");
    }

    #[test]
    fn render_layout_draws_bar_into_canvas() {
        let mut canvas = Canvas::new(60, 20);
        canvas.set_background(0x000000);
        canvas.clear();
        let mut layout = Layout::new();
        layout.push(Widget {
            id: "bar",
            rect: Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 8,
            },
            kind: ZoneKind::Dynamic,
            cadence: Cadence::EveryFrame,
            content: WidgetContent::Bar {
                x: 0,
                y: 0,
                w: 40,
                h: 8,
                percent: 50.0,
                fill: 0xFFFFFF,
                bg: 0x202020,
            },
        });
        render_layout(&mut canvas, &layout);
        // Left half (filled) is white; right half is the bar background.
        let px = canvas.pixels(); // RGBA8, row-major, width*height*4
        let at = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 60 + x) * 4;
            (px[i], px[i + 1], px[i + 2])
        };
        assert_eq!(at(2, 4), (255, 255, 255), "filled portion white");
        assert_eq!(at(38, 4), (0x20, 0x20, 0x20), "unfilled portion = bar bg");
    }

    /// Shared test data for the dual-sparkline dispatch tests.
    fn dual_sparkline_data() -> (Vec<f64>, Vec<f64>) {
        let a: Vec<f64> = (0..60).map(|i| (i % 10) as f64).collect();
        let b: Vec<f64> = (0..60).map(|i| ((i + 3) % 10) as f64).collect();
        (a, b)
    }

    /// `wrap_around: true` routes to `draw_dual_graph_wrap` — pixel output must equal a
    /// direct call with the same arguments.
    #[test]
    fn dual_sparkline_wrap_true_routes_to_wrap_renderer() {
        let (a, b) = dual_sparkline_data();
        let count: u64 = 100;
        let scale = 9.0_f64;
        let color_a = 0xFF0000_u32;
        let color_b = 0x00FF00_u32;
        let bg = 0x000000_u32;
        let w = 60_u32;
        let h = 20_u32;

        // Render via render_layout with wrap_around: true.
        let mut via_layout = Canvas::new(w, h);
        via_layout.set_background(bg);
        via_layout.clear();
        let mut layout = Layout::new();
        layout.push(Widget {
            id: "ds",
            rect: Rect { x: 0, y: 0, w, h },
            kind: ZoneKind::Dynamic,
            cadence: Cadence::EveryFrame,
            content: WidgetContent::DualSparkline {
                x: 0,
                y: 0,
                w,
                h,
                a: a.clone(),
                b: b.clone(),
                scale,
                color_a,
                color_b,
                bg,
                wrap_around: true,
                count,
            },
        });
        render_layout(&mut via_layout, &layout);

        // Render directly via draw_dual_graph_wrap with identical args.
        let mut direct = Canvas::new(w, h);
        direct.set_background(bg);
        direct.clear();
        direct.draw_dual_graph_wrap(0, 0, w, h, &a, &b, count, scale, color_a, color_b, bg);

        assert_eq!(
            via_layout.pixels(),
            direct.pixels(),
            "wrap_around: true must route to draw_dual_graph_wrap"
        );
    }

    /// `wrap_around: false` routes to the legacy `draw_dual_graph` — pixel output must
    /// equal a direct call with the same arguments.
    #[test]
    fn dual_sparkline_wrap_false_routes_to_legacy_renderer() {
        let (a, b) = dual_sparkline_data();
        let scale = 9.0_f64;
        let color_a = 0xFF0000_u32;
        let color_b = 0x00FF00_u32;
        let bg = 0x000000_u32;
        let w = 60_u32;
        let h = 20_u32;

        // Render via render_layout with wrap_around: false.
        let mut via_layout = Canvas::new(w, h);
        via_layout.set_background(bg);
        via_layout.clear();
        let mut layout = Layout::new();
        layout.push(Widget {
            id: "ds",
            rect: Rect { x: 0, y: 0, w, h },
            kind: ZoneKind::Dynamic,
            cadence: Cadence::EveryFrame,
            content: WidgetContent::DualSparkline {
                x: 0,
                y: 0,
                w,
                h,
                a: a.clone(),
                b: b.clone(),
                scale,
                color_a,
                color_b,
                bg,
                wrap_around: false,
                count: 0,
            },
        });
        render_layout(&mut via_layout, &layout);

        // Render directly via draw_dual_graph with identical args.
        let mut direct = Canvas::new(w, h);
        direct.set_background(bg);
        direct.clear();
        direct.draw_dual_graph(0, 0, w, h, &a, &b, scale, color_a, color_b, bg);

        assert_eq!(
            via_layout.pixels(),
            direct.pixels(),
            "wrap_around: false must route to legacy draw_dual_graph"
        );
    }
}
