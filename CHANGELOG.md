# Changelog
## [0.8.1] - 2026-06-23

Robustness, safety, and interrupt-handling fixes for long-running per-title/per-shot/per-segment analyses.

### Fixed

- **Float NaN/Inf panics in ordering** — all `f64::partial_cmp(...).unwrap()` sites (convex hull construction, ladder selection, BD-rate, dip sorting, test invariants) replaced with `f64::total_cmp`. Added non-empty guards and filtering improvements around `first()`/`last()` after sorts.
- **Checkpoint mutex poisoning** — replaced `std::sync::Mutex` with `parking_lot::Mutex`. All lock sites are now infallible; previous poison-recovery `unwrap_or_else` removed.
- **Fragile Arc ownership after parallel joins** — replaced `Arc::try_unwrap(...).unwrap().into_inner()` with `Arc::into_inner(...).expect(...)` (per-title analysis).
- **Two-pass passlog leaks** — `PasslogCleanup` now implements `Drop` so ffmpeg two-pass log files are always cleaned, even on error, `?` return, or panic. Manual `cleanup.run()` call removed.
- **Per-segment parallelism ignored config** — `per-segment analyze` now respects `Config.parallel` (and the new `--parallel` CLI flag); previously always hardcoded `available_parallelism()`.
- **Worker tasks survived cancellation** — converted per-title, per-shot, and per-segment parallel loops to `tokio::task::JoinSet`. Dropping the set aborts in-flight tasks.
- **No graceful Ctrl-C handling** — top-level command dispatch now uses `tokio::select!` on `ctrl_c()`. Interrupt drops futures (triggering all RAII), aborts JoinSet tasks, and prints checkpoint resume guidance. Added `signal` feature to tokio in `viser-cli`.

### Changed

- `viser-checkpoint` depends on `parking_lot = "0.12"`.
- Semaphore acquires and guarded `last_mut` accesses in shot detection now use descriptive `.expect(...)`.

## [0.8.0] - 2026-06-16

Faster analysis: choose a cheaper quality metric and expose subsampling controls.

### Added

- **`--metric vmaf|psnr|ssim`** on `per-title analyze` and `per-shot analyze` —
  selects the quality metric that drives hull/ladder selection. PSNR and SSIM use
  FFmpeg's native filters and bypass libvmaf's expensive feature extraction
  (ADM/VIF/motion), running ~10-20x faster per measurement for quick iteration.
  The chosen metric is recorded as a top-level `metric` field in the analysis JSON
  and labels the result tables accordingly.
- **`per-shot analyze --subsample`** — exposes quality-metric frame subsampling
  (every Nth frame; default 5), previously only available on `per-title analyze`.
- **`per-shot analyze --parallel`** — caps concurrent shot analyses (0 = auto).
- **Native PSNR/SSIM measurement path** in `viser-quality::measure` — when only
  PSNR and/or SSIM are requested, measurement uses FFmpeg's `psnr`/`ssim` filters
  instead of libvmaf, honoring `subsample` via frame decimation.

## [0.7.1] - 2026-06-16

Correctness fixes across shot detection, hardware encoding, probing, quality
measurement, and the comparison player.

### Fixed

- **Shot detection** — `scdet` parsing now keys off the `lavfi.scd.time` cut flag
  instead of treating every per-frame score as a boundary. Previously every frame
  was marked a boundary and the minimum-duration merge collapsed them back into a
  single shot, so a 60s clip with 10 cuts reported just 1. `detect()` now also fails
  loudly on ffmpeg errors and skips boundaries at/after the total duration.
- **NVENC capped CRF** — uses `-rc vbr` instead of `constqp`, which silently ignored
  `-maxrate`/`-bufsize` and dropped the requested bitrate cap.
- **Convex hull** — non-finite (NaN/inf) bitrate/VMAF points are filtered before hull
  construction, avoiding a panic.
- **`extract()`** — validates that start is non-negative and duration is positive and
  finite; concat list paths now escape backslashes as well as single quotes.
- **Probe (MediaInfo path)** — the 8-bit chroma fallback emits a valid `yuv420p`
  pixel format instead of the invalid `yuv420p8le`.
- **Quality measurement** — frame-extraction and XPSNR failures include ffmpeg stderr,
  and a frame-count mismatch is warned. Weighted PSNR `(6·Y + U + V) / 8` is only used
  when both chroma planes are present, otherwise it falls back to luma.
- **Per-segment** — guards against zero total duration so average bitrate/VMAF are
  `0.0` rather than `NaN`.
- **CLI loudness report** — includes true-peak (`Peak:`) and gating threshold lines;
  the over-broad `starts_with("L")` filter that dropped them while printing header
  noise is fixed.
- **Comparison player** — frame timestamps derive from the source's real frame rate
  (probed) instead of a hardcoded 24 fps, fixing dip seek positions on 25/30/50/60 fps
  content.

### Added

- **Per-shot analysis** — `vmaf_model` and `allow_hdr` are now configurable, with
  matching `per-shot analyze` CLI flags.
- **`per-segment analyze --segment-duration`** — segment length is configurable
  (default 1s) instead of hardcoded.

## [0.7.0] - 2026-06-15

Completes the hardware encode/decode matrix.

### Added

- **AV1 hardware encoders** — completes the encode matrix: `av1_nvenc`, `av1_qsv`,
  `av1_vaapi`, `av1_amf` (no `av1_videotoolbox` — Apple ships no AV1 encoder).
  Requires recent silicon (Arc/Battlemage, Ada/Blackwell, RDNA3+). Selectable by
  name/alias across all analysis and encode commands.
- **Hardware-accelerated decode** — `EncodeJob.hwaccel` and the `encode --hwaccel`
  flag inject `-hwaccel <method>` (e.g. `vaapi`, `cuda`, `qsv`, `videotoolbox`)
  before the input; available methods are detected via `ffmpeg -hwaccels` at startup.

### Fixed

- **VAAPI encode surface plumbing** — VAAPI encoders (`h264_vaapi`, `hevc_vaapi`,
  `av1_vaapi`) now initialise a render device (`-vaapi_device`, overridable via
  `VISER_VAAPI_DEVICE`) and upload frames to GPU surfaces (`format=nv12,hwupload`)
  via a unified `-vf` filter chain. Previously VAAPI encodes were emitted with
  software frames and would fail on real hardware.

### Changed

- **`EncodeJob` gained a `hwaccel: Option<String>` field** (breaking for
  struct-literal construction).

## [0.6.1] - 2026-06-09

### Fixed

- **HDR detection with ffprobe >= 8.0** — `color_primaries` and `bits_per_raw_sample`
  are no longer reported as top-level stream fields by ffprobe 8.x for HEVC/Matroska.
  `hdr_kind()` now falls back to the `color_space` field (`bt2020nc`, `bt2020c`) when
  `color_primaries` is absent, combined with pixel-format bit-depth detection.

### Added

- **Property-based tests (`proptest`)** — randomised mathematical-invariant tests for
  convex hull (monotonicity, convexity, per-codec partitioning, below-hull containment)
  and ladder selection (bitrate/VMAF sorting, rung-count bounds, index contiguity).
- **FFmpeg argument invariants** — proptest-driven verification of encoder argument
  construction against FFmpeg's documented encoder syntax.
- **FATE-style integration tests** — 37 tests that generate synthetic test media via
  `ffmpeg -f lavfi` and exercise the full probe → encode → measure pipeline against
  real ffmpeg/ffprobe, validating resolution, codec, duration, bitrate monotonicity,
  VMAF scoring, progress reporting, segment extraction, and concatenation.
- **`just coverage` / `just coverage-lcov`** recipes and CI coverage job
  (`cargo llvm-cov`).

## [0.6.0] - 2026-06-08

### Added

- **Hardware encoder support** — 10 hardware-accelerated codec variants:
  NVENC (`h264_nvenc`, `hevc_nvenc`), QSV (`h264_qsv`, `hevc_qsv`),
  VideoToolbox (`h264_videotoolbox`, `hevc_videotoolbox`),
  VAAPI (`h264_vaapi`, `hevc_vaapi`), AMF (`h264_amf`, `hevc_amf`).
- **Runtime HW encoder detection** via `ffmpeg -encoders` — available hardware
  encoders are discovered at startup and surfaced automatically.
- **Per-backend rate-control dispatch** — CRF, QP, and VBR modes mapped to
  each encoder's native syntax (e.g. NVENC `-cq`, QSV `-global_quality`).
- **Per-backend preset mapping** — NVENC `p1`–`p7`, QSV speed levels,
  VAAPI compression levels, AMF quality/speed, VideoToolbox real-time flag.

## [0.5.0] - 2026-06-07

### Added

- **`viser-metrics` crate** — metric-vs-metric correlation (Pearson, Spearman/
  SROCC, Kendall/KROCC) and divergence detection across aligned score series.
- **`viser metrics compare`** — measure several encodes against one reference and
  compare the metrics: ranked per-metric table, best-per-metric, an agreement
  matrix, and CSV/JSON/HTML reports.
- **`viser metrics no-ref`** — pure-Rust no-reference signals (sharpness via
  variance of Laplacian, 8×8 blockiness, Immerkær noise) that need no reference.
- **More metrics** — MS-SSIM, VIF, CAMBI, and XPSNR, plus per-component (Y/U/V)
  PSNR and pooling strategies (harmonic mean, p1/p5/p10, median).

### Changed

- **Full-clip SSIMULACRA2/butteraugli by default** — measured over every frame
  via a single-pass batch extract; `--frame-samples N` remains as a speed knob.

### Fixed

- **PSNR silently zero** — libvmaf was sent repeated `:feature=` options, so PSNR
  was dropped whenever SSIM was also requested; features are now combined into a
  single `|`-separated option.

## [0.4.2] - 2026-06-06

### Added

- **FFmpeg version detection** — `check_ffmpeg()` and `check_ffprobe()` validate
  that FFmpeg >= 6.0 is installed and report version info at startup (debug
  level). Catches missing or outdated binaries before any work begins.
- **VMAF model validation** — `validate_vmaf_model()` rejects unknown VMAF model
  names passed via `--model`, surfacing a clear error with the list of known
  models.
- **STATUS.md** — project roadmap and status tracker, linked from the README.

### Fixed

- **Banner image URL** — crates.io now shows the viser banner (was a broken
  relative path).

## [0.4.1] - 2026-06-03

### Changed

- **Trimmed tokio features** — the workspace no longer enables tokio's `full`
  feature set. Each crate now declares only the features it uses (`rt`, `process`,
  `io-util`, `sync`, `net`, `macros` as needed), dropping unused subsystems
  (`fs`, `time`, `signal`, …) from the dependency graph and leaving the library
  crates leaner for downstream consumers.

### Fixed

- **Activated a dormant test** — `viser-complexity`'s `test_analyze_opts_default`
  was missing its `#[test]` attribute and had never run; it is now part of the suite.
- **Corrected MSRV in the README project table** — it listed 1.85 while the actual
  MSRV (and the badge) is 1.88.
- Removed the unknown `clippy::manual_checked_ops` lint allow and cleared the
  clippy lint debt that surfaced once warnings became errors.

### CI

- `clippy` now runs with `-D warnings`, so lint regressions fail the build.
- Added a macOS runner to the test matrix and an MSRV (1.88) build job.
- Added a `cargo-deny` job that enforces the existing `deny.toml`
  (advisories, licenses, bans).

## [0.4.0] - 2026-05-31

### Added

- **Revelo probe engine** — optional pure-Rust metadata extraction replaces ffprobe.
  Enable with `--features revelo` at build time, then use `viser inspect probe --probe-engine revelo`.
  Supports the full ProbeResult contract: codec names, HDR transfer/primaries, pixel format,
  frame rate, duration, bitrate, audio channels. ProbeCache can dispatch to either engine.
- **Audio bitrate-aware ladder budgets** — per-title analysis now extracts audio bitrate
  from the source and reserves it in the delivery budget. LadderOpts gains audio_bitrate_kbps,
  Result includes it in saved analysis JSON.
- **Screen content detection** — viser-complexity classifies video as natural or screen
  content (slides, code, UI) from spatial/temporal/DCT heuristics. Four confidence levels
  with detailed reason strings.
- **SSIMULACRA2 and butteraugli quality metrics** — viser-quality now supports five
  perceptual metrics alongside VMAF/PSNR/SSIM. Missing binaries degrade gracefully.
- **Parallel per-shot and per-segment analysis** — both pipelines now use tokio
  spawn + semaphore fan-out instead of sequential for-loops. Per-shot extracts all segments
  first then analyzes in parallel; per-segment runs independent CRF binary search per segment
  concurrently.
- **Comprehensive algorithm test suite** — 170+ tests covering convex hull, BD-rate,
  Trellis optimization, ladder selection, screen content detection, and revelo probe mapping.

### Changed

- **MSRV raised to 1.88** — required by revelo-core use of let-chains.
- LadderOpts gains audio_bitrate_kbps field (default 0.0, backwards-compatible via serde default).
- ProbeCache takes a ProbeEngine parameter; ProbeCache::new() defaults to ffprobe,
  ProbeCache::with_revelo() uses the revelo engine (feature-gated).
- README updated with per-analysis-type command examples, test suite commands, and build flags.

### Fixed

- Release workflow awk bracket-escaping and v prefix stripping fixed so release notes
  actually populate from CHANGELOG.md.

## [0.3.0] - 2026-05-31

### Added

- **SSIMULACRA2 and butteraugli quality metrics** — `viser-quality` now supports
  five perceptual metrics. SSIMULACRA2 and butteraugli run their respective CLI
  tools on extracted PNG frames; missing binaries degrade gracefully to 0.0.
  The `Metric` enum and `Result` struct include `Ssimulacra2` and `Butteraugli`
  variants, and both are enabled by default in `MeasureOpts`.

### Changed

- `viser-quality` README updated with prerequisites for the new metrics.

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
