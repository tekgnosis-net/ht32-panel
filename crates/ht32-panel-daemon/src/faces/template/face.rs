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
}
