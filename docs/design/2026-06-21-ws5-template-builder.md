# WS5 — Web template builder (the end goal)

Status: SPEC · 2026-06-21 · fork: tekgnosis-net/ht32-panel

## Vision

Let a user **design their own LCD face in the browser** — drop widgets (text, bars, gauges,
sparklines, clock), position them, bind each to a live data source, pick colours/size/cadence,
preview against the real render, and save it as a face the daemon shows. This is what the WS2
typed-widget Layout engine was built *for*: every render primitive a user could place already
exists and is tested (`Text`/`TextScaled`/`Bar`/`DualSparkline`/`Line`/`Circle`/`Arc`).

## Core insight

A **template is not a serialized `Layout`.** A `Layout`'s widgets carry *literal rendered data*
(`Text { text: "endeavour" }`). A template carries a **binding** (`Text <- hostname`) that the
daemon resolves against live `SystemData` **every frame**. So:

```
TemplateSpec  +  SystemData  +  Theme   ──(resolve)──►  Layout  ──► render_layout ──► canvas/LCD
(stored JSON)    (live)         (palette)               (existing engine, unchanged)
```

The existing render path is untouched; we add a *new face* that produces a `Layout` from a spec
instead of from hand-written Rust.

## Data model (serde)

```rust
// crates/ht32-panel-daemon/src/faces/template/spec.rs  (new)
#[derive(Serialize, Deserialize, Clone)]
struct TemplateSpec {
    name: String,
    orientation: Option<Orientation>,   // None = inherit the display setting
    theme: Option<String>,              // None = inherit
    widgets: Vec<TemplateWidget>,
}

#[derive(Serialize, Deserialize, Clone)]
struct TemplateWidget {
    id: String,                         // user-chosen; unique within the template
    rect: Rect,                         // {x,y,w,h} in canvas coords
    #[serde(flatten)] content: TemplateContent,
}

// Each variant = a widget KIND + its binding(s) + style. Tagged by `"kind"`.
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TemplateContent {
    Text   { value: TextSource, size: f32, color: ColorRef, align: Align },
    Bar    { value: NumberSource, fill: ColorRef, bg: ColorRef },
    Gauge  { value: NumberSource, min: f64, max: f64, /* arc geometry */ color: ColorRef, track: ColorRef },
    Sparkline { a: HistorySource, b: Option<HistorySource>, wrap_around: bool,
                color_a: ColorRef, color_b: ColorRef, bg: ColorRef, scale: ScaleMode },
    Clock  { mode: ClockMode, color: ColorRef },          // analog | digital
}

// Bindings — a CLOSED set of named sources (validated, and the UI's dropdown options).
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "src", content = "fmt", rename_all = "snake_case")]
enum TextSource {
    Literal(String), Hostname, Uptime, Ip, NetInterface,
    Time(TimeFmt), Date(DateFmt),
    Number(NumberSource, NumberFmt),                       // e.g. "CPU {}%", rate "↓{}"
}
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum NumberSource { CpuPercent, RamPercent, CpuTemp, DiskReadRate, DiskWriteRate, NetRxRate, NetTxRate }
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum HistorySource { DiskHistory, DiskReadHistory, DiskWriteHistory, NetHistory, NetRxHistory, NetTxHistory }

enum ColorRef { Theme(ThemeSlot), Hex(u32) }              // "primary" | "#00ff88"
```

`Rect` already exists; it gains `#[derive(Serialize, Deserialize)]`. A closed-enum binding model
(not free-form expressions) keeps it validated, safe, and trivially enumerable for the editor's
dropdowns. The `id` field is now a user string — this is where `Widget.id: &'static str` becomes
`Box<str>` (the deferred decision; templates need dynamic ids).

## Resolver + the template face

```rust
// resolve a single widget against live data → a WidgetContent
fn resolve(w: &TemplateWidget, data: &SystemData, theme: &Theme) -> Option<WidgetContent>;

struct TemplateFace { spec: TemplateSpec }
impl Face for TemplateFace {
    fn layout(&self, canvas, data, theme, _comps) -> Layout {
        let mut layout = Layout::new();
        for w in &self.spec.widgets {
            if let Some(content) = resolve(w, data, theme) {
                layout.push(Widget { id: w.id.clone().into(), rect: w.rect, kind, cadence, content });
            }
        }
        layout
    }
}
```

Resolvers: `resolve_text(TextSource,data)->String`, `resolve_number(NumberSource,data)->f64`,
`resolve_history(HistorySource,data)->&VecDeque<f64>`. `ColorRef`→`u32` via the active `Theme`.

## Storage + selection

- Templates live as JSON in `<state_dir>/templates/<name>.json`.
- `create_face("template:<name>")` (or a reserved name) loads the spec and returns a `TemplateFace`.
  `available_faces()` lists built-ins **plus** every saved template.
- Setting the face to a template name activates it (existing `set_face` path, persisted in
  `DisplaySettings`).

## Build order (milestones)

**Milestone 1 — the rendering pipeline (this plan, no UI):**
1. `Rect` + a new `template` module: `TemplateSpec`/`TemplateContent`/source enums + serde. Unit-test round-trip.
2. Resolvers (`resolve_text/number/history`, `ColorRef`→theme) + `resolve()` → `WidgetContent`. Unit-test each source.
3. `TemplateFace` + wire into `create_face`/`available_faces`/storage load. (`Widget.id` → `Box<str>` here.)
4. A sample `templates/example.json` (hostname + clock + cpu/ram bars + a disk sparkline); an
   integration test renders it and asserts a known pixel; **deploy to pve3 and view it on the LCD**
   (the real acceptance test — eyes on hardware, the lesson from Phase 4).

**Milestone 2 — the web editor (separate plan, brainstorm UX first):**
- A 320×170 / 170×320 editor canvas (HTMX + a small JS layer): a widget palette, drag-to-place,
  a property panel (binding dropdown from the closed source set, colour, size, cadence), live
  preview via the existing `/lcd.png`, and save (POST → write `<name>.json`, refresh `available_faces`).
- D-Bus/web CRUD: list/get/create/update/delete templates; activate.

## Testing / acceptance

- **Pure:** spec serde round-trip; each resolver source → expected value; `ColorRef`→theme; a full
  `resolve()` of every `TemplateContent` variant.
- **Integration:** load `example.json` → `TemplateFace::layout()` → `render_layout` → assert pixels.
- **Hardware (non-negotiable after Phase 4):** deploy, render a template on the LCD, **view it**
  (video/photo) before declaring it works. The web `/lcd.png` is necessary but NOT sufficient.

## Notes

- Milestone 1 is behaviour-additive: existing faces untouched; the template face is new and opt-in.
- `Box<str>` ids also unblock a future per-zone scheduler (the parked Phase 3) if revisited.
- Keep the binding set CLOSED (an enum) — it's the editor's source-of-truth dropdown and keeps
  templates portable + validated. Extend by adding variants as new sensors land.
