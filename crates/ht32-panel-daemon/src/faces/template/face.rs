//! `TemplateFace` — a JSON-defined face that resolves `TemplateSpec` widgets
//! against live `SystemData` every frame.

use std::borrow::Cow;
use std::path::Path;

use tracing::{info, warn};

use super::resolve::resolve;
use super::spec::TemplateSpec;
use crate::faces::layout::{Cadence, Layout, Widget, ZoneKind};
use crate::faces::{Complication, EnabledComplications, Face, Theme};
use crate::rendering::Canvas;
use crate::sensors::data::SystemData;

// ── TemplateFace ─────────────────────────────────────────────────────────────

/// A display face whose widget layout is defined by a [`TemplateSpec`] JSON file.
///
/// Every frame, `layout()` resolves the spec's widget bindings against live
/// `SystemData` and returns a fully-populated [`Layout`].  The existing
/// `render_layout` engine renders it without modification.
pub struct TemplateFace {
    spec: TemplateSpec,
}

impl TemplateFace {
    /// Wraps a parsed [`TemplateSpec`] in a `TemplateFace`.
    pub fn new(spec: TemplateSpec) -> Self {
        Self { spec }
    }
}

impl Face for TemplateFace {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn available_complications(&self) -> Vec<Complication> {
        // Templates carry their own widget bindings; they do not expose
        // free-form complications.
        vec![]
    }

    fn layout(
        &self,
        canvas: &Canvas,
        data: &SystemData,
        theme: &Theme,
        _complications: &EnabledComplications,
    ) -> Layout {
        let mut layout = Layout::new();
        for tw in &self.spec.widgets {
            for rw in resolve(tw, data, theme, canvas) {
                layout.push(Widget {
                    id: Cow::Owned(rw.id),
                    rect: rw.rect,
                    kind: ZoneKind::Dynamic,
                    cadence: Cadence::EveryFrame,
                    content: rw.content,
                });
            }
        }
        layout
    }
}

// ── Storage helpers ───────────────────────────────────────────────────────────

/// Reads and parses `<state_dir>/templates/<name>.json`.
///
/// Returns `None` on any I/O or parse error (error is logged).
pub fn load_template(state_dir: &Path, name: &str) -> Option<TemplateSpec> {
    let path = state_dir.join("templates").join(format!("{name}.json"));
    let json = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to read template {:?}: {}", path, e);
            return None;
        }
    };
    match serde_json::from_str::<TemplateSpec>(&json) {
        Ok(spec) => {
            info!("Loaded template '{}' from {:?}", name, path);
            Some(spec)
        }
        Err(e) => {
            warn!("Failed to parse template {:?}: {}", path, e);
            None
        }
    }
}

/// Returns the file-stems of all `*.json` files in `<state_dir>/templates/`.
///
/// Unreadable files or stems that are not valid UTF-8 are silently skipped.
// Called via AppState::list_all_faces (web/D-Bus wiring in Task 4).
#[allow(dead_code)]
pub fn list_templates(state_dir: &Path) -> Vec<String> {
    let dir = state_dir.join("templates");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x == "json")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            e.path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .collect();
    names.sort();
    names
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::layout::{render_layout, Rect};
    use crate::faces::template::spec::{
        ColorRef, NumberSource, TemplateContent, TemplateWidget, ThemeSlot,
    };
    use crate::faces::{EnabledComplications, Theme};
    use crate::rendering::Canvas;
    use crate::sensors::data::SystemData;

    // ── Helpers ──────────────────────────────────────────────────────────────

    #[allow(clippy::field_reassign_with_default)]
    fn sample_data() -> SystemData {
        let mut d = SystemData::default();
        d.hostname = "testhost".into();
        d.cpu_percent = 50.0;
        d.net_rx_history.push_back(1_000_000.0);
        d.net_tx_history.push_back(500_000.0);
        d
    }

    fn default_theme() -> Theme {
        Theme::from_preset("nord")
    }

    // ── TemplateFace rendering pipeline ──────────────────────────────────────

    /// End-to-end chain: `TemplateSpec` → `TemplateFace` → `layout()` →
    /// `render_layout()` → `canvas.pixels()`.
    ///
    /// A 50% CPU bar (fill=white, bg=black) on a 40×8 canvas:
    /// - left half (x=0..19, y=0..7) should be white (0xFF)
    /// - right half (x=20..39, y=0..7) should be black (0x00)
    #[test]
    fn template_face_bar_renders_fill_pixels() {
        let spec = TemplateSpec {
            name: "test_bar".to_string(),
            orientation: None,
            theme: None,
            widgets: vec![TemplateWidget {
                id: "cpu_bar".to_string(),
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 40,
                    h: 8,
                },
                content: TemplateContent::Bar {
                    value: NumberSource::CpuPercent, // 50.0 from sample_data
                    fill: ColorRef::Hex(0xFFFFFF),
                    bg: ColorRef::Hex(0x000000),
                },
            }],
        };

        let face = TemplateFace::new(spec);
        let canvas = Canvas::new(40, 8);
        let data = sample_data();
        let theme = default_theme();
        let comps = EnabledComplications::new();

        let layout = face.layout(&canvas, &data, &theme, &comps);
        assert_eq!(layout.widgets.len(), 1, "should resolve to 1 widget");

        let mut canvas = Canvas::new(40, 8);
        canvas.set_background(0x000000);
        canvas.clear();
        render_layout(&mut canvas, &layout);

        let px = canvas.pixels();
        let at = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 40 + x) * 4;
            (px[i], px[i + 1], px[i + 2])
        };

        // Left half (filled at 50%) should be white
        assert_eq!(at(2, 4), (255, 255, 255), "filled portion should be white");
        // Right half (unfilled) should be black bg
        assert_eq!(at(38, 4), (0, 0, 0), "unfilled portion should be black");
    }

    // ── load_template round-trip ──────────────────────────────────────────────

    #[test]
    fn load_template_round_trips_json_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path();

        // Create the templates sub-directory and write a JSON file.
        let templates_dir = state_dir.join("templates");
        std::fs::create_dir_all(&templates_dir).expect("create templates dir");

        let spec = TemplateSpec {
            name: "my_face".to_string(),
            orientation: None,
            theme: None,
            widgets: vec![TemplateWidget {
                id: "hostname".to_string(),
                rect: Rect {
                    x: 4,
                    y: 4,
                    w: 160,
                    h: 16,
                },
                content: TemplateContent::Bar {
                    value: NumberSource::CpuPercent,
                    fill: ColorRef::Theme(ThemeSlot::Primary),
                    bg: ColorRef::Hex(0x000000),
                },
            }],
        };

        let json = serde_json::to_string_pretty(&spec).expect("serialize");
        std::fs::write(templates_dir.join("my_face.json"), &json).expect("write");

        let loaded = load_template(state_dir, "my_face").expect("load_template should succeed");
        assert_eq!(loaded, spec, "loaded spec must equal the original");
    }

    #[test]
    fn load_template_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = load_template(dir.path(), "nonexistent");
        assert!(result.is_none(), "missing file should return None");
    }

    #[test]
    fn load_template_returns_none_for_invalid_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path();
        let templates_dir = state_dir.join("templates");
        std::fs::create_dir_all(&templates_dir).expect("create dir");
        std::fs::write(templates_dir.join("bad.json"), b"{ not valid json").expect("write");

        let result = load_template(state_dir, "bad");
        assert!(result.is_none(), "invalid JSON should return None");
    }

    // ── list_templates ────────────────────────────────────────────────────────

    #[test]
    fn list_templates_finds_written_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let templates_dir = dir.path().join("templates");
        std::fs::create_dir_all(&templates_dir).expect("create dir");

        // Write two template files and one non-JSON file (should be ignored).
        std::fs::write(templates_dir.join("alpha.json"), b"{}").expect("write alpha");
        std::fs::write(templates_dir.join("beta.json"), b"{}").expect("write beta");
        std::fs::write(templates_dir.join("readme.txt"), b"ignore me").expect("write txt");

        let names = list_templates(dir.path());
        assert_eq!(
            names,
            vec!["alpha", "beta"],
            "should list only .json stems, sorted"
        );
    }

    #[test]
    fn list_templates_empty_when_no_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        // No `templates/` sub-directory created.
        let names = list_templates(dir.path());
        assert!(names.is_empty(), "no templates dir → empty list");
    }

    // ── example.json end-to-end tests ────────────────────────────────────────

    /// Deserializes the repo-root `templates/example.json` and verifies the
    /// spec round-trips.  This proves that the JSON wire format matches the
    /// actual serde representation produced by `TemplateSpec`.
    #[test]
    fn example_json_deserializes_correctly() {
        let json = include_str!("../../../../../templates/example.json");
        let spec: super::super::spec::TemplateSpec =
            serde_json::from_str(json).expect("example.json must parse without error");

        assert_eq!(spec.name, "example");
        assert!(
            !spec.widgets.is_empty(),
            "example.json must contain at least one widget"
        );

        // Round-trip: serialise back and re-parse — must be identical
        let re_json = serde_json::to_string_pretty(&spec).expect("serialise");
        let back: super::super::spec::TemplateSpec =
            serde_json::from_str(&re_json).expect("round-trip parse");
        assert_eq!(spec, back, "spec must survive a serde round-trip");
    }

    /// Builds a `TemplateFace` from `templates/example.json`, runs
    /// `layout()` against representative `SystemData`, and renders to a
    /// `Canvas`.  Asserts:
    ///   - layout produces > 0 widgets
    ///   - at least one non-background pixel exists (something was painted)
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn example_template_end_to_end_render() {
        use crate::faces::layout::render_layout;

        let json = include_str!("../../../../../templates/example.json");
        let spec: super::super::spec::TemplateSpec =
            serde_json::from_str(json).expect("example.json parse");

        let face = TemplateFace::new(spec);
        let canvas = Canvas::new(320, 170);
        let theme = Theme::from_preset("nord");
        let comps = EnabledComplications::new();

        // Build a rich SystemData so every widget has something to draw
        let mut data = SystemData::default();
        data.hostname = "pve3".into();
        data.cpu_percent = 65.0;
        data.ram_percent = 80.0;
        data.cpu_temp = Some(72.0);
        data.hour = 14;
        data.minute = 30;
        data.day = 21;
        data.month = 6;
        data.year = 2026;
        data.disk_read_rate = 5_000_000.0;
        data.disk_write_rate = 1_000_000.0;
        data.net_rx_rate = 2_000_000.0;
        data.net_tx_rate = 500_000.0;
        data.disk_sample_count = 60;
        data.net_sample_count = 60;
        // Fill 60-sample histories so sparklines have data to draw
        for i in 0..60u64 {
            let v = (i as f64) * 100_000.0;
            data.disk_history.push_back(v);
            data.disk_read_history.push_back(v);
            data.disk_write_history.push_back(v / 2.0);
            data.net_history.push_back(v);
            data.net_rx_history.push_back(v);
            data.net_tx_history.push_back(v / 4.0);
        }

        let layout = face.layout(&canvas, &data, &theme, &comps);
        assert!(
            !layout.widgets.is_empty(),
            "layout must produce at least one widget"
        );

        // Render to a fresh canvas with a black background
        let mut canvas = Canvas::new(320, 170);
        canvas.set_background(0x000000);
        canvas.clear();
        render_layout(&mut canvas, &layout);

        let px = canvas.pixels();

        // Helper: read (R,G,B) at pixel (x,y)
        let at = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 320 + x) * 4;
            (px[i], px[i + 1], px[i + 2])
        };

        // At least one pixel in the canvas must be non-black (something rendered)
        let any_painted = (0..320usize)
            .flat_map(|x| (0..170usize).map(move |y| at(x, y)))
            .any(|(r, g, b)| r != 0 || g != 0 || b != 0);
        assert!(
            any_painted,
            "render must paint at least one non-background pixel"
        );

        // CPU bar fill region (x=40..311, y=28..39 for the 65% fill portion):
        // at 65% fill the bar extends ~176 px into the 272px wide bar.
        // Check a pixel well inside the fill (x=80, y=33 → well within 65% fill).
        let (r, g, b) = at(80, 33);
        assert!(
            r != 0 || g != 0 || b != 0,
            "CPU bar fill region (80,33) must be non-background; got ({r},{g},{b})"
        );
    }

    /// Renders `templates/example.json` to PNGs for visual inspection.
    ///
    /// Outputs:
    ///   /tmp/ws5_template_landscape.png  (320×170)
    ///   /tmp/ws5_template_portrait.png   (170×320)
    ///
    /// This test is intentionally kept in the suite so the controller can
    /// eyeball the layout after any change.
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn render_example_template_png() {
        use crate::faces::layout::render_layout;

        let json = include_str!("../../../../../templates/example.json");
        let spec: super::super::spec::TemplateSpec =
            serde_json::from_str(json).expect("example.json parse");

        let theme = Theme::from_preset("nord");
        let comps = EnabledComplications::new();

        let mut data = SystemData::default();
        data.hostname = "pve3".into();
        data.cpu_percent = 65.0;
        data.ram_percent = 80.0;
        data.cpu_temp = Some(72.0);
        data.hour = 14;
        data.minute = 30;
        data.day = 21;
        data.month = 6;
        data.year = 2026;
        data.disk_read_rate = 5_000_000.0;
        data.disk_write_rate = 1_000_000.0;
        data.net_rx_rate = 2_000_000.0;
        data.net_tx_rate = 500_000.0;
        data.disk_sample_count = 60;
        data.net_sample_count = 60;
        for i in 0..60u64 {
            let v = (i as f64) * 100_000.0;
            data.disk_history.push_back(v);
            data.disk_read_history.push_back(v);
            data.disk_write_history.push_back(v / 2.0);
            data.net_history.push_back(v);
            data.net_rx_history.push_back(v);
            data.net_tx_history.push_back(v / 4.0);
        }

        // ── Landscape 320×170 ──
        {
            let (w, h) = (320u32, 170u32);
            let face = TemplateFace::new(spec.clone());
            let canvas_ref = Canvas::new(w, h);
            let layout = face.layout(&canvas_ref, &data, &theme, &comps);
            let mut canvas = Canvas::new(w, h);
            canvas.set_background(theme.background);
            canvas.clear();
            render_layout(&mut canvas, &layout);

            let raw: Vec<u8> = canvas.pixels().to_vec();
            image::RgbaImage::from_raw(w, h, raw)
                .expect("from_raw landscape")
                .save("/tmp/ws5_template_landscape.png")
                .expect("save landscape PNG");
        }

        // ── Portrait 170×320 ──
        // The spec is designed for 320×170 landscape; widgets with rects that
        // overflow the 170px-wide portrait canvas are skipped to avoid
        // debug_assert panics.  This is intentional: the portrait PNG shows
        // whichever widgets fit, as a developer preview only.
        {
            use crate::faces::layout::{Layout, Widget};

            let (w, h) = (170u32, 320u32);
            let face = TemplateFace::new(spec);
            let canvas_ref = Canvas::new(w, h);
            let full_layout = face.layout(&canvas_ref, &data, &theme, &comps);

            // Keep only widgets whose rect fits entirely within the portrait canvas.
            let clipped = Layout {
                widgets: full_layout
                    .widgets
                    .into_iter()
                    .filter(|widget| {
                        let r = &widget.rect;
                        r.x >= 0
                            && r.y >= 0
                            && (r.x + r.w as i32) <= w as i32
                            && (r.y + r.h as i32) <= h as i32
                    })
                    .collect::<Vec<Widget>>(),
            };

            let mut canvas = Canvas::new(w, h);
            canvas.set_background(theme.background);
            canvas.clear();
            render_layout(&mut canvas, &clipped);

            let raw: Vec<u8> = canvas.pixels().to_vec();
            image::RgbaImage::from_raw(w, h, raw)
                .expect("from_raw portrait")
                .save("/tmp/ws5_template_portrait.png")
                .expect("save portrait PNG");
        }

        // Confirm the files exist and have non-zero size
        let landscape_meta =
            std::fs::metadata("/tmp/ws5_template_landscape.png").expect("landscape PNG must exist");
        let portrait_meta =
            std::fs::metadata("/tmp/ws5_template_portrait.png").expect("portrait PNG must exist");
        assert!(landscape_meta.len() > 0, "landscape PNG must not be empty");
        assert!(portrait_meta.len() > 0, "portrait PNG must not be empty");
    }
}
