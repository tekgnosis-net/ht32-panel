//! Pure server-truth preview: render a draft `TemplateSpec` off-screen through the
//! real `render_layout`, and report layout warnings — without touching the live device.

use crate::faces::layout::{Layout, WidgetContent};
use crate::rendering::Canvas;
use crate::sensors::data::SystemData;

/// A non-blocking layout warning surfaced in the editor.
#[derive(Debug, Clone, serde::Serialize)]
#[allow(dead_code)]
pub struct Warning {
    pub widget_id: String,
    pub message: String,
}

/// A fixed, representative `SystemData` so previews are deterministic and populated.
#[allow(clippy::field_reassign_with_default)]
#[allow(dead_code)]
pub fn sample_data() -> SystemData {
    let mut d = SystemData::default();
    d.hostname = "preview-host".into();
    d.cpu_percent = 65.0;
    d.ram_percent = 80.0;
    d.cpu_temp = Some(72.0);
    d.hour = 14;
    d.minute = 30;
    d.day = 21;
    d.month = 6;
    d.year = 2026;
    d.disk_read_rate = 5_000_000.0;
    d.disk_write_rate = 1_000_000.0;
    d.net_rx_rate = 2_000_000.0;
    d.net_tx_rate = 500_000.0;
    d.disk_sample_count = 60;
    d.net_sample_count = 60;
    for i in 0..60u64 {
        let v = (i as f64) * 100_000.0;
        d.disk_history.push_back(v);
        d.disk_read_history.push_back(v);
        d.disk_write_history.push_back(v / 2.0);
        d.net_history.push_back(v);
        d.net_rx_history.push_back(v);
        d.net_tx_history.push_back(v / 4.0);
    }
    d
}

/// Reports widgets whose rect leaves the canvas, or whose resolved text is wider
/// than its rect. Computed in the same units the renderer draws in, so a warning
/// can never disagree with the rendered pixels.
#[allow(dead_code)]
pub fn check_bounds(layout: &Layout, canvas: &Canvas) -> Vec<Warning> {
    let (cw, ch) = canvas.dimensions();
    let (cw, ch) = (cw as i32, ch as i32);
    let mut out = Vec::new();
    for w in &layout.widgets {
        let r = &w.rect;
        if r.x < 0 || r.y < 0 || r.x + r.w as i32 > cw || r.y + r.h as i32 > ch {
            out.push(Warning {
                widget_id: w.id.to_string(),
                message: format!("widget '{}' extends outside the {}×{} screen", w.id, cw, ch),
            });
            continue; // one warning per widget is enough
        }
        if let WidgetContent::Text { text, size, .. } = &w.content {
            if canvas.text_width(text, *size) > r.w as i32 {
                out.push(Warning {
                    widget_id: w.id.to_string(),
                    message: format!("text in '{}' is wider than its box", w.id),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::layout::{Cadence, Layout, Rect, Widget, WidgetContent, ZoneKind};
    use crate::rendering::Canvas;
    use std::borrow::Cow;

    // NOTE: The brief's test helper references `align: Align::Left` and `crate::faces::Align`,
    // but the actual `WidgetContent::Text` variant has fields `{text, x, y, size, color}` —
    // there is no `align` field in the real type. The helper is adjusted to match the real shape.
    // `x` and `y` within the content are set to the rect origin; `color` is kept as 0xFFFFFF.
    fn text_widget(id: &str, rect: Rect, text: &str, size: f32) -> Widget {
        Widget {
            id: Cow::Owned(id.into()),
            rect,
            kind: ZoneKind::Dynamic,
            cadence: Cadence::EveryFrame,
            content: WidgetContent::Text {
                text: text.into(),
                x: rect.x,
                y: rect.y,
                size,
                color: 0xFFFFFF,
            },
        }
    }

    #[test]
    fn rect_off_canvas_warns() {
        let canvas = Canvas::new(170, 320);
        let layout = Layout {
            widgets: vec![text_widget(
                "over",
                Rect {
                    x: 160,
                    y: 4,
                    w: 40,
                    h: 16,
                },
                "hi",
                12.0,
            )],
        };
        let warns = check_bounds(&layout, &canvas);
        assert!(warns.iter().any(|w| w.widget_id == "over"));
    }

    #[test]
    fn fitting_widget_no_warn() {
        let canvas = Canvas::new(170, 320);
        let layout = Layout {
            widgets: vec![text_widget(
                "ok",
                Rect {
                    x: 4,
                    y: 4,
                    w: 80,
                    h: 16,
                },
                "hi",
                12.0,
            )],
        };
        assert!(check_bounds(&layout, &canvas).is_empty());
    }

    #[test]
    fn text_wider_than_rect_warns() {
        let canvas = Canvas::new(170, 320);
        // A long string in a narrow rect: measured width should exceed rect.w.
        let layout = Layout {
            widgets: vec![text_widget(
                "wide",
                Rect {
                    x: 4,
                    y: 4,
                    w: 20,
                    h: 16,
                },
                "a very long label that will not fit",
                14.0,
            )],
        };
        assert!(check_bounds(&layout, &canvas)
            .iter()
            .any(|w| w.widget_id == "wide"));
    }

    #[test]
    fn sample_data_has_full_histories() {
        let d = sample_data();
        assert!(d.disk_history.len() >= 60);
        assert!(d.net_rx_history.len() >= 60);
        assert!(!d.hostname.is_empty());
    }
}
