# Research Note 12 — Chunked Encoding Collation

## What is collation?

Chunked encoding splits a source video into independent segments, encodes them in parallel, and then combines the resulting per-chunk outputs into final renditions. **Collation** is the combination step: taking N encoded chunks for a single bitrate rung and producing one continuous, playable, spec-compliant stream.

Collation is the inverse of chunking. If chunking is about finding clean cut points, collation is about making sure those cuts are invisible in the final output.

## Why it matters

The speedup from chunked encoding is only useful if the assembled output is bit-identical or perceptually equivalent to a single-pass encode of the same parameters. Collation errors show up as:

- Visible glitches at chunk boundaries
- Audio drift or pops
- Broken seeking
- Players stalling because of timestamp discontinuities
- Container metadata mismatches (different SAR, color info, etc.)

## Core requirements

### 1. Seamless video boundaries

Each chunk must end and the next must begin on a closed Group of Pictures (GOP). Typically this means:

- The last frame of chunk N is a closed GOP end
- The first frame of chunk N+1 is an IDR frame
- Both chunks use the same codec profile, level, color description, and SAR

If these do not match, the concat demuxer may still succeed, but decoders can drop frames or show macroblock artifacts at the boundary.

### 2. Continuous timestamps

When concatenating with FFmpeg's `concat` demuxer, output timestamps are the sum of each segment's declared duration. If chunk durations are slightly off due to frame-rate rounding, the final stream drifts. For CFR content this is usually safe; for VFR or telecined content it is a common source of sync issues.

### 3. Audio alignment

Audio chunks must:

- Start and end on packet boundaries
- Have the same sample rate, channel layout, and codec
- Have durations that exactly span the video chunk duration

A common bug is audio trailing by a few samples past the video boundary. The next chunk's audio then overlaps or leaves a gap, producing clicks or A/V desync.

### 4. Consistent codec parameters

Each chunk must be encoded with the same:

- Codec and encoder (libx264, libsvtav1, etc.)
- Profile, level, tier
- Pixel format and color range/matrix/transfer/primaries
- Frame rate and time base
- SAR / DAR

If any of these differ, the concat demuxer may refuse to concatenate or players may fail at the transition.

## Collation strategies

### Strategy A: FFmpeg concat demuxer (copy)

```bash
ffmpeg -f concat -safe 0 -i chunks.txt -c copy output.mp4
```

Fastest, but requires perfectly compatible chunks. viser uses this path for local chunked delivery after forcing IDR frames at chunk boundaries.

### Strategy B: Transcode boundaries only

Copy the interior of each chunk and re-encode a few GOPs around each boundary to guarantee a closed transition. Slower than pure copy, but more tolerant of parameter drift.

### Strategy C: Full remux with timestamp rewriting

Use `ffmpeg -fflags +genpts` or equivalent to regenerate timestamps across the whole timeline. Useful when chunk timestamps are known to be consistent relative to each other but not absolutely continuous.

### Strategy D: Container-level stitching (DASH/HLS)

Instead of producing one MP4, keep chunks separate and serve them through a manifest. Collation is replaced by manifest authoring. This is what most streaming services do at scale, but it is out of scope for viser's local MP4 delivery path.

## viser implementation

viser's chunked delivery pipeline currently does the following:

1. **Shot detection** groups frames into shots.
2. **Chunking** groups shots into target-duration chunks without splitting shots.
3. **Per-chunk encoding** runs in parallel for each rung, using the same codec parameters across all chunks.
4. **Collation** concatenates per-rung chunks with `ffmpeg concat` using a generated list file.

The `viser-ffmpeg::concat` helper handles the concat-list format, escaping single quotes and backslashes in file paths. See `crates/viser-ffmpeg/src/encode.rs` for the implementation.

## Open questions / risks

- **Audio packet alignment**: Current validation is limited. Long-form content with non-integer frame durations may expose drift.
- **HDR boundary consistency**: Per-shot `allow_hdr` and different color metadata between chunks could break concat copy mode.
- **VFR sources**: Timestamps from independently extracted chunks may not sum exactly to the source duration.
- **Fault tolerance**: If one chunk encode fails or produces a different duration, the concat list must be validated before assembly.

## Recommendations

1. Always force IDR at chunk boundaries.
2. Validate that all chunks share identical codec parameters before concat.
3. Verify total output duration matches the source duration within one frame.
4. For production use, prefer manifest-based delivery over physical concatenation when possible.
5. Add a post-concat probe step that checks for timestamp discontinuities and A/V sync.

## References

### Industry practice

- Netflix, [Optimized Shot-Based Encodes](https://netflixtechblog.com/optimized-shot-based-encodes-now-streaming-4b9464204830)
- Netflix, [Per-Title Encode Optimization](https://netflixtechblog.com/per-title-encode-optimization-7e99450b2588)
- Bitmovin, [Split and Stitch Encoding](https://bitmovin.com/blog/split-and-stitch-encoding/)
- FFmpeg, [concat demuxer documentation](https://ffmpeg.org/ffmpeg-formats.html#concat)

### Research literature

- Giladi et al., [Massively Parallel Open Source Encoding for Adaptive Streaming](https://journal.smpte.org/conferences/SMPTE%202018/21/), SMPTE 2018 — introduces the chunk encode "joblet" and demonstrates distributed encoding with minimal quality impact.
- Neugebauer, [Nagare Media Engine: A System for Cloud- and Edge-Native Network-based Multimedia Workflows](https://arxiv.org/abs/2509.24546), arXiv 2025 — standards-based NBMP workflow system for distributed media processing.
- Li et al., [Performance Analysis and Modeling of Video Transcoding Using Heterogeneous Cloud Services](https://arxiv.org/abs/1809.06529), arXiv 2018 — models cloud transcoding performance across heterogeneous compute instances.
- Shu et al., [Predicting total time to compress a video corpus using online inference systems](https://arxiv.org/abs/2410.18260), IEEE VCIP 2024 — corpus-level transcoding time prediction for cloud VOD cost management.
- Durbha et al., [Leveraging Compression to Construct Transferable Bitrate Ladders](https://arxiv.org/abs/2512.12952), arXiv 2025 — ML-based per-shot bitrate ladder construction and convex-hull approximation.
