# viser roadmap

viser today is a **content-adaptive VOD encoding toolkit**: probe, analyze,
encode, and measure quality — all from a single Rust binary. This document
tracks what's covered, what's missing, and what's planned.

Priorities are ordered by impact; checkboxes track status. Nothing here is a
commitment to a date.

## Status snapshot

**Covered:** per-title convex-hull analysis, per-shot Trellis bit allocation,
segment-level CRF tuning, content-adaptive encoding profiles, shot detection
(FFmpeg scdet), CRF / capped-CRF / fixed-QP / two-pass VBR encoding,
checkpoint/resume, audio-bitrate-aware ladder budgets, screen-content
detection, an optional pure-Rust probe engine (`revelo`), a broad quality-
metric suite (VMAF, PSNR, SSIM, MS-SSIM, VIF, XPSNR, CAMBI, SSIMULACRA2,
butteraugli + no-reference signals) with metric-vs-metric comparison, the
comparison player, and hardware encoder support (NVENC, QuickSync,
VideoToolbox, VAAPI, AMF — H.264/H.265).

**Not covered:** see tiers below.

---

## P0 — correctness fixes (small, do first)

- [x] **VMAF model validation.** Reject unknown VMAF model names at startup
      rather than failing deep into an encode run. Known models are validated
      against the libvmaf catalog.
- [x] **FFmpeg version detection.** Validate minimum FFmpeg/libvmaf versions at
      startup and surface clear errors instead of cryptic encode failures.
- [x] **Core algorithm tests.** Convex hull, BD-rate, Trellis allocation, and
      ladder selection are covered by a 170+ test suite, plus property-based
      (`proptest`) invariant tests for the convex hull and ladder selection.
- [x] **Integration tests.** FATE-style end-to-end tests generate synthetic
      media with `ffmpeg -f lavfi` and exercise the full probe → encode →
      measure pipeline against real ffmpeg/ffprobe.
- [x] **10-bit pipeline correctness.** 10-bit content is detected
      (`bit_depth`), preserved through encode (output pix_fmt stays 10-bit),
      and scored bit-depth-aware: `resolve_scoring_plan` keeps the native
      high-bit-depth format for VMAF/PSNR, warns when reference and distorted
      depths differ, and `psnr_peak` tracks the scoring depth. Covered by
      `fate_10bit.rs` and `viser-quality::scoring` unit tests.

## P1 — highest-value features

- [x] **Metric-by-metric comparison (MSU VQMT-class report).** Run the full
      metric suite on the same content and compare the *metrics against each
      other* — not just rank encodes — surfacing where PSNR and the perceptual
      metrics disagree. Shipped via `viser metrics compare` with a unified
      per-metric report and an agreement matrix; sub-items below.
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
- [x] **Two-pass VBR encoding.** `RateControlMode::Vbr` runs a two-pass encode
      against a target bitrate (`encode_two_pass`), alongside CRF, capped-CRF,
      and fixed-QP modes; per-title delivery maps saved analyses to VBR rungs.
- [~] **HDR support (proper).** PQ/HLG handling, HDR-aware VMAF models.
  - [x] HDR10 static-metadata preservation. `viser-ffmpeg::hdr` extracts
        mastering-display colour volume (SMPTE ST 2086) and MaxCLL/MaxFALL from
        the source's frame side data; `SourceFormat::enrich_hdr10` attaches it
        across the per-title/per-segment/delivery pipelines. x265 re-signals it
        via `master-display` / `max-cll`, and SVT-AV1 via `-svtav1-params`
        `mastering-display` / `content-light` (real-valued grammar, with the
        rate-control `-svtav1-params` coalesced). FATE round-trips verify both
        codecs survive a re-encode.
  - [x] HDR-aware scoring via tonemap-to-BT.709 (`--hdr-scoring`), shipped with
        the 0.9.0 10-bit/HDR work.
  - [ ] Native HDR-domain VMAF (no SDR tonemap) and HDR10 passthrough on the
        hardware encoders (NVENC/QSV/VAAPI/AMF). libvmaf ships no official HDR
        model, so the scoring side needs a PQ-domain path and differential
        validation; the HW encoders need per-backend metadata flags (no shared
        `master-display` string like the software encoders).
- [ ] **Chunked/segmented encoding.** Distribute encodes across machines for
      long-form content. Currently in backlog.
- [ ] **Scene-complexity blending.** Per-shot analysis produces separate ladders
      per shot — no mechanism to smooth transitions or produce a single
      composite ladder.

## P2 — completeness

- [ ] **Differentiators beyond MSU VQMT parity.** Builds on the metric-
      comparison work in P1, but reaches past what MSU/psy-ex/ffmpeg-quality-
      metrics offer. Higher effort and lower certainty than the parity tiers —
      grouped here deliberately. Mostly unbuilt — only the no-reference signal
      set has landed so far.
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
- [x] **Hardware encode/decode matrix.** NVENC, QuickSync, VideoToolbox, VAAPI,
      AMF integration for GPU-accelerated encodes across H.264, H.265, and AV1,
      plus hardware-accelerated decode. Shipped in 0.6.0 (H.264/H.265 encode),
      completed in 0.7.0 (AV1 encode row + VAAPI surface plumbing + decode axis):

  - [x] *Runtime detection.* `ffmpeg -encoders` and `ffmpeg -hwaccels` probed at
        CLI startup; available encoders/decoders cached in `OnceLock` sets.
  - [x] *Codec enum.* 17 variants: 3 SW + 14 HW. H.264/H.265 across all 5
        backends, plus AV1 across NVENC/QSV/VAAPI/AMF (no `av1_videotoolbox` —
        Apple has no AV1 encoder). `EncoderBackend`, `CodecFamily`, `backend()`,
        `family()`, `is_hardware()`, `is_software()`.
  - [x] *Rate-control dispatch.* `build_sw_args()` / `build_hw_args()` in
        `viser-ffmpeg/src/encode.rs` with per-backend rate-control flags
        (NVENC `-cq -rc constqp`, QSV `-global_quality`, VideoToolbox
        `-quality`, VAAPI `-global_quality`, AMF `-qp_i / -qp_p`). Backend-keyed,
        so the AV1 row reuses the existing dispatch.
  - [x] *VAAPI surface plumbing.* `-vaapi_device` initialised before `-i`
        (overridable via `VISER_VAAPI_DEVICE`), and a unified `-vf` filter chain
        appends `format=nv12,hwupload` so the encoder receives VAAPI surfaces.
  - [x] *Hardware decode.* `EncodeJob.hwaccel` injects `-hwaccel <method>` before
        the input (frames downloaded to system memory, keeping the SW filter
        pipeline intact). `encode --hwaccel` flag; detection via `-hwaccels`.
  - [x] *Preset mapping.* NVENC `p1`-`p7`, QSV passthrough, VAAPI
        `compression_level` 1-5, AMF `speed/balanced/quality`, in
        `viser-encoding/src/lib.rs`.
  - [x] *CLI integration.* All commands (`per-title analyze`, `per-title
        deliver`, `per-shot analyze`, `per-segment analyze`, `encode`) accept
        HW codec names and aliases (`nvenc`, `qsv`, `vt`, `vaapi`, `amf`,
        `videotoolbox`, plus `av1_nvenc` / `av1_qsv` / `av1_vaapi` / `av1_amf`).
  - [x] *Chart labels.* `viser-chart` maps all 14 HW encoder names to
        human-readable labels (e.g., `h264_nvenc` → `H.264 (NVENC)`,
        `av1_vaapi` → `AV1 (VAAPI)`).

  **Scope boundary.** No native FFI bindings — all HW encode/decode goes through
  the FFmpeg subprocess. AV1 HW encode requires recent silicon (Arc/Battlemage,
  Ada/Blackwell, RDNA3+) and is validated at the argument level; real-GPU
  validation needs hardware in CI. GPU-accelerated VMAF remains deferred
  (libvmaf is CPU-only; no viable GPU path exists).
- [ ] **VP9 codec support.** Currently only H.264, H.265, and AV1.
- [ ] **ML-based ladder prediction.** Feature extraction from source to predict
      ladders without trial encodes (Bitmovin/Mux-style). `viser-complexity`
      measures spatial/temporal entropy but doesn't feed into prediction yet.
- [ ] **Streaming manifest output.** HLS/DASH playlist generation from ladder
      results.
- [x] **Audio bitrate optimization.** Per-title analysis extracts source audio
      bitrate (`audio_bitrate_kbps`) and reserves it in the delivery budget, so
      ladder rungs are sized against the video budget alone.
- [x] **Screen content detection.** `viser-complexity::detect_screen_content`
      classifies content as natural vs. screen (slides, code, UI) from
      spatial/temporal/DCT heuristics. (Detection only — not yet used to switch
      encoding strategy automatically.)

## P3 — quality of life

- [~] **Charts in CLI.** `per-title analyze --charts <dir>` emits charts via
      `viser-chart`; still missing a dedicated `chart` subcommand and chart
      output for the other analysis modes.
- [ ] **Cost-aware optimization.** Storage + CDN delivery costs factored into
      ladder selection.
- [ ] **ABR logic integration.** Ladder selection tuned for client switching
      behavior and bandwidth distributions.
