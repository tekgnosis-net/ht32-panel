//! Template face — a data-driven face defined by a JSON `TemplateSpec`.
//!
//! **Task 1 (WS5):** defines the serialisable format only; the resolver and
//! `TemplateFace` impl follow in Tasks 2–3.
//!
//! **Task 2 (WS5):** adds `resolve` — source resolvers + `resolve()` that turns
//! a `TemplateWidget` + `SystemData` + `Theme` into `Vec<ResolvedWidget>`.
//!
//! **Task 3 (WS5):** adds `TemplateFace` + JSON storage helpers so templates are
//! first-class, selectable faces.

pub mod face;
pub mod preview;
pub mod resolve;
pub mod spec;

pub use face::{list_templates, load_template, TemplateFace};
