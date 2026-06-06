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
encoding profiles, segment-level CRF tuning, comparison player.

**Not covered:** see tiers below.

---

## P0 — correctness fixes (small, do first)

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

- [ ] **Hardware encoder support.** NVENC, QuickSync, VideoToolbox integration
      for GPU-accelerated encodes.
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
