//! # viser — Video Encoding Optimizer
//!
//! This is the **facade crate** for the viser workspace. It does not contain
//! logic of its own; it re-exports each viser library crate as a module so you
//! can depend on a single `viser` crate instead of a dozen `viser-*` crates.
//!
//! Every module is gated behind a feature flag of the same name. All features
//! are enabled by default via the `full` feature; disable default features and
//! opt in to keep your dependency tree small:
//!
//! ```toml
//! # everything (default)
//! viser = "0.10"
//!
//! # only what you need
//! viser = { version = "0.10", default-features = false, features = ["quality", "hull"] }
//! ```
//!
//! The command-line tool lives in the separate `viser-cli` crate, which
//! installs a `viser` binary: `cargo install viser-cli`.
//!
//! ## Modules
//!
//! | Module | Crate | Purpose |
//! |--------|-------|---------|
//! | [`ffmpeg`] | `viser-ffmpeg` | FFmpeg/FFprobe wrapper |
//! | [`quality`] | `viser-quality` | VMAF/PSNR/SSIM measurement |
//! | [`hull`] | `viser-hull` | Convex hull (Pareto frontier) and BD-Rate |
//! | [`ladder`] | `viser-ladder` | Bitrate ladder selection |
//! | [`shot`] | `viser-shot` | Shot/scene detection |
//! | [`complexity`] | `viser-complexity` | Spatial/temporal/DCT complexity analysis |
//! | [`encoding`] | `viser-encoding` | Shared encoding configuration |
//! | [`checkpoint`] | `viser-checkpoint` | Checkpoint/resume support |
//! | [`pertitle`] | `viser-pertitle` | Per-title encoding pipeline |
//! | [`pershot`] | `viser-pershot` | Per-shot encoding with Trellis allocation |
//! | [`persegment`] | `viser-persegment` | Segment-level CRF adaptation |
//! | [`contextaware`] | `viser-contextaware` | Device-specific ladder generation |
//! | [`compare`] | `viser-compare` | Side-by-side comparison player |
//! | [`chart`] | `viser-chart` | Chart generation (R-D curves, hull, ladder) |

#[cfg(feature = "ffmpeg")]
pub use viser_ffmpeg as ffmpeg;

#[cfg(feature = "quality")]
pub use viser_quality as quality;

#[cfg(feature = "hull")]
pub use viser_hull as hull;

#[cfg(feature = "ladder")]
pub use viser_ladder as ladder;

#[cfg(feature = "shot")]
pub use viser_shot as shot;

#[cfg(feature = "complexity")]
pub use viser_complexity as complexity;

#[cfg(feature = "encoding")]
pub use viser_encoding as encoding;

#[cfg(feature = "checkpoint")]
pub use viser_checkpoint as checkpoint;

#[cfg(feature = "pertitle")]
pub use viser_pertitle as pertitle;

#[cfg(feature = "pershot")]
pub use viser_pershot as pershot;

#[cfg(feature = "persegment")]
pub use viser_persegment as persegment;

#[cfg(feature = "contextaware")]
pub use viser_contextaware as contextaware;

#[cfg(feature = "compare")]
pub use viser_compare as compare;

#[cfg(feature = "chart")]
pub use viser_chart as chart;
