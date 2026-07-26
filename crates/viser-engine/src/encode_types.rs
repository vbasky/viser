use std::time::Duration;

use crate::{Codec, RateControlMode, Resolution, SourceFormat, StreamInfo};

/// Parameters for a single encode.
///
/// Fields such as `extra_args` and `hwaccel` are engine hints: the FFmpeg backend
/// interprets them as FFmpeg CLI flags / hwaccel methods; other engines may ignore
/// or reinterpret them.
#[derive(Debug, Clone)]
pub struct EncodeJob {
    /// Source media file path.
    pub input: String,
    /// Destination file path for the encoded output.
    pub output: String,
    /// Optional target resolution; when set, scales appropriately for the engine.
    pub resolution: Option<Resolution>,
    /// Video codec to encode with.
    pub codec: Codec,
    /// Constant rate factor / quantizer value (interpretation depends on `rate_control`).
    pub crf: i32,
    /// Rate-control mode that determines how `crf`/bitrate fields are applied.
    pub rate_control: RateControlMode,
    /// Target bitrate in kbps; used for VBR mode.
    pub target_bitrate: f64, // kbps, used for VBR mode
    /// Maximum bitrate cap in kbps; used for capped CRF mode.
    pub max_bitrate: f64, // kbps, used for capped CRF mode
    /// VBV buffer size in kbps; used for capped CRF mode.
    pub bufsize: f64, // kbps, used for capped CRF mode
    /// Encoder speed preset (e.g. `"medium"`); empty leaves the encoder default.
    pub preset: String,
    /// Optional hardware-accelerated decode method (e.g. `"vaapi"`, `"cuda"`,
    /// `"qsv"`, `"videotoolbox"`). `None` (or empty) decodes in software.
    /// Frames are downloaded to system memory for the filter/encode pipeline.
    pub hwaccel: Option<String>,
    /// Extra engine-specific arguments appended verbatim before the output path
    /// (FFmpeg flags, external CLI tokens, …).
    pub extra_args: Vec<String>,
    /// Source color/bit-depth characteristics to preserve in the output encode.
    pub source_format: Option<SourceFormat>,
}

impl EncodeJob {
    /// Attaches probed source video characteristics for bit-depth/HDR preservation.
    pub fn with_source_video(mut self, video: &StreamInfo) -> Self {
        self.source_format = Some(SourceFormat::from_stream(video));
        self
    }
}

/// Output of a completed encode.
#[derive(Debug, Clone)]
pub struct EncodeResult {
    /// The job that produced this result.
    pub job: EncodeJob,
    /// Average bitrate of the output in kbps, measured by probing it.
    pub bitrate: f64, // kbps (average)
    /// Output file size in bytes.
    pub file_size: u64, // bytes
    /// Wall-clock time taken to encode.
    pub duration: Duration, // wall-clock encode time
}

/// Real-time encoding progress info.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// Number of frames encoded so far.
    pub frame: i64,
    /// Current encoding rate in frames per second.
    pub fps: f64,
    /// Current output bitrate in kbps.
    pub bitrate: f64, // kbps
    /// Encoding speed relative to real time (e.g. 2.5 means 2.5x).
    pub speed: f64, // e.g. 2.5x
    /// Output timestamp reached so far.
    pub time: Duration,
}

/// Splits a source duration into a list of `(start_seconds, chunk_duration_seconds)` tuples.
///
/// Each chunk is `chunk_seconds` long except the last, which is shortened to fit
/// the remaining duration. Returns an empty vec when `duration` or `chunk_seconds`
/// is non-positive.
pub fn chunk_plan(duration: f64, chunk_seconds: f64) -> Vec<(f64, f64)> {
    if duration <= 0.0 || chunk_seconds <= 0.0 {
        return vec![];
    }
    let mut start = 0.0;
    let mut chunks = Vec::new();
    while start < duration {
        let remaining = duration - start;
        let cd = remaining.min(chunk_seconds);
        chunks.push((start, cd));
        start += cd;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_plan_basic() {
        assert_eq!(chunk_plan(25.0, 10.0), vec![(0.0, 10.0), (10.0, 10.0), (20.0, 5.0)]);
        assert!(chunk_plan(0.0, 10.0).is_empty());
    }
}
