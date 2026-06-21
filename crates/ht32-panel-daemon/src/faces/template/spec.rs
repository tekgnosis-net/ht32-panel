//! Serializable template specification for the WS5 template-builder face.
//!
//! A `TemplateSpec` is stored as JSON in `<state_dir>/templates/<name>.json`.
//! It carries **bindings** (which live `SystemData` field to read) rather than
//! literal rendered values, so the daemon resolves it against live data every
//! frame.
//!
//! # Wire-format decisions
//!
//! - **`TemplateOrientation`**: a local mirror of `ht32_panel_hw::Orientation`
//!   (the hw crate has no serde dep).  Serialises as `"landscape"` /
//!   `"portrait"` / `"landscape_upside_down"` / `"portrait_upside_down"`.
//!
//! - **`ColorRef`**: `#[serde(untagged)]`.  `Theme(ThemeSlot)` serialises as a
//!   plain string (e.g. `"primary"`), `Hex(u32)` as a JSON integer
//!   (e.g. `16711935`).  Untagged works cleanly because the two runtime shapes
//!   are disjoint (string vs. number).
//!
//! - **`TextSource`**: `#[serde(tag = "src", content = "fmt")]`.  Variants
//!   with no data (Hostname, Uptime, …) serialise as `{"src":"hostname"}`;
//!   `Time` as `{"src":"time","fmt":"hhmm"}`; `Number` uses a struct-variant
//!   to keep the two sub-fields named: `{"src":"number","fmt":{"source":"cpu_percent","style":"percent"}}`.
//!
//! - **`ScaleMode`**: `Auto` → `"auto"` (unit variant string); `Fixed(f64)`
//!   → `{"fixed":1234.5}` (newtype-variant object).  Uses the default serde
//!   externally-tagged representation.

use crate::faces::layout::Rect;
use serde::{Deserialize, Serialize};

// ── Orientation mirror ───────────────────────────────────────────────────────

/// Local mirror of `ht32_panel_hw::Orientation` (the hw crate has no serde
/// dependency, so we define our own serialisable copy and convert on demand).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateOrientation {
    Landscape,
    Portrait,
    LandscapeUpsideDown,
    PortraitUpsideDown,
}

impl From<TemplateOrientation> for ht32_panel_hw::Orientation {
    fn from(o: TemplateOrientation) -> Self {
        match o {
            TemplateOrientation::Landscape => ht32_panel_hw::Orientation::Landscape,
            TemplateOrientation::Portrait => ht32_panel_hw::Orientation::Portrait,
            TemplateOrientation::LandscapeUpsideDown => {
                ht32_panel_hw::Orientation::LandscapeUpsideDown
            }
            TemplateOrientation::PortraitUpsideDown => {
                ht32_panel_hw::Orientation::PortraitUpsideDown
            }
        }
    }
}

impl From<ht32_panel_hw::Orientation> for TemplateOrientation {
    fn from(o: ht32_panel_hw::Orientation) -> Self {
        match o {
            ht32_panel_hw::Orientation::Landscape => TemplateOrientation::Landscape,
            ht32_panel_hw::Orientation::Portrait => TemplateOrientation::Portrait,
            ht32_panel_hw::Orientation::LandscapeUpsideDown => {
                TemplateOrientation::LandscapeUpsideDown
            }
            ht32_panel_hw::Orientation::PortraitUpsideDown => {
                TemplateOrientation::PortraitUpsideDown
            }
        }
    }
}

// ── Colour refs ──────────────────────────────────────────────────────────────

/// A slot in the active `Theme`.  Variants match the public fields of
/// [`crate::faces::Theme`]: `primary`, `secondary`, `text`, `background`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSlot {
    Primary,
    Secondary,
    Text,
    Background,
}

/// A colour reference — either a named theme slot or a literal RGB-888 value.
///
/// Wire format (untagged):
/// - `"primary"` → `Theme(ThemeSlot::Primary)`
/// - `16711935`  → `Hex(0xFF00FF)`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ColorRef {
    /// A named slot in the active theme.
    Theme(ThemeSlot),
    /// A literal RGB-888 colour packed into the low 24 bits.
    Hex(u32),
}

// ── Source enums ─────────────────────────────────────────────────────────────

/// A scalar numeric data source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberSource {
    CpuPercent,
    RamPercent,
    CpuTemp,
    DiskReadRate,
    DiskWriteRate,
    NetRxRate,
    NetTxRate,
}

/// A ring-buffer history data source (maps to `VecDeque<f64>` fields in
/// `SystemData`).
///
/// Variant names intentionally carry the `History` suffix to mirror the field
/// names in `SystemData` (e.g. `disk_history`, `net_rx_history`).
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistorySource {
    DiskHistory,
    DiskReadHistory,
    DiskWriteHistory,
    NetHistory,
    NetRxHistory,
    NetTxHistory,
}

// ── Format enums ─────────────────────────────────────────────────────────────

/// How a time value is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeFmt {
    /// `HH:MM` — 24-hour.
    Hhmm,
    /// `HH:MM:SS` — 24-hour with seconds.
    Hhmmss,
    /// `hh:MM am/pm` — 12-hour.
    Hhmm12h,
}

/// How a date value is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFmt {
    /// `YYYY-MM-DD`
    Iso,
    /// `DD/MM/YYYY`
    Eu,
    /// `MM/DD/YYYY`
    Us,
    /// `Mon DD` (e.g. `Jan 15`)
    Short,
}

/// How a number is formatted into a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberFmt {
    /// `{value:.0}%`
    Percent,
    /// `{value:.1} MB/s` (or appropriate unit)
    Rate,
    /// Raw decimal, no unit.
    Raw,
}

// ── Text source ──────────────────────────────────────────────────────────────

/// Helper struct for the `Number` variant of `TextSource` so both sub-fields
/// are named in the JSON content object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberBinding {
    pub source: NumberSource,
    pub style: NumberFmt,
}

/// Where a `Text` widget gets its string value.
///
/// Wire format: `#[serde(tag = "src", content = "fmt")]`
/// - `{"src":"hostname"}` → `Hostname`
/// - `{"src":"time","fmt":"hhmm"}` → `Time(TimeFmt::Hhmm)`
/// - `{"src":"number","fmt":{"source":"cpu_percent","style":"percent"}}` → `Number(…)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "src", content = "fmt", rename_all = "snake_case")]
pub enum TextSource {
    Literal(String),
    Hostname,
    Uptime,
    Ip,
    NetInterface,
    Time(TimeFmt),
    Date(DateFmt),
    Number(NumberBinding),
}

// ── Support enums ─────────────────────────────────────────────────────────────

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Y-axis scale mode for a sparkline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    /// Scale automatically to the peak value in the visible window.
    Auto,
    /// Fix the maximum to a specific value (e.g. `100.0` for CPU%).
    Fixed(f64),
}

/// Analog vs digital clock widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockMode {
    Analog,
    Digital,
}

// ── Widget content ────────────────────────────────────────────────────────────

/// The kind of widget and all its binding + style parameters.
///
/// Tagged by `"kind"` in JSON (e.g. `"kind":"text"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateContent {
    /// A text label bound to a data source.
    Text {
        value: TextSource,
        size: f32,
        color: ColorRef,
        align: Align,
    },
    /// A horizontal progress bar.
    Bar {
        value: NumberSource,
        fill: ColorRef,
        bg: ColorRef,
    },
    /// An arc gauge (partial circle fill).
    Gauge {
        value: NumberSource,
        min: f64,
        max: f64,
        color: ColorRef,
        track: ColorRef,
    },
    /// A dual-channel scrolling sparkline / oscilloscope.
    Sparkline {
        a: HistorySource,
        b: Option<HistorySource>,
        wrap_around: bool,
        color_a: ColorRef,
        color_b: ColorRef,
        bg: ColorRef,
        scale: ScaleMode,
    },
    /// An analog or digital clock.
    Clock { mode: ClockMode, color: ColorRef },
}

// ── Top-level spec types ──────────────────────────────────────────────────────

/// A single widget inside a template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateWidget {
    /// User-chosen unique identifier within this template.
    pub id: String,
    /// Bounding box in canvas coordinates.
    pub rect: Rect,
    /// Widget kind and all its binding/style data.
    #[serde(flatten)]
    pub content: TemplateContent,
}

/// A complete face template stored as JSON.
///
/// `None` for `orientation` / `theme` means "inherit the daemon's current setting".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<TemplateOrientation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub widgets: Vec<TemplateWidget>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a representative `TemplateSpec` in Rust, serialise to JSON, parse
    /// back and assert structural equality.  This locks the wire format against
    /// accidental breaks.
    #[test]
    fn round_trip_template_spec() {
        let spec = TemplateSpec {
            name: "my_face".to_string(),
            orientation: Some(TemplateOrientation::Landscape),
            theme: Some("nord".to_string()),
            widgets: vec![
                TemplateWidget {
                    id: "hostname".to_string(),
                    rect: Rect {
                        x: 4,
                        y: 4,
                        w: 160,
                        h: 16,
                    },
                    content: TemplateContent::Text {
                        value: TextSource::Hostname,
                        size: 12.0,
                        color: ColorRef::Theme(ThemeSlot::Text),
                        align: Align::Left,
                    },
                },
                TemplateWidget {
                    id: "cpu_bar".to_string(),
                    rect: Rect {
                        x: 0,
                        y: 24,
                        w: 320,
                        h: 8,
                    },
                    content: TemplateContent::Bar {
                        value: NumberSource::CpuPercent,
                        fill: ColorRef::Theme(ThemeSlot::Primary),
                        bg: ColorRef::Hex(0x202020),
                    },
                },
                TemplateWidget {
                    id: "net_spark".to_string(),
                    rect: Rect {
                        x: 0,
                        y: 40,
                        w: 160,
                        h: 40,
                    },
                    content: TemplateContent::Sparkline {
                        a: HistorySource::NetRxHistory,
                        b: Some(HistorySource::NetTxHistory),
                        wrap_around: true,
                        color_a: ColorRef::Theme(ThemeSlot::Primary),
                        color_b: ColorRef::Theme(ThemeSlot::Secondary),
                        bg: ColorRef::Hex(0x000000),
                        scale: ScaleMode::Auto,
                    },
                },
                TemplateWidget {
                    id: "clock".to_string(),
                    rect: Rect {
                        x: 200,
                        y: 0,
                        w: 120,
                        h: 120,
                    },
                    content: TemplateContent::Clock {
                        mode: ClockMode::Analog,
                        color: ColorRef::Theme(ThemeSlot::Primary),
                    },
                },
            ],
        };

        let json = serde_json::to_string_pretty(&spec).expect("serialise");
        let back: TemplateSpec = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(spec, back, "round-trip must be identity");
    }

    /// Parse a hand-written JSON string covering every `TemplateContent` variant
    /// plus a `Number` text binding, a theme colour, and a hex colour.
    /// This test locks the concrete wire format so accidental serde-repr changes
    /// are caught immediately.
    #[test]
    fn parse_all_variants_from_json() {
        // Wire format: TextSource uses {"src":"...", "fmt":...} nested under "value".
        // NumberSource/HistorySource serialize as snake_case strings.
        // ColorRef: theme slot → plain string (e.g. "primary"); hex → integer.
        // ScaleMode: "auto" | {"fixed": <f64>}.
        let json = r#"
        {
          "name": "all_variants",
          "widgets": [
            {
              "id": "label_literal",
              "rect": {"x":0,"y":0,"w":100,"h":20},
              "kind": "text",
              "value": {"src": "literal", "fmt": "hello"},
              "size": 14.0,
              "color": "text",
              "align": "center"
            },
            {
              "id": "label_time",
              "rect": {"x":0,"y":20,"w":100,"h":20},
              "kind": "text",
              "value": {"src": "time", "fmt": "hhmm"},
              "size": 14.0,
              "color": "primary",
              "align": "left"
            },
            {
              "id": "label_number",
              "rect": {"x":0,"y":40,"w":100,"h":20},
              "kind": "text",
              "value": {"src": "number", "fmt": {"source": "cpu_percent", "style": "percent"}},
              "size": 14.0,
              "color": 16711935,
              "align": "right"
            },
            {
              "id": "cpu_bar",
              "rect": {"x":0,"y":60,"w":320,"h":8},
              "kind": "bar",
              "value": "cpu_percent",
              "fill": "primary",
              "bg": 2105376
            },
            {
              "id": "temp_gauge",
              "rect": {"x":0,"y":80,"w":60,"h":60},
              "kind": "gauge",
              "value": "cpu_temp",
              "min": 20.0,
              "max": 100.0,
              "color": "secondary",
              "track": "background"
            },
            {
              "id": "net_spark",
              "rect": {"x":0,"y":150,"w":160,"h":40},
              "kind": "sparkline",
              "a": "net_rx_history",
              "b": "net_tx_history",
              "wrap_around": true,
              "color_a": "primary",
              "color_b": "secondary",
              "bg": 0,
              "scale": "auto"
            },
            {
              "id": "clock",
              "rect": {"x":200,"y":0,"w":120,"h":120},
              "kind": "clock",
              "mode": "analog",
              "color": "primary"
            }
          ]
        }
        "#;

        let spec: TemplateSpec = serde_json::from_str(json).expect("parse all-variants JSON");
        assert_eq!(spec.name, "all_variants");
        assert_eq!(spec.widgets.len(), 7);
        assert!(spec.orientation.is_none());
        assert!(spec.theme.is_none());

        // widget 0: literal text
        match &spec.widgets[0].content {
            TemplateContent::Text {
                value,
                color,
                align,
                ..
            } => {
                assert_eq!(*value, TextSource::Literal("hello".to_string()));
                assert_eq!(*color, ColorRef::Theme(ThemeSlot::Text));
                assert_eq!(*align, Align::Center);
            }
            other => panic!("expected Text, got {other:?}"),
        }

        // widget 1: time text
        match &spec.widgets[1].content {
            TemplateContent::Text { value, color, .. } => {
                assert_eq!(*value, TextSource::Time(TimeFmt::Hhmm));
                assert_eq!(*color, ColorRef::Theme(ThemeSlot::Primary));
            }
            other => panic!("expected Text, got {other:?}"),
        }

        // widget 2: number text + hex colour
        match &spec.widgets[2].content {
            TemplateContent::Text {
                value,
                color,
                align,
                ..
            } => {
                assert_eq!(
                    *value,
                    TextSource::Number(NumberBinding {
                        source: NumberSource::CpuPercent,
                        style: NumberFmt::Percent,
                    })
                );
                assert_eq!(*color, ColorRef::Hex(16711935));
                assert_eq!(*align, Align::Right);
            }
            other => panic!("expected Text, got {other:?}"),
        }

        // widget 3: bar
        match &spec.widgets[3].content {
            TemplateContent::Bar { value, fill, bg } => {
                assert_eq!(*value, NumberSource::CpuPercent);
                assert_eq!(*fill, ColorRef::Theme(ThemeSlot::Primary));
                assert_eq!(*bg, ColorRef::Hex(2105376));
            }
            other => panic!("expected Bar, got {other:?}"),
        }

        // widget 4: gauge
        match &spec.widgets[4].content {
            TemplateContent::Gauge {
                value,
                min,
                max,
                color,
                track,
            } => {
                assert_eq!(*value, NumberSource::CpuTemp);
                assert!((min - 20.0).abs() < f64::EPSILON);
                assert!((max - 100.0).abs() < f64::EPSILON);
                assert_eq!(*color, ColorRef::Theme(ThemeSlot::Secondary));
                assert_eq!(*track, ColorRef::Theme(ThemeSlot::Background));
            }
            other => panic!("expected Gauge, got {other:?}"),
        }

        // widget 5: sparkline
        match &spec.widgets[5].content {
            TemplateContent::Sparkline {
                a,
                b,
                wrap_around,
                scale,
                ..
            } => {
                assert_eq!(*a, HistorySource::NetRxHistory);
                assert_eq!(*b, Some(HistorySource::NetTxHistory));
                assert!(*wrap_around);
                assert_eq!(*scale, ScaleMode::Auto);
            }
            other => panic!("expected Sparkline, got {other:?}"),
        }

        // widget 6: clock
        match &spec.widgets[6].content {
            TemplateContent::Clock { mode, color } => {
                assert_eq!(*mode, ClockMode::Analog);
                assert_eq!(*color, ColorRef::Theme(ThemeSlot::Primary));
            }
            other => panic!("expected Clock, got {other:?}"),
        }
    }

    /// `TemplateOrientation` round-trips and maps to the hw crate's enum.
    #[test]
    fn orientation_mirror_converts_both_ways() {
        use ht32_panel_hw::Orientation;
        let pairs = [
            (TemplateOrientation::Landscape, Orientation::Landscape),
            (TemplateOrientation::Portrait, Orientation::Portrait),
            (
                TemplateOrientation::LandscapeUpsideDown,
                Orientation::LandscapeUpsideDown,
            ),
            (
                TemplateOrientation::PortraitUpsideDown,
                Orientation::PortraitUpsideDown,
            ),
        ];
        for (t, h) in pairs {
            assert_eq!(Orientation::from(t), h);
            assert_eq!(TemplateOrientation::from(h), t);
        }

        // Also check serde round-trip of orientation
        let json = serde_json::to_string(&TemplateOrientation::LandscapeUpsideDown).unwrap();
        assert_eq!(json, r#""landscape_upside_down""#);
        let back: TemplateOrientation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TemplateOrientation::LandscapeUpsideDown);
    }

    /// `ColorRef` untagged: theme slots serialise as strings, hex as integers.
    #[test]
    fn color_ref_wire_format() {
        let theme_color = ColorRef::Theme(ThemeSlot::Primary);
        let hex_color = ColorRef::Hex(0xFF00FF);

        let theme_json = serde_json::to_string(&theme_color).unwrap();
        let hex_json = serde_json::to_string(&hex_color).unwrap();

        assert_eq!(theme_json, r#""primary""#);
        assert_eq!(hex_json, "16711935"); // 0xFF00FF decimal

        let back_theme: ColorRef = serde_json::from_str(&theme_json).unwrap();
        let back_hex: ColorRef = serde_json::from_str(&hex_json).unwrap();

        assert_eq!(back_theme, theme_color);
        assert_eq!(back_hex, hex_color);
    }

    /// Fixed `ScaleMode` round-trips correctly.
    #[test]
    fn scale_mode_fixed_round_trip() {
        let scale = ScaleMode::Fixed(200.0);
        let json = serde_json::to_string(&scale).unwrap();
        let back: ScaleMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scale);

        let auto = ScaleMode::Auto;
        let auto_json = serde_json::to_string(&auto).unwrap();
        assert_eq!(auto_json, r#""auto""#);
    }
}
