//! Pure server-truth preview: render a draft `TemplateSpec` off-screen through the
//! real `render_layout`, and report layout warnings — without touching the live device.

use crate::faces::layout::{render_layout, Layout, Widget, WidgetContent};
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

/// Returns true if the primitive extent of `w` would draw outside `canvas` —
/// mirroring the exact conditions of `draw_arc`/`fill_circle`'s `debug_assert!`s.
/// Used by both `overflows_canvas` (filter) and `check_bounds` (warn).
fn widget_primitive_overflows(w: &Widget, cw: i32, ch: i32) -> bool {
    match &w.content {
        WidgetContent::Arc {
            cx, cy, r, stroke, ..
        } => {
            let total = *r as i32 + (*stroke / 2.0).ceil() as i32;
            cx - total < 0 || cy - total < 0 || cx + total > cw || cy + total > ch
        }
        WidgetContent::Circle { cx, cy, r, .. } => {
            let ri = *r as i32;
            cx - ri < 0 || cy - ri < 0 || cx + ri > cw || cy + ri > ch
        }
        _ => false,
    }
}

/// True if rendering `w` would draw outside the canvas — exactly the condition
/// that trips render_layout's `debug_assert!`s (rect off-canvas, Text/TextScaled
/// whose drawn extent exceeds the canvas edge, or Arc/Circle whose geometric
/// extent exceeds the canvas edge). `preview_render` filters these so a draft
/// with off-canvas content does not panic in debug builds; on release hardware
/// such content is clipped, so omitting it in the preview is the accepted, warned
/// divergence. Text that overflows its RECT but stays within the canvas is NOT
/// filtered — it renders truthfully, matching hardware.
fn overflows_canvas(w: &Widget, canvas: &Canvas) -> bool {
    let (cw, ch) = canvas.dimensions();
    let (cw, ch) = (cw as i32, ch as i32);
    let r = &w.rect;
    // rect-based widgets: rect must be fully on-canvas.
    if r.x < 0 || r.y < 0 || r.x + r.w as i32 > cw || r.y + r.h as i32 > ch {
        return true;
    }
    // Arc and Circle: check geometric extent against canvas bounds.
    if widget_primitive_overflows(w, cw, ch) {
        return true;
    }
    // Text additionally asserts on the measured text extent (draw_text panics
    // if x + text_width > canvas width, or y + line_height > canvas height).
    match &w.content {
        WidgetContent::Text {
            text, x, y, size, ..
        } => {
            *x < 0
                || *y < 0
                || *x + canvas.text_width(text, *size) > cw
                || *y + canvas.line_height(*size) > ch
        }
        WidgetContent::TextScaled {
            x,
            y,
            text,
            size,
            x_scale,
            ..
        } => {
            *x < 0
                || *y < 0
                || *x + canvas.text_width_scaled(text, *size, *x_scale) > cw
                || *y + canvas.line_height(*size) > ch
        }
        _ => false,
    }
}

/// Renders a draft spec off-screen through the REAL renderer and returns
/// `(png_bytes, warnings)`. Never touches the live device or active face.
///
/// Widgets that extend outside the canvas are warned about but excluded from
/// rendering (so the renderer never draws out-of-bounds). Text that overflows
/// its rect but stays within the canvas is warned but NOT filtered — it renders
/// truthfully, matching hardware behavior.
pub fn preview_render(
    spec: &TemplateSpec,
    theme: &Theme,
    orientation: Orientation,
) -> (Vec<u8>, Vec<Warning>) {
    let (w, h) = orientation.dimensions();
    let (w, h) = (w as u32, h as u32);
    let reference = Canvas::new(w, h);
    let comps = EnabledComplications::new();
    let face = TemplateFace::new(spec.clone());
    let layout = face.layout(&reference, &sample_data(), theme, &comps);
    let warnings = check_bounds(&layout, &reference);

    // Exclude only widgets whose drawn extent would overflow the canvas.
    // This is the exact predicate that triggers render_layout's debug_assert!s.
    // Text that overflows its rect but fits within the canvas is kept (hardware renders it).
    let render_layout_filtered = Layout {
        widgets: layout
            .widgets
            .into_iter()
            .filter(|w| !overflows_canvas(w, &reference))
            .collect(),
    };

    let mut canvas = Canvas::new(w, h);
    let bg = face.background(theme).unwrap_or(theme.background);
    canvas.set_background(bg);
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

/// Reports widgets whose rect leaves the canvas, whose Arc/Circle extent leaves
/// the canvas, or whose resolved text is wider than its rect. Computed in the same
/// units the renderer draws in, so a warning can never disagree with the rendered pixels.
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
        // Arc and Circle: check primitive draw extent against canvas bounds.
        if widget_primitive_overflows(w, cw, ch) {
            out.push(Warning {
                widget_id: w.id.to_string(),
                message: format!("widget '{}' draws outside the {}×{} screen", w.id, cw, ch),
            });
            continue;
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
            background: None,
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

    /// Regression: a Text widget whose rect is fully in-bounds but whose text is wider
    /// than its rect (yet the text still fits within the canvas) must:
    ///   (a) produce a `check_bounds` WARNING (text wider than box), AND
    ///   (b) NOT be filtered — `overflows_canvas` returns false, so render_layout draws it.
    ///
    /// Before the fix, the widget was incorrectly dropped because filter was on `warned_ids`.
    #[test]
    fn text_wider_than_rect_but_in_canvas_is_warned_and_not_filtered() {
        // Canvas 170×320. Widget rect at x=4,y=4 w=20 h=16 — fully in-bounds.
        // Text "WWWWWWWWWW" at size 14.0 is measurably wider than rect.w=20,
        // but x=4 + text_width is well within 170px.
        let canvas_ref = Canvas::new(170, 320);
        let widget = text_widget(
            "wide_inbounds",
            Rect {
                x: 4,
                y: 4,
                w: 20,
                h: 16,
            },
            "WWWWWWWWWW",
            14.0,
        );

        // (a) check_bounds must warn.
        let layout = Layout {
            widgets: vec![widget.clone()],
        };
        let warns = check_bounds(&layout, &canvas_ref);
        assert!(
            warns.iter().any(|w| w.widget_id == "wide_inbounds"),
            "check_bounds must warn when text is wider than its rect"
        );

        // (b) overflows_canvas must return false — text extent stays within 170px.
        assert!(
            !overflows_canvas(&widget, &canvas_ref),
            "overflows_canvas must be false: text fits within canvas even though it exceeds rect"
        );

        // (c) End-to-end: render a layout containing only this widget and confirm that
        // render_layout does NOT panic (i.e., the widget was not filtered out and draws fine).
        let mut render_canvas = Canvas::new(170, 320);
        render_canvas.set_background(0x000000);
        render_canvas.clear();
        render_layout(&mut render_canvas, &layout); // must not panic in debug builds
        let px = render_canvas.pixels();
        // At least one non-background pixel in the widget's row (y=4..20) proves it was drawn.
        let row_start = 4 * 170 * 4; // y=4, row of 170 RGBA pixels
        let row_end = 20 * 170 * 4; // up to y=20
        let any_painted = px[row_start..row_end]
            .chunks_exact(4)
            .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0);
        assert!(
            any_painted,
            "widget with text wider-than-rect (but in-canvas) must be rendered, not filtered"
        );
    }

    /// Confirm the off-canvas filter still works: a widget whose rect extends past the
    /// canvas edge is both warned AND filtered (overflows_canvas returns true).
    #[test]
    fn rect_off_canvas_is_warned_and_filtered() {
        let canvas_ref = Canvas::new(170, 320);
        let widget = text_widget(
            "off_canvas",
            Rect {
                x: 160,
                y: 4,
                w: 40, // 160+40=200 > 170 → rect OOB
                h: 16,
            },
            "hi",
            12.0,
        );

        let layout = Layout {
            widgets: vec![widget.clone()],
        };

        // check_bounds warns.
        let warns = check_bounds(&layout, &canvas_ref);
        assert!(
            warns.iter().any(|w| w.widget_id == "off_canvas"),
            "off-canvas widget must produce a warning"
        );

        // overflows_canvas returns true → it would be filtered.
        assert!(
            overflows_canvas(&widget, &canvas_ref),
            "overflows_canvas must be true for a rect that extends past the canvas edge"
        );
    }

    #[test]
    fn sample_data_has_full_histories() {
        let d = sample_data();
        assert!(d.disk_history.len() >= 60);
        assert!(d.net_rx_history.len() >= 60);
        assert!(!d.hostname.is_empty());
    }

    /// Regression: an Arc widget whose rect is in-bounds but whose geometric extent
    /// (cx + radius + ceil(stroke/2)) exceeds the canvas edge must:
    ///   (a) NOT panic in `preview_render` / `render_layout` (previously would panic in
    ///       debug builds because the off-canvas arc slipped through the filter),
    ///   (b) produce a `check_bounds` WARNING for the overflowing widget, AND
    ///   (c) be filtered out by `overflows_canvas` (returns true).
    ///
    /// Setup: 320×170 canvas; Arc at cx=310, cy=85, r=20, stroke=4.0.
    ///   total_radius = 20 + ceil(2.0) = 22; 310 + 22 = 332 > 320 → right-edge overflow.
    ///   The rect is set to cover the full canvas (in-bounds), so only the primitive
    ///   extent check catches it.
    #[test]
    fn arc_primitive_overflow_is_warned_filtered_and_does_not_panic() {
        // Canvas 320×170 (landscape orientation).
        let canvas_ref = Canvas::new(320, 170);
        let arc_widget = Widget {
            id: Cow::Borrowed("arc_oob"),
            rect: Rect {
                x: 0,
                y: 0,
                w: 320,
                h: 170,
            },
            kind: ZoneKind::Dynamic,
            cadence: Cadence::EveryFrame,
            content: WidgetContent::Arc {
                cx: 310,
                cy: 85,
                r: 20,
                start_angle: 0.0,
                end_angle: std::f32::consts::PI * 2.0,
                stroke: 4.0, // total_radius = 20 + ceil(2.0) = 22; 310+22=332 > 320
                color: 0xFF0000,
            },
        };

        // (b) check_bounds must warn.
        let layout = Layout {
            widgets: vec![arc_widget.clone()],
        };
        let warns = check_bounds(&layout, &canvas_ref);
        assert!(
            warns.iter().any(|w| w.widget_id == "arc_oob"),
            "check_bounds must warn when arc primitive extent exceeds the canvas"
        );

        // (c) overflows_canvas must return true.
        assert!(
            overflows_canvas(&arc_widget, &canvas_ref),
            "overflows_canvas must be true when arc+stroke extends past the canvas edge"
        );

        // (a) render_layout must NOT panic when the widget is filtered.
        // Build a filtered layout the same way preview_render does.
        let filtered = Layout {
            widgets: layout
                .widgets
                .into_iter()
                .filter(|w| !overflows_canvas(w, &canvas_ref))
                .collect(),
        };
        let mut render_canvas = Canvas::new(320, 170);
        render_canvas.set_background(0x000000);
        render_canvas.clear();
        render_layout(&mut render_canvas, &filtered); // must not panic
                                                      // The widget was filtered, so the canvas stays black.
        let all_black = render_canvas
            .pixels()
            .chunks_exact(4)
            .all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0);
        assert!(all_black, "filtered arc must leave canvas unchanged");
    }
}
