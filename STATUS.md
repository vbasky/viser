# viser status & roadmap

viser today is a **content-adaptive VOD encoding toolkit**: probe, analyze,
encode, and measure quality — all from a single Rust binary. This document
tracks what's covered, what's missing, and what's planned.

Priorities are ordered by impact; checkboxes track status. Nothing here is a
commitment to a date.

## Status snapshot

**Covered:** per-title convex-hull analysis, per-shot Trellis bit allocation,
VMAF/PSNR/SSIM quality measurement, shot detection (scdet, PySceneDetect,
TransNetV2), capped-CRF and CBR encoding, checkpoint/resume, content-adaptive
encoding profiles, segment-level CRF tuning, comparison player, hardware
encoder support (NVENC, QuickSync, VideoToolbox, VAAPI, AMF — H.264/H.265).

**Not covered:** see tiers below.

---

## P0 — correctness fixes (small, do first)

- [x] **VMAF model validation.** Reject unknown VMAF model names at startup
      rather than failing deep into an encode run. Known models are validated
      against the libvmaf catalog.
- [x] **FFmpeg version detection.** Validate minimum FFmpeg/libvmaf versions at
      startup and surface clear errors instead of cryptic encode failures.
- [ ] **Core algorithm tests.** Convex hull, BD-rate, Trellis allocation, and
      shot boundary detection are numerically tricky — property-based tests
      would catch regressions early.
- [ ] **Integration tests.** End-to-end `per-title analyze` + `per-title deliver`
      on a known reference clip with expected output.
- [ ] **VMAF model validation.** Check that the VMAF model file exists and is
      readable before starting encodes — currently fails mid-run.
- [ ] **FFmpeg version detection.** Validate minimum FFmpeg/libvmaf versions at
      startup and surface clear errors instead of cryptic encode failures.
- [ ] **10-bit pipeline correctness.** Verify that 10-bit content is correctly
      detected, warned about, and that VMAF scores account for bit depth
      differences between reference and distorted.

## P1 — highest-value features

- [ ] **Metric-by-metric comparison (MSU VQMT-class report).** Run the full
      metric suite on the same content and compare the *metrics against each
      other* — not just rank encodes — surfacing where PSNR and the perceptual
      metrics disagree. Most building blocks already exist; what remains is the
      unified report and a CLI surface.
  - [x] Per-component PSNR (Y/U/V + weighted `(6·Y + U + V) / 8`) in
        `viser-quality`.
  - [x] Pooling beyond the arithmetic mean — harmonic mean, p1/p5/p10, median,
        min/max — in `viser-quality::pool` (`PooledStats` / `PoolStrategy`).
  - [x] Metric-vs-metric correlation (Pearson, Spearman/SROCC, Kendall/KROCC)
        and divergence flagging in `viser-metrics` (`correlation_matrix`,
        `divergences`, `to_markdown`).
  - [x] Full-clip SSIMULACRA2/Butteraugli by default. `frame_samples == 0`
        (the new default) measures every frame in a single-pass batch extract;
        `--frame-samples N` stays as the speed/accuracy knob.
  - [x] Wire it into the CLI: `viser metrics compare -r ref enc_a enc_b … --all
        --pool harmonic --report {csv,json,html}` — ranks each encode per metric
        and prints the metric-vs-metric agreement on that ranking.
  - [x] Unified per-metric report (CSV/JSON/HTML) — emitted via `--report`
        (table to stdout, machine report to `--output` or stdout).
- [x] **Metric coverage parity with MSU VQMT.** Broadened the `Metric` enum
      (and the `Result`/`Pooled`/`FrameResult` structs) past VMAF/PSNR/SSIM/
      SSIMULACRA2/Butteraugli, all wired into `metrics compare`.
  - [x] **MS-SSIM** — multi-scale SSIM via libvmaf's `float_ms_ssim`; rides the
        existing VMAF pass.
  - [x] **VIF** — visual information fidelity, the mean of libvmaf's
        `*_vif_scale0..3`; computed alongside VMAF for free.
  - [x] **XPSNR** — perceptually-weighted PSNR `(6·Y+U+V)/8` via FFmpeg's
        `xpsnr` filter (a separate pass; under `--all`).
  - [x] **CAMBI** — Netflix's banding detector via libvmaf's `cambi` feature
        (lower is better; oriented "up" in the agreement matrix).
- [ ] **Two-pass VBR encoding.** CRF-only trial encodes today; ladders output
      CRF values but don't map to VBR bitrates for production.
- [ ] **HDR support (proper).** PQ/HLG handling, HDR-aware VMAF models. Current
      HDR detection gates behind `--allow-hdr` but VMAF scores are SDR-only.
- [ ] **Chunked/segmented encoding.** Distribute encodes across machines for
      long-form content. Currently in backlog.
- [ ] **Scene-complexity blending.** Per-shot analysis produces separate ladders
      per shot — no mechanism to smooth transitions or produce a single
      composite ladder.

## P2 — completeness

- [ ] **Differentiators beyond MSU VQMT parity.** Builds on the metric-
      comparison work in P1, but reaches past what MSU/psy-ex/ffmpeg-quality-
      metrics offer. Higher effort and lower certainty than the parity tiers —
      grouped here deliberately. None of this exists yet.
  - [~] **No-reference metrics.** `metrics no-ref` scores files with no pristine
        source. Done: a pure-Rust, model-free signal set — sharpness (variance
        of Laplacian), 8×8 blockiness, and Immerkær noise — in
        `viser-quality::noref`, streamed frame-by-frame from a `gray8` pipe.
    - [ ] **NIQE / BRISQUE proper.** These need their *published trained model
          parameters* (NIQE's pristine MVG; BRISQUE's SVR). Deferred rather than
          shipping numbers that match no oracle — embed the real model data or
          shell out to a reference implementation, then differential-test it.
  - [ ] **Faithfulness / hallucination metric (research-grade).** Distinguish
        recovered detail from *invented* detail in AI-enhanced output — the gap
        every existing metric is blind to (full-reference needs the missing
        source; no-reference rewards the confident fake). No oracle, so the
        evaluation protocol is part of the work. Candidate wedges: seed-
        disagreement heatmaps, round-trip re-degradation consistency, frequency-
        band attribution. This is the gatekeeper any future AI-enhancement
        pipeline can't ship without; treat as exploratory, multi-stage.
        Deferred deliberately: pure encoding never *adds* detail, so there is
        nothing to hallucinate until an AI-enhancement stage exists in the
        pipeline — premature to build against today.
  - [ ] **Pure-Rust + WASM measurement.** Replace the libvmaf/FFmpeg/CLI shell-
        outs with native implementations so the comparison player computes and
        overlays metrics in the browser — client-side metric comparison no other
        tool offers. The big lever is decode (today every metric path shells out
        to FFmpeg); the metric *math* is increasingly pure Rust already (pooling,
        correlation, the `noref` signals). Large effort; viser's unfair advantage
        if landed. See the FFmpeg-independence notes below.
  - [ ] **HDR-aware metric variants.** PQ/HLG-correct scoring across the suite;
        depends on and extends the **HDR support (proper)** item in P1.
- [x] **Hardware encoder support.** NVENC, QuickSync, VideoToolbox, VAAPI, AMF
      integration for GPU-accelerated encodes (H.264/H.265 only — AV1 HW is
      deferred to OxiMedia). Shipped in 0.6.0:

  - [x] *Runtime detection.* `ffmpeg -encoders` probed at CLI startup; available
        hardware encoders cached in a `OnceLock<HashSet<Codec>>`.
  - [x] *Codec enum.* Extended from 3 to 13 variants: 3 SW + 10 HW (5 backends
        × H.264/H.265). Added `EncoderBackend`, `CodecFamily`, `backends()`,
        `family()`, `is_hardware()`, `is_software()`.
  - [x] *Rate-control dispatch.* `build_sw_args()` / `build_hw_args()` in
        `viser-ffmpeg/src/encode.rs` with per-backend rate-control flags
        (NVENC `-cq -rc constqp`, QSV `-global_quality`, VideoToolbox
        `-quality`, VAAPI `-global_quality`, AMF `-qp_i / -qp_p`).
  - [x] *Preset mapping.* NVENC `p1`-`p7`, QSV passthrough, VAAPI
        `compression_level` 1-5, AMF `speed/balanced/quality`, in
        `viser-encoding/src/lib.rs`.
  - [x] *CLI integration.* All commands (`per-title analyze`, `per-title
        deliver`, `per-shot analyze`, `per-segment analyze`, `encode`) accept
        HW codec names and aliases (`nvenc`, `qsv`, `vt`, `vaapi`, `amf`,
        `videotoolbox`).
  - [x] *Chart labels.* `viser-chart` maps all 10 HW encoder names to
        human-readable labels (e.g., `h264_nvenc` → `H.264 (NVENC)`).

  **Scope boundary.** No AV1 hardware encoders — those belong to OxiMedia's
  royalty-free domain. No native FFI bindings — all HW encoding goes through
  the FFmpeg subprocess. GPU-accelerated VMAF remains deferred (libvmaf is
  CPU-only; no viable GPU path exists).
- [ ] **VP9 codec support.** Currently only H.264, H.265, and AV1.
- [ ] **ML-based ladder prediction.** Feature extraction from source to predict
      ladders without trial encodes (Bitmovin/Mux-style). `viser-complexity`
      measures spatial/temporal entropy but doesn't feed into prediction yet.
- [ ] **Streaming manifest output.** HLS/DASH playlist generation from ladder
      results.
- [ ] **Audio bitrate optimization.** Audio/video bitrate split in ladder budgets.
- [ ] **Screen content detection.** Slides, code, UI captures need different
      encoding strategies.

## P3 — quality of life

- [ ] **Charts in CLI.** `viser-chart` crate exists but isn't wired into the CLI.
- [ ] **Cost-aware optimization.** Storage + CDN delivery costs factored into
      ladder selection.
- [ ] **ABR logic integration.** Ladder selection tuned for client switching
      behavior and bandwidth distributions.
