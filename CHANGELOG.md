# Changelog

## [0.1.0] - 2026-05-29

### Added

- Initial release — ported from VEO Go implementation
- **Per-Title Encoding**: Convex hull (Pareto frontier) analysis, BD-Rate computation, resolution crossover enforcement, Netflix/Apple fixed ladder comparison, checkpoint/resume
- **Per-Shot Encoding**: Shot detection via FFmpeg scdet, per-shot convex hulls, Trellis Lagrangian bit allocation
- **Segment-Level CRF Adaptation**: Complexity analysis (entropy + YDIF + DCT energy), binary-search CRF per segment, closed-loop VMAF verification
- **Context-Aware Encoding**: Device profiles (mobile/desktop/TV/4K TV) with resolution caps, codec preferences, VMAF model selection
- **Quality Metrics**: VMAF (primary), PSNR, SSIM, per-frame output, quality dip detection
- **Comparison Player**: Browser-based side-by-side player with VMAF timeline
- **CLI**: clap-based command interface with encode, inspect, quality, per-title, per-shot, per-segment, context-aware, and compare subcommands
- **Infrastructure**: tokio async runtime, structured logging (tracing), SHA-256 checkpointing, semaphore-gated parallelism
