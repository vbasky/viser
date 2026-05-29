# viser — Robustness Assessment

## ✅ Strengths

- **🏗️ Well-architected** — 15 focused crates with clear boundaries (FFmpeg wrapper, convex hull, ladder selection, shot detection, quality measurement, checkpointing all in separate crates).
- **🏭 Production lineage** — ported from a prior Go implementation that ran in production; algorithms are battle-tested, not greenfield.
- **💾 Checkpoint/resume** — SHA-256 hashed configs + atomic writes; multi-hour runs survive crashes.
- **⚡ Async parallelism** — tokio + semaphore-controlled concurrency, not naive blocking loops.
- **🛡️ Good error handling** — structured result types, no blind unwrapping, 10-bit content detection with user warnings.

## ⚠️ Weaknesses & Rough Edges

- **🧪 Zero tests in the crates** — the core algorithms (convex hull, BD-rate) are numerically tricky and should have property-based tests.
- **📊 Charts not wired into CLI** — `viser-chart` exists but isn't reachable from the CLI yet (backlog item).
- **🎬 Only 3 codecs** — H.264, H.265, AV1. No VP9, no hardware encoders (NVENC, QuickSync).
- **🔧 FFmpeg dependency** — wraps FFmpeg binaries; requires FFmpeg compiled with libvmaf installed separately, which is the biggest barrier to `cargo install && run`.
- **👤 Single maintainer** — project structure suggests one developer's deep work; bus factor is 1.
- **🔄 No CI/CD visible** — no GitHub Actions or test infrastructure evident.

## 🧩 Feature Gaps vs. Commercial Per-Title Tools

- **🔮 No ML prediction** — every encode starts from scratch. Bitmovin/Mux predict ladders from source features without trial encodes. viser does zero feature extraction for prediction — `viser-complexity` measures spatial/temporal entropy but only feeds into per-segment CRF targeting, not ladder prediction. This means per-title analysis is always slow (hours for a 2-hour movie with 42+ trial encodes).
- **🎚️ No two-pass VBR** — CRF-only for trial encodes. Ladders output CRF values but don't map them to VBR bitrates for production encoding.
- **🌅 No HDR support** — no PQ/HLG handling, no HDR-aware VMAF models. Feeding HDR content produces incorrect VMAF scores (default libvmaf assumes SDR).
- **💻 No hardware encoders** — NVENC, QuickSync, VideoToolbox absent. Software encoders only (x264, x265, SVT-AV1).
- **🧩 No chunked/segmented encoding** — backlog item; can't distribute encodes across machines for long-form content.
- **🔄 No scene-complexity blending** — per-shot analysis exists but produces separate ladders per shot. No mechanism to smooth transitions or produce a single composite ladder from per-shot data.
- **🔇 No audio bitrate optimization** — pure video; no audio/video bitrate split.
- **🖥️ No screen content awareness** — slides, code, UI captures need different encoding strategies than natural video.
- **📦 No streaming manifest output** — produces JSON ladders, not HLS/DASH manifests. Requires a separate packager step.
- **📶 No ABR logic integration** — ladder selection is purely quality-spaced, not tuned for client switching behavior or bandwidth distributions.
- **💰 No cost-aware optimization** — storage + CDN delivery costs not factored in.

## 📐 How viser Compares

viser's architecture is closest to Netflix's original per-title approach (trial encodes → VMAF → convex hull → ladder), which is the most accurate but computationally expensive. Commercial alternatives (Bitmovin, Mux) use ML prediction to skip trial encodes and estimate ladders from source features, trading accuracy for speed.

viser goes further than Netflix's basic per-title approach in two areas:

1. **🎯 Per-shot Trellis optimization** — Lagrangian bit allocation across scenes. Netflix's original approach optimizes per-title only (one ladder for the whole video). viser detects shot boundaries and allocates bits optimally across shots using a constant-slope lambda search. Some commercial tools offer per-shot optimization (e.g., Beamr, which pioneered shot-based encoding), though it is not universal — especially the combination with convex-hull-based per-shot ladder selection.

2. **🎛️ Per-segment closed-loop CRF targeting** — binary-search CRF per 1-second segment with VMAF verification. This is finer granularity than most commercial or open-source tools offer, which typically optimize at the title or shot level rather than at the sub-shot segment level.

These are genuine differentiators, but they also add complexity and encode time. Whether they matter depends on the use case: per-shot optimization is valuable for feature films with diverse scenes; per-segment CRF tuning helps maintain consistent quality on variable-complexity content.
