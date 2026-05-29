# Changelog
## [0.2.1] - 2026-05-29

### Added

- Emoji banner on CLI help
- lib.rs for viser-cli crate (docs.rs compatibility)

### Fixed

- Bare viser with no subcommand exits 0 and shows help instead of erroring with exit 2
- All subcommands with missing required args exit 0 and show usage instead of erroring with exit 2


## [0.2.0] - 2026-05-29

### Added

- **Two-pass VBR delivery workflow**: per-title analyses can now be turned into final delivery encodes with saved-analysis driven rung generation
- **Parallel delivery pipeline**: rung encodes can run concurrently with semaphore-controlled fan-out
- **Delivery manifest export**: delivery runs can emit machine-readable manifest JSON with artifact metadata and measured bitrates
- **Capped CRF mode**: added capped-CRF rate control for direct encodes and per-title delivery outputs
- **Local chunked delivery**: per-title delivery can split outputs into local chunks and concatenate them into final artifacts
- **HDR probe classification**: ffprobe metadata is now classified for HDR formats including PQ, HLG, and BT.2020/high-bit-depth sources

### Changed

- **HDR guardrails**: per-title analysis and delivery now block HDR inputs by default unless best-effort `--allow-hdr` is explicitly set
- **CLI encode surface**: `viser encode` and `viser per-title deliver` now expose richer rate-control options for VBR and capped-CRF workflows
- **Probe inspection output**: `viser inspect probe` now shows dynamic-range and color-metadata details directly in the CLI
- **Documentation**: README updated to describe the new delivery pipeline, capped-CRF support, chunked delivery, and HDR limitations

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
