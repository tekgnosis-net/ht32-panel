//! Template face — a data-driven face defined by a JSON `TemplateSpec`.
//!
//! **Task 1 (WS5):** defines the serialisable format only; the resolver and
//! `TemplateFace` impl follow in Tasks 2–3.
//!
//! **Task 2 (WS5):** adds `resolve` — source resolvers + `resolve()` that turns
//! a `TemplateWidget` + `SystemData` + `Theme` into `Vec<ResolvedWidget>`.

pub mod resolve;
pub mod spec;
