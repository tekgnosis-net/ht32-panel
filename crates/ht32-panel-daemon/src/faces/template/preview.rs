//! Pure server-truth preview: render a draft `TemplateSpec` off-screen through the
//! real `render_layout`, and report layout warnings — without touching the live device.

use crate::faces::layout::{render_layout, Layout, WidgetContent};
use crate::faces::template::spec::TemplateSpec;
use crate::faces::{EnabledComplications, Face, TemplateFace, Theme};
use crate::rendering::Canvas;
use crate::sensors::data::SystemData;
use ht32_panel_hw::Orientation;

/// A non-blocking layout warning surfaced in the editor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Warning {
    pub widget_id: String,
    pub message: String,
}

/// A fixed, representative `SystemData` so previews are deterministic and populated.
#[allow(clippy::field_reassign_with_default)]
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

/// Renders a draft spec off-screen through the REAL renderer and returns
/// `(png_bytes, warnings)`. Never touches the live device or active face.
///
/// Widgets that extend outside the canvas are warned about but excluded from
/// rendering (so the renderer never draws out-of-bounds).
pub fn preview_render(
    spec: &TemplateSpec,
    theme: &Theme,
    orientation: Orientation,
) -> (Vec<u8>, Vec<Warning>) {
    let (w, h) = orientation.dimensions();
    let (w, h) = (w as u32, h as u32);
    let reference = Canvas::new(w, h);
    let comps = EnabledComplications::new();
    let layout = TemplateFace::new(spec.clone()).layout(&reference, &sample_data(), theme, &comps);
    let warnings = check_bounds(&layout, &reference);

    // Exclude out-of-bounds widgets from the render pass so the renderer
    // never tries to draw past the canvas edge.
    let warned_ids: std::collections::HashSet<&str> =
        warnings.iter().map(|w| w.widget_id.as_str()).collect();
    let render_layout_filtered = Layout {
        widgets: layout
            .widgets
            .into_iter()
            .filter(|w| !warned_ids.contains(w.id.as_ref()))
            .collect(),
    };

    let mut canvas = Canvas::new(w, h);
    canvas.set_background(theme.background);
    canvas.clear();
    render_layout(&mut canvas, &render_layout_filtered);

    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().expect("png header");
        writer.write_image_data(canvas.pixels()).expect("png data");
    }
    (png, warnings)
}

/// Reports widgets whose rect leaves the canvas, or whose resolved text is wider
/// than its rect. Computed in the same units the renderer draws in, so a warning
/// can never disagree with the rendered pixels.
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

    #[test]
    fn preview_render_paints_and_reports() {
        use crate::faces::template::spec::*;
        use crate::faces::Theme;
        use ht32_panel_hw::Orientation;
        // One in-bounds bar (paints) + one off-canvas text (warns).
        let spec = TemplateSpec {
            name: "p".into(),
            orientation: None,
            theme: None,
            widgets: vec![
                TemplateWidget {
                    id: "bar".into(),
                    rect: Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 10,
                    },
                    content: TemplateContent::Bar {
                        value: NumberSource::CpuPercent,
                        fill: ColorRef::Hex(0xFFFFFF),
                        bg: ColorRef::Hex(0x000000),
                    },
                },
                TemplateWidget {
                    id: "off".into(),
                    rect: Rect {
                        x: 300,
                        y: 0,
                        w: 80,
                        h: 16,
                    },
                    content: TemplateContent::Text {
                        value: TextSource::Hostname,
                        size: 12.0,
                        color: ColorRef::Hex(0xFFFFFF),
                        align: Align::Left,
                    },
                },
            ],
        };
        let theme = Theme::from_preset("nord");
        let (png, warns) = preview_render(&spec, &theme, Orientation::Landscape);
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "valid PNG header"
        );
        assert!(
            warns.iter().any(|w| w.widget_id == "off"),
            "off-canvas widget warns"
        );
    }

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
