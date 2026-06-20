//! Typed-widget Layout model for display faces.
//!
//! A face produces a `Layout` (a list of `Widget`s). `render_layout` interprets
//! the widgets into `Canvas` drawing calls. `rect`/`kind`/`cadence` describe each
//! widget's screen region and update policy; they are unused in this phase and
//! consumed by the per-zone scheduler and partial-update transport in later phases.

use crate::rendering::Canvas;

/// Bounding box of a widget, in canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        /// Phase 2 flips this to true; false reproduces the legacy scrolling graph.
        wrap_around: bool,
    },
}

/// A positioned, typed widget with update metadata.
#[derive(Debug, Clone)]
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
        Self { widgets: Vec::new() }
    }
    pub fn push(&mut self, widget: Widget) {
        self.widgets.push(widget);
    }
}

/// Draws every widget of `layout` onto `canvas`, in order.
pub fn render_layout(canvas: &mut Canvas, layout: &Layout) {
    for w in &layout.widgets {
        match &w.content {
            WidgetContent::Text { text, x, y, size, color } => {
                canvas.draw_text(*x, *y, text, *size, *color);
            }
            WidgetContent::Bar { x, y, w: bw, h: bh, percent, fill, bg } => {
                canvas.fill_rect(*x, *y, *bw, *bh, *bg);
                let fill_w = ((*bw as f64 * (percent / 100.0)) as u32).min(*bw);
                if fill_w > 0 {
                    canvas.fill_rect(*x, *y, fill_w, *bh, *fill);
                }
            }
            WidgetContent::DualSparkline { x, y, w: gw, h: gh, a, b, scale, color_a, color_b, bg, wrap_around: _ } => {
                // Phase 1: always the legacy dual graph. Phase 2 adds the wrap-around path.
                let a_deque: std::collections::VecDeque<f64> = a.iter().copied().collect();
                let b_deque: std::collections::VecDeque<f64> = b.iter().copied().collect();
                canvas.draw_dual_graph(*x, *y, *gw, *gh, &a_deque, &b_deque, *scale, *color_a, *color_b, *bg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::Canvas;

    #[test]
    fn render_layout_draws_text_and_bar_into_canvas() {
        let mut canvas = Canvas::new(60, 20);
        canvas.set_background(0x000000);
        canvas.clear();
        let mut layout = Layout::new();
        layout.push(Widget {
            id: "bar", rect: Rect { x: 0, y: 0, w: 40, h: 8 },
            kind: ZoneKind::Dynamic, cadence: Cadence::EveryFrame,
            content: WidgetContent::Bar { x: 0, y: 0, w: 40, h: 8, percent: 50.0, fill: 0xFFFFFF, bg: 0x202020 },
        });
        render_layout(&mut canvas, &layout);
        // Left half (filled) is white; right half is the bar background.
        let px = canvas.pixels(); // RGBA8, row-major, width*height*4
        let at = |x: usize, y: usize| -> (u8,u8,u8) {
            let i = (y * 60 + x) * 4; (px[i], px[i+1], px[i+2])
        };
        assert_eq!(at(2, 4), (255, 255, 255), "filled portion white");
        assert_eq!(at(38, 4), (0x20, 0x20, 0x20), "unfilled portion = bar bg");
    }
}
