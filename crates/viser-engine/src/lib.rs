//! Engine-agnostic video media layer for the `viser` workspace.
//!
//! Viser pipelines (per-title, per-shot, quality, CLI) talk to a [`VideoEngine`]
//! rather than calling FFmpeg directly. [`viser_ffmpeg`](https://docs.rs/viser-ffmpeg)
//! provides the default FFmpeg/FFprobe implementation; additional engines (for
//! example neural codecs such as MLVC) implement the same trait.
//!
//! # Architecture
//!
//! ```text
//!   CLI / pipelines
//!         │
//!         ▼
//!   viser-engine  ◄── shared types + VideoEngine trait + registry
//!         │
//!    ┌────┴────┐
//!    ▼         ▼
//!  FFmpeg    External / MLVC / …
//!  backend   backends
//! ```
//!
//! # Default engine
//!
//! Call [`set_default_engine`] once at process startup (the CLI does this).
//! Free functions [`probe`], [`encode`], [`extract`], and [`concat`] then
//! dispatch through the registered default.

mod codec;
mod color;
mod composite;
mod encode_types;
mod engine;
mod external;
mod hdr;
mod mlvc;
mod probe_types;
mod registry;
mod resolution;

pub use codec::*;
pub use color::*;
pub use composite::*;
pub use encode_types::*;
pub use engine::*;
pub use external::*;
pub use hdr::*;
pub use mlvc::*;
pub use probe_types::*;
pub use registry::*;
pub use resolution::*;
