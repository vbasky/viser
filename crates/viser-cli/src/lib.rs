//! CLI for viser — Video Encoding Optimizer.
//!
//! The binary entry point (`main.rs`) provides subcommands for encoding,
//! inspection, quality measurement, comparison, charting, and per-title
//! optimization with per-shot refinement.
//!
//! # Installation
//!
//! ```sh
//! cargo install viser-cli
//! ```
//!
//! # Usage
//!
//! ```text
//! viser encode <input>          Encode a video file
//! viser inspect <input>         Inspect video files / probes
//! viser quality <input>         Quality measurement (VMAF, PSNR, SSIM)
//! viser per-title analyze       Per-title convex hull analysis
//! viser per-title deliver       Delivery pipeline from saved analysis
//! viser compare <a> <b>         Side-by-side comparison player
//! viser chart <data>            Generate R-D curve charts
//! viser shot <input>            Shot detection
//! viser complexity <input>      Segment-level complexity analysis
//! ```
