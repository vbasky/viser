use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{CommandFactory, Parser, Subcommand};
use viser_encoding::clean_stale_temp_dirs;

#[derive(Parser)]
#[command(
    name = "viser",
    about = "",
    subcommand_required = false,
    before_help = "  📈 viser\n\n  🎬 video encoding optimizer\n  per-title analysis, per-shot refinement, quality metrics"
)]
struct Cli {
    /// Enable debug logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Encode a video file
    Encode(EncodeArgs),
    /// Inspect video files
    Inspect {
        #[command(subcommand)]
        command: InspectCommands,
    },
    /// Quality measurement tools
    Quality {
        #[command(subcommand)]
        command: QualityCommands,
    },
    /// Per-title encoding optimization
    #[command(name = "per-title")]
    PerTitle {
        #[command(subcommand)]
        command: PerTitleCommands,
    },
    /// Per-shot encoding optimization
    #[command(name = "per-shot")]
    PerShot {
        #[command(subcommand)]
        command: PerShotCommands,
    },
    /// Segment-level adaptive CRF encoding
    #[command(name = "per-segment", alias = "per-frame")]
    PerSegment {
        #[command(subcommand)]
        command: PerSegmentCommands,
    },
    /// Context-aware encoding (device-specific ladders)
    #[command(name = "context-aware")]
    ContextAware {
        #[command(subcommand)]
        command: ContextAwareCommands,
    },
    /// Launch side-by-side video comparison player
    Compare(CompareArgs),
}

// ── Encode ──

#[derive(Parser)]
struct EncodeArgs {
    /// Source video file
    #[arg(short, long)]
    input: String,
    /// Output file path
    #[arg(short, long)]
    output: String,
    /// Video codec
    #[arg(long, default_value = "libx264")]
    codec: String,
    /// Rate control mode (crf, capped-crf, qp, or vbr)
    #[arg(long, default_value = "crf")]
    mode: String,
    /// CRF or QP value for analysis-style modes
    #[arg(long, default_value_t = 23)]
    crf: i32,
    /// Target average bitrate in kbps for 2-pass VBR mode
    #[arg(long)]
    target_bitrate: Option<f64>,
    /// Peak bitrate cap in kbps for capped CRF mode
    #[arg(long)]
    max_bitrate: Option<f64>,
    /// VBV buffer size in kbps for capped CRF mode (defaults to 2x max bitrate)
    #[arg(long)]
    bufsize: Option<f64>,
    /// Encoding preset
    #[arg(long, default_value = "medium")]
    preset: String,
    /// Output width (0 = keep original)
    #[arg(long, default_value_t = 0)]
    width: i32,
    /// Output height (0 = keep original)
    #[arg(long, default_value_t = 0)]
    height: i32,
}

// ── Inspect ──

#[derive(Subcommand)]
enum InspectCommands {
    /// Show video file metadata via ffprobe
    Probe {
        /// Video file to inspect
        file: String,
    },
    /// Detect black frames in a video file
    BlackFrames {
        /// Video file to inspect
        file: String,
        /// Minimum duration of black to detect in seconds (default: 2.0)
        #[arg(long, default_value = "2.0")]
        duration: f64,
    },
    /// Measure EBU R128 loudness (integrated, short-term, true peak)
    Loudness {
        /// Audio or video file to measure
        file: String,
    },
}

// ── Quality ──

#[derive(Subcommand)]
enum QualityCommands {
    /// Measure quality (VMAF/PSNR/SSIM)
    Measure(QualityMeasureArgs),
}

#[derive(Parser)]
struct QualityMeasureArgs {
    /// Reference (original) video file
    #[arg(long)]
    reference: String,
    /// Distorted (encoded) video file
    #[arg(long)]
    distorted: String,
    /// VMAF frame subsampling (0 = every frame)
    #[arg(long, default_value_t = 0)]
    subsample: i32,
    /// VMAF model name
    #[arg(long, default_value = "vmaf_v0.6.1")]
    model: String,
    /// Include per-frame metrics
    #[arg(long)]
    per_frame: bool,
    /// Save results as JSON
    #[arg(short, long)]
    output: Option<String>,
}

// ── Per-Title ──

#[derive(Subcommand)]
enum PerTitleCommands {
    /// Run per-title analysis
    Analyze(PerTitleAnalyzeArgs),
    /// Encode final delivery ladder rungs from saved per-title analysis
    Deliver(PerTitleDeliverArgs),
}

#[derive(Parser)]
struct PerTitleAnalyzeArgs {
    /// Source video file
    #[arg(short, long)]
    input: String,
    /// Output JSON file
    #[arg(short, long)]
    output: Option<String>,
    /// Directory to save PNG charts
    #[arg(long)]
    charts: Option<String>,
    /// Codecs to test
    #[arg(long, value_delimiter = ',', default_values_t = vec!["libx264".to_string()])]
    codecs: Vec<String>,
    /// Resolutions to test
    #[arg(long, value_delimiter = ',', default_values_t = vec!["480p".to_string(), "720p".to_string(), "1080p".to_string()])]
    resolutions: Vec<String>,
    /// CRF values to test
    #[arg(long, value_delimiter = ',', default_values_t = vec![18, 22, 26, 30, 34, 38, 42])]
    crf_values: Vec<i32>,
    /// Encoding preset
    #[arg(long, default_value = "veryfast")]
    preset: String,
    /// VMAF frame subsampling
    #[arg(long, default_value_t = 5)]
    subsample: i32,
    /// Max parallel encodes
    #[arg(long, default_value_t = 2)]
    parallel: i32,
    /// Number of ladder rungs
    #[arg(long, default_value_t = 6)]
    rungs: i32,
    /// Minimum bitrate (kbps)
    #[arg(long, default_value_t = 200.0)]
    min_bitrate: f64,
    /// Maximum bitrate (kbps)
    #[arg(long, default_value_t = 8000.0)]
    max_bitrate: f64,
    /// Show what would be encoded without running
    #[arg(long)]
    dry_run: bool,
    /// Rate control mode (crf or qp)
    #[arg(long, default_value = "crf")]
    mode: String,
    /// Allow best-effort analysis on HDR sources
    #[arg(long)]
    allow_hdr: bool,
}

#[derive(Parser)]
struct PerTitleDeliverArgs {
    /// Saved per-title analysis JSON file
    #[arg(short, long)]
    analysis: String,
    /// Directory to write delivery encodes into
    #[arg(short = 'd', long)]
    output_dir: String,
    /// Override source video path from the analysis JSON
    #[arg(long)]
    source: Option<String>,
    /// Override preset used for delivery encodes
    #[arg(long)]
    preset: Option<String>,
    /// Delivery rate control mode (vbr or capped-crf)
    #[arg(long, default_value = "vbr")]
    mode: String,
    /// Number of delivery encodes to run in parallel (0 = auto)
    #[arg(long, default_value_t = 0)]
    parallel: i32,
    /// VBV buffer size as a multiple of the rung bitrate for capped CRF mode
    #[arg(long, default_value_t = 2.0)]
    bufsize_factor: f64,
    /// Encode ladder rungs in local chunks of this many seconds before concatenation
    #[arg(long)]
    chunk_seconds: Option<f64>,
    /// Optional manifest output path (defaults to <output-dir>/delivery_manifest.json)
    #[arg(long)]
    manifest: Option<String>,
    /// Output container extension (without dot)
    #[arg(long, default_value = "mp4")]
    extension: String,
    /// Show planned output files without encoding
    #[arg(long)]
    dry_run: bool,
    /// Allow delivery from HDR analyses/sources
    #[arg(long)]
    allow_hdr: bool,
}

// ── Per-Shot ──

#[derive(Subcommand)]
enum PerShotCommands {
    /// Detect shot boundaries
    Detect(PerShotDetectArgs),
    /// Run per-shot analysis with Trellis bit allocation
    Analyze(PerShotAnalyzeArgs),
}

#[derive(Parser)]
struct PerShotDetectArgs {
    /// Source video file
    #[arg(short, long)]
    input: String,
    /// Scene change threshold (0-100)
    #[arg(long, default_value_t = 10.0)]
    threshold: f64,
    /// Minimum shot duration in seconds
    #[arg(long, default_value_t = 0.5)]
    min_duration: f64,
}

#[derive(Parser)]
struct PerShotAnalyzeArgs {
    /// Source video file
    #[arg(short, long)]
    input: String,
    /// Output JSON file
    #[arg(short, long)]
    output: Option<String>,
    /// Scene change threshold
    #[arg(long, default_value_t = 10.0)]
    threshold: f64,
    /// Minimum shot duration (seconds)
    #[arg(long, default_value_t = 0.5)]
    min_duration: f64,
    /// Codecs to test
    #[arg(long, value_delimiter = ',', default_values_t = vec!["libx264".to_string()])]
    codecs: Vec<String>,
    /// Resolutions to test
    #[arg(long, value_delimiter = ',', default_values_t = vec!["480p".to_string(), "720p".to_string(), "1080p".to_string()])]
    resolutions: Vec<String>,
    /// CRF values to test
    #[arg(long, value_delimiter = ',', default_values_t = vec![22, 26, 30, 34, 38])]
    crf_values: Vec<i32>,
    /// Encoding preset
    #[arg(long, default_value = "veryfast")]
    preset: String,
    /// Target average bitrate for Trellis (kbps)
    #[arg(long, default_value_t = 2000.0)]
    target_bitrate: f64,
}

// ── Per-Segment ──

#[derive(Subcommand)]
enum PerSegmentCommands {
    /// Run segment-level adaptive CRF analysis
    Analyze(PerSegmentAnalyzeArgs),
}

#[derive(Parser)]
struct PerSegmentAnalyzeArgs {
    /// Source video file
    #[arg(short, long)]
    input: String,
    /// Video codec
    #[arg(long, default_value = "libx264")]
    codec: String,
    /// Encoding preset
    #[arg(long, default_value = "medium")]
    preset: String,
    /// Target VMAF quality
    #[arg(long, default_value_t = 93.0)]
    target_vmaf: f64,
    /// VMAF tolerance (+/-)
    #[arg(long, default_value_t = 2.0)]
    tolerance: f64,
    /// Minimum CRF (max quality)
    #[arg(long, default_value_t = 15)]
    min_crf: i32,
    /// Maximum CRF (min quality)
    #[arg(long, default_value_t = 45)]
    max_crf: i32,
    /// Max iterations per segment
    #[arg(long, default_value_t = 3)]
    max_iter: i32,
}

// ── Context-Aware ──

#[derive(Subcommand)]
enum ContextAwareCommands {
    /// Generate device-specific bitrate ladders
    Analyze(ContextAwareAnalyzeArgs),
}

#[derive(Parser)]
struct ContextAwareAnalyzeArgs {
    /// Source video file
    #[arg(short, long)]
    input: String,
    /// Encoding preset
    #[arg(long, default_value = "veryfast")]
    preset: String,
    /// Max parallel encodes
    #[arg(long, default_value_t = 2)]
    parallel: i32,
    /// Device profiles
    #[arg(long, value_delimiter = ',', default_values_t = vec!["mobile".to_string(), "desktop".to_string(), "tv".to_string()])]
    devices: Vec<String>,
}

// ── Compare ──

#[derive(Parser)]
struct CompareArgs {
    /// Reference (original) video file
    #[arg(long)]
    reference: String,
    /// Encoded video file
    #[arg(long)]
    encoded: String,
    /// Per-frame VMAF JSON file
    #[arg(long)]
    vmaf_data: Option<String>,
    /// HTTP port for the player
    #[arg(long, default_value_t = 8787)]
    port: u16,
}

// ── Main ──

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            if e.kind() == clap::error::ErrorKind::DisplayHelp
                || e.kind() == clap::error::ErrorKind::DisplayVersion
            {
                let _ = e.exit();
            } else {
                let _ = e.print();
            }
            println!();
            return Ok(());
        }
    };

    // Init logging
    let level: tracing::Level =
        if cli.verbose { tracing::Level::DEBUG } else { tracing::Level::WARN };
    tracing_subscriber::fmt().with_max_level(level).with_writer(std::io::stderr).init();

    // Clean stale temp dirs
    clean_stale_temp_dirs(Duration::from_secs(24 * 3600));

    let Some(command) = cli.command else {
        // No subcommand — show help and exit cleanly
        let _ = Cli::command().print_help();
        println!();
        return Ok(());
    };

    match command {
        Commands::Encode(args) => cmd_encode(args).await,
        Commands::Inspect { command } => match command {
            InspectCommands::Probe { file } => cmd_inspect_probe(&file).await,
            InspectCommands::BlackFrames { file, duration } => {
                cmd_inspect_blackframes(&file, duration).await
            }
            InspectCommands::Loudness { file } => cmd_inspect_loudness(&file).await,
        },
        Commands::Quality { command } => match command {
            QualityCommands::Measure(args) => cmd_quality_measure(args).await,
        },
        Commands::PerTitle { command } => match command {
            PerTitleCommands::Analyze(args) => cmd_pertitle_analyze(args).await,
            PerTitleCommands::Deliver(args) => cmd_pertitle_deliver(args).await,
        },
        Commands::PerShot { command } => match command {
            PerShotCommands::Detect(args) => cmd_pershot_detect(args).await,
            PerShotCommands::Analyze(args) => cmd_pershot_analyze(args).await,
        },
        Commands::PerSegment { command } => match command {
            PerSegmentCommands::Analyze(args) => cmd_persegment_analyze(args).await,
        },
        Commands::ContextAware { command } => match command {
            ContextAwareCommands::Analyze(args) => cmd_contextaware_analyze(args).await,
        },
        Commands::Compare(args) => cmd_compare(args).await,
    }
}

// ── Command Implementations ──

async fn cmd_encode(args: EncodeArgs) -> anyhow::Result<()> {
    let codec: viser_ffmpeg::Codec = args.codec.parse()?;
    let rate_control = match normalize_mode(&args.mode).as_str() {
        "crf" => viser_ffmpeg::RateControlMode::Crf,
        "capped-crf" => viser_ffmpeg::RateControlMode::CappedCrf,
        "qp" => viser_ffmpeg::RateControlMode::Qp,
        "vbr" => viser_ffmpeg::RateControlMode::Vbr,
        other => {
            anyhow::bail!("unsupported encode mode: {other} (expected crf, capped-crf, qp, or vbr)")
        }
    };

    let target_bitrate = args.target_bitrate.unwrap_or(0.0);
    let max_bitrate = args.max_bitrate.unwrap_or(0.0);
    let bufsize = args.bufsize.unwrap_or(0.0);
    if matches!(rate_control, viser_ffmpeg::RateControlMode::Vbr) && target_bitrate <= 0.0 {
        anyhow::bail!("--target-bitrate must be set to a positive value when --mode vbr");
    }
    if !matches!(rate_control, viser_ffmpeg::RateControlMode::Vbr) && args.target_bitrate.is_some()
    {
        anyhow::bail!("--target-bitrate is only valid with --mode vbr");
    }
    if matches!(rate_control, viser_ffmpeg::RateControlMode::CappedCrf) && max_bitrate <= 0.0 {
        anyhow::bail!("--max-bitrate must be set to a positive value when --mode capped-crf");
    }
    if !matches!(rate_control, viser_ffmpeg::RateControlMode::CappedCrf)
        && args.max_bitrate.is_some()
    {
        anyhow::bail!("--max-bitrate is only valid with --mode capped-crf");
    }
    if !matches!(rate_control, viser_ffmpeg::RateControlMode::CappedCrf) && args.bufsize.is_some() {
        anyhow::bail!("--bufsize is only valid with --mode capped-crf");
    }

    let resolution = if args.width > 0 && args.height > 0 {
        Some(viser_ffmpeg::Resolution::new(args.width, args.height))
    } else {
        None
    };

    let job = viser_ffmpeg::EncodeJob {
        input: args.input,
        output: args.output,
        resolution,
        codec,
        crf: args.crf,
        rate_control,
        target_bitrate,
        max_bitrate,
        bufsize,
        preset: args.preset,
        extra_args: vec![],
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<viser_ffmpeg::Progress>(10);
    tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            eprint!(
                "\rFrame: {}  FPS: {:.1}  Bitrate: {:.0} kbps  Speed: {:.1}x",
                p.frame, p.fps, p.bitrate, p.speed
            );
        }
    });

    let result = viser_ffmpeg::encode(job, Some(tx)).await?;
    eprintln!();
    println!("\nEncode complete:");
    println!("  Bitrate:   {:.0} kbps", result.bitrate);
    println!("  File size: {} bytes", result.file_size);
    println!("  Time:      {:.1}s", result.duration.as_secs_f64());
    Ok(())
}

async fn cmd_inspect_probe(file: &str) -> anyhow::Result<()> {
    let result = viser_ffmpeg::probe(file).await?;
    println!("File:     {}", result.format.filename);
    println!("Format:   {}", result.format.format_long_name);
    println!("Duration: {:.2}s", result.format.duration);
    println!("Size:     {} bytes", result.format.size);
    if result.format.bit_rate > 0 {
        println!("Bitrate:  {:.0} kbps", result.format.bit_rate as f64 / 1000.0);
    }
    for s in &result.streams {
        println!("\nStream #{}: {}", s.index, s.codec_type);
        println!(
            "  Codec: {}{}",
            s.codec_name,
            if s.profile.is_empty() { String::new() } else { format!(" ({})", s.profile) }
        );
        if s.codec_type == "video" {
            println!("  Resolution:  {}x{}", s.width, s.height);
            println!("  Pixel Fmt:   {}", s.pix_fmt);
            println!(
                "  Dynamic Range: {}",
                s.hdr_kind().map(|kind| format!("HDR ({kind})")).unwrap_or_else(|| "SDR".into())
            );
            if !s.color_transfer.is_empty() {
                println!("  Transfer:    {}", s.color_transfer);
            }
            if !s.color_primaries.is_empty() {
                println!("  Primaries:   {}", s.color_primaries);
            }
            if !s.color_space.is_empty() {
                println!("  Color Space: {}", s.color_space);
            }
            let fps = s.fps();
            if fps > 0.0 {
                println!("  Frame Rate:  {fps:.2} fps");
            }
            if s.nb_frames > 0 {
                println!("  Frames:      {}", s.nb_frames);
            }
        }
        if s.codec_type == "audio" {
            if s.sample_rate > 0 {
                println!("  Sample Rate: {} Hz", s.sample_rate);
            }
            if s.channels > 0 {
                println!("  Channels:    {}", s.channels);
            }
        }
    }
    Ok(())
}

async fn cmd_quality_measure(args: QualityMeasureArgs) -> anyhow::Result<()> {
    let opts = viser_quality::MeasureOpts {
        metrics: vec![
            viser_quality::Metric::Vmaf,
            viser_quality::Metric::Psnr,
            viser_quality::Metric::Ssim,
        ],
        subsample: args.subsample,
        model: args.model,
        per_frame: args.per_frame,
        probe_cache: None,
    };

    let result = viser_quality::measure(&args.reference, &args.distorted, opts).await?;
    println!("VMAF:  {:.2}", result.vmaf);
    println!("PSNR:  {:.2} dB", result.psnr);
    println!("SSIM:  {:.6}", result.ssim);

    if args.per_frame && !result.frames.is_empty() {
        println!("\nPer-frame: {} frames measured", result.frames.len());
        let limit = result.frames.len().min(20);
        for f in &result.frames[..limit] {
            println!(
                "  {:>6}  VMAF {:.2}  PSNR {:.2}  SSIM {:.6}",
                f.frame_num, f.vmaf, f.psnr, f.ssim
            );
        }
        if result.frames.len() > 20 {
            println!("  ... ({} more frames)", result.frames.len() - 20);
        }
    }

    if let Some(output) = args.output {
        let data = serde_json::to_string_pretty(&result)?;
        std::fs::write(&output, data)?;
        println!("\nResults saved to: {output}");
    }
    Ok(())
}

async fn cmd_pertitle_analyze(args: PerTitleAnalyzeArgs) -> anyhow::Result<()> {
    let resolutions = parse_resolutions(&args.resolutions)?;
    let codecs = parse_codecs(&args.codecs)?;

    let rate_control = match args.mode.as_str() {
        "qp" => viser_ffmpeg::RateControlMode::Qp,
        _ => viser_ffmpeg::RateControlMode::Crf,
    };

    let cfg = viser_pertitle::Config {
        encoding: viser_encoding::Config {
            resolutions: resolutions.clone(),
            crf_values: args.crf_values.clone(),
            codecs: codecs.clone(),
            preset: args.preset.clone(),
            subsample: args.subsample,
            parallel: args.parallel,
            rate_control,
        },
        ladder_opts: viser_ladder::Opts {
            num_rungs: args.rungs,
            min_bitrate: args.min_bitrate,
            max_bitrate: args.max_bitrate,
            min_vmaf: 40.0,
            max_vmaf: 97.0,
        },
        checkpoint_path: String::new(),
        vmaf_model: String::new(),
        allow_hdr: args.allow_hdr,
    };

    let total = resolutions.len() * codecs.len() * args.crf_values.len();
    println!("Viser Per-Title Analysis");
    println!("  Source:  {}", args.input);
    println!(
        "  Trials:  {} ({} res x {} CRF x {} codecs)",
        total,
        resolutions.len(),
        args.crf_values.len(),
        codecs.len()
    );

    if args.dry_run {
        println!("\n  [DRY RUN] Would encode {total} trials");
        return Ok(());
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<viser_pertitle::TrialProgress>(10);
    tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            eprint!(
                "\r  [{}/{}] {} {} CRF {} -> {:.0} kbps, VMAF {:.1}    ",
                p.done,
                p.total,
                p.resolution.label(),
                p.codec.as_str(),
                p.crf,
                p.bitrate,
                p.vmaf
            );
        }
    });

    let result = viser_pertitle::analyze(&args.input, cfg, Some(tx)).await?;
    eprintln!();

    for warning in &result.warnings {
        println!("\n  Warning: {warning}");
    }

    println!("\n  Convex Hull ({} points):", result.hull.points.len());
    for p in &result.hull.points {
        println!(
            "    {} {} CRF {} -> {:.0} kbps, VMAF {:.1}",
            p.resolution.label(),
            p.codec.as_str(),
            p.crf,
            p.bitrate,
            p.vmaf
        );
    }

    if !result.ladder.rungs.is_empty() {
        println!("\n  Optimized Ladder ({} rungs):", result.ladder.rungs.len());
        for r in &result.ladder.rungs {
            println!(
                "    #{} {} {} CRF {} -> {:.0} kbps, VMAF {:.1}",
                r.index + 1,
                r.point.resolution.label(),
                r.point.codec.as_str(),
                r.point.crf,
                r.point.bitrate,
                r.point.vmaf
            );
        }
    }

    if let Some(output) = args.output {
        result.save_json(&output)?;
        println!("\nResults saved to: {output}");
    }
    Ok(())
}

async fn cmd_pertitle_deliver(args: PerTitleDeliverArgs) -> anyhow::Result<()> {
    validate_file(&args.analysis)?;

    let result = viser_pertitle::Result::load_json(&args.analysis)?;
    if result.ladder.rungs.is_empty() {
        anyhow::bail!("analysis contains no ladder rungs to deliver");
    }

    let source = args.source.unwrap_or_else(|| result.source.clone());
    validate_file(&source)?;

    let delivery_mode = parse_delivery_mode(&args.mode)?;
    if args.bufsize_factor <= 0.0 {
        anyhow::bail!("--bufsize-factor must be greater than zero");
    }
    if let Some(chunk_seconds) = args.chunk_seconds {
        if chunk_seconds <= 0.0 {
            anyhow::bail!("--chunk-seconds must be greater than zero");
        }
    }

    if let Some(video) = result.source_info.video_stream() {
        if video.is_hdr() && !args.allow_hdr {
            anyhow::bail!(
                "HDR source detected ({}) in analysis/source. Delivery currently requires --allow-hdr for best-effort output.",
                video.hdr_kind().unwrap_or("HDR")
            );
        }
    }

    let preset = args.preset.unwrap_or_else(|| result.config.encoding.preset.clone());
    std::fs::create_dir_all(&args.output_dir)?;
    let manifest_path = args.manifest.clone().unwrap_or_else(|| {
        Path::new(&args.output_dir).join("delivery_manifest.json").display().to_string()
    });
    let parallel = effective_parallel(args.parallel);
    let source_duration = result.source_info.format.duration;

    println!("Viser Per-Title Delivery");
    println!("  Analysis: {}", args.analysis);
    println!("  Source:   {}", source);
    println!("  Rungs:    {}", result.ladder.rungs.len());
    println!("  Preset:   {}", preset);
    println!("  Mode:     {}", args.mode);
    println!("  Parallel: {}", parallel);
    if let Some(chunk_seconds) = args.chunk_seconds {
        println!("  Chunks:   {:.1}s", chunk_seconds);
    }
    for warning in &result.warnings {
        println!("  Warning:  {warning}");
    }

    let jobs: Vec<DeliveryPlan> = result
        .ladder
        .rungs
        .iter()
        .cloned()
        .map(|rung| {
            let output =
                build_delivery_output_path(&args.output_dir, &source, &rung, &args.extension);
            DeliveryPlan {
                rung_index: rung.index,
                resolution: rung.point.resolution.label(),
                codec: rung.point.codec.as_str().to_string(),
                crf: rung.point.crf,
                target_bitrate: rung.point.bitrate,
                target_vmaf: rung.point.vmaf,
                output: output.to_string_lossy().into_owned(),
                rung,
            }
        })
        .collect();

    if args.dry_run {
        println!("\n  [DRY RUN] Planned delivery encodes:");
        for job in &jobs {
            println!(
                "    #{} {} {} -> {:.0} kbps [{}{}] => {}",
                job.rung_index + 1,
                job.resolution,
                job.codec,
                job.target_bitrate,
                args.mode,
                args.chunk_seconds
                    .map(|seconds| format!(", chunked {seconds:.1}s"))
                    .unwrap_or_default(),
                job.output
            );
        }
        println!("\n  Manifest would be written to: {}", manifest_path);
        return Ok(());
    }

    for job in &jobs {
        println!(
            "\n  Queueing rung #{}: {} {} @ {:.0} kbps [{}{}]",
            job.rung_index + 1,
            job.resolution,
            job.codec,
            job.target_bitrate,
            args.mode,
            args.chunk_seconds
                .map(|seconds| format!(", chunked {seconds:.1}s"))
                .unwrap_or_default()
        );
    }

    let source = Arc::new(source);
    let preset = Arc::new(preset);
    let chunk_seconds = args.chunk_seconds;
    let bufsize_factor = args.bufsize_factor;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallel));
    let mut join_set = tokio::task::JoinSet::new();

    for job in jobs.iter().cloned() {
        let source = source.clone();
        let preset = preset.clone();
        let semaphore = semaphore.clone();
        join_set.spawn(async move {
            let _permit = semaphore.acquire_owned().await?;
            let encode_result = run_delivery_job(
                &job,
                source.as_ref().as_str(),
                preset.as_str(),
                delivery_mode,
                bufsize_factor,
                chunk_seconds,
                source_duration,
            )
            .await?;
            Ok::<DeliveryArtifact, anyhow::Error>(DeliveryArtifact {
                rung_index: job.rung_index,
                resolution: job.resolution,
                codec: job.codec,
                crf: job.crf,
                target_bitrate: job.target_bitrate,
                actual_bitrate: encode_result.bitrate,
                target_vmaf: job.target_vmaf,
                output: job.output,
                mode: mode_label(delivery_mode).to_string(),
                chunk_count: chunk_count(source_duration, chunk_seconds),
                duration_secs: encode_result.duration.as_secs_f64(),
            })
        });
    }

    let mut delivered = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        let artifact = joined??;
        println!(
            "    wrote {} ({:.0} kbps actual, {:.1}s)",
            artifact.output, artifact.actual_bitrate, artifact.duration_secs
        );
        delivered.push(artifact);
    }

    delivered.sort_by_key(|artifact| artifact.rung_index);
    let manifest = DeliveryManifest {
        analysis: args.analysis,
        source: (*source).clone(),
        output_dir: args.output_dir,
        preset: (*preset).clone(),
        mode: args.mode,
        chunk_seconds,
        extension: args.extension,
        generated_count: delivered.len(),
        artifacts: delivered,
    };
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    println!("\nManifest saved to: {}", manifest_path);

    Ok(())
}

async fn cmd_pershot_detect(args: PerShotDetectArgs) -> anyhow::Result<()> {
    let opts = viser_shot::DetectOpts {
        threshold: args.threshold,
        min_duration: Duration::from_secs_f64(args.min_duration),
    };

    println!("Detecting shots: {} (threshold={:.2})", args.input, args.threshold);
    let shots = viser_shot::detect(&args.input, opts).await?;

    println!("\nFound {} shots:", shots.len());
    for s in &shots {
        println!(
            "  #{}: {:.2}s - {:.2}s ({:.2}s)",
            s.index + 1,
            s.start.as_secs_f64(),
            s.end.as_secs_f64(),
            s.duration.as_secs_f64()
        );
    }
    Ok(())
}

async fn cmd_pershot_analyze(args: PerShotAnalyzeArgs) -> anyhow::Result<()> {
    let resolutions = parse_resolutions(&args.resolutions)?;
    let codecs = parse_codecs(&args.codecs)?;

    let cfg = viser_pershot::Config {
        encoding: viser_encoding::Config {
            resolutions,
            crf_values: args.crf_values,
            codecs,
            preset: args.preset,
            ..Default::default()
        },
        shot_opts: viser_shot::DetectOpts {
            threshold: args.threshold,
            min_duration: Duration::from_secs_f64(args.min_duration),
        },
        ladder_opts: viser_ladder::Opts::default(),
    };

    println!("Viser Per-Shot Analysis\n  Source: {}", args.input);

    let result = viser_pershot::analyze(&args.input, cfg, None).await?;
    println!(
        "  {} shots, {} trials, {:.1}s",
        result.shot_count,
        result.trial_count,
        result.duration.as_secs_f64()
    );

    if args.target_bitrate > 0.0 && result.shots.len() > 1 {
        let assignments = viser_pershot::trellis_optimize(
            &result.shots,
            &viser_pershot::TrellisOpts {
                target_bitrate: args.target_bitrate,
                ..Default::default()
            },
        );
        println!("\n  Trellis (target: {:.0} kbps):", args.target_bitrate);
        for a in &assignments {
            println!(
                "    Shot {} -> {} {} CRF {} {:.0} kbps VMAF {:.1}",
                a.shot_index + 1,
                a.resolution.label(),
                a.codec.as_str(),
                a.crf,
                a.bitrate,
                a.vmaf
            );
        }
    }
    Ok(())
}

async fn cmd_persegment_analyze(args: PerSegmentAnalyzeArgs) -> anyhow::Result<()> {
    let codec: viser_ffmpeg::Codec = args.codec.parse()?;
    let cfg = viser_persegment::Config {
        target_vmaf: args.target_vmaf,
        tolerance: args.tolerance,
        min_crf: args.min_crf,
        max_crf: args.max_crf,
        codec,
        resolution: None,
        preset: args.preset,
        segment_duration: Duration::from_secs(2),
        max_iterations: args.max_iter,
    };

    println!("Viser Segment-Level CRF Adaptation");
    println!("  Source:      {}", args.input);
    println!("  Target VMAF: {:.1} (+/- {:.1})", args.target_vmaf, args.tolerance);

    let result = viser_persegment::adapt(&args.input, cfg).await?;
    println!(
        "\n  {} segments, avg {:.0} kbps, avg VMAF {:.1}",
        result.segments.len(),
        result.avg_bitrate,
        result.avg_vmaf
    );
    Ok(())
}

async fn cmd_contextaware_analyze(args: ContextAwareAnalyzeArgs) -> anyhow::Result<()> {
    use viser_contextaware::*;

    let mut profiles = Vec::new();
    for d in &args.devices {
        match d.as_str() {
            "mobile" => profiles.push(mobile_profile()),
            "desktop" => profiles.push(desktop_profile()),
            "tv" => profiles.push(tv_profile()),
            "tv_4k" => profiles.push(tv_4k_profile()),
            _ => anyhow::bail!("unknown device: {d}"),
        }
    }

    let cfg = Config {
        profiles,
        crf_values: vec![18, 22, 26, 30, 34, 38, 42],
        preset: args.preset,
        subsample: 5,
        parallel: args.parallel,
    };

    println!("Viser Context-Aware Analysis\n  Source:  {}", args.input);
    let result = analyze(&args.input, cfg, None).await?;

    for dev in &result.devices {
        println!("\n  {} ({}):", dev.profile.name, dev.profile.description);
        for r in &dev.ladder.rungs {
            println!(
                "    #{} {} {} {:.0} kbps VMAF {:.1}",
                r.index + 1,
                r.point.resolution.label(),
                r.point.codec.as_str(),
                r.point.bitrate,
                r.point.vmaf
            );
        }
    }
    Ok(())
}

async fn cmd_compare(args: CompareArgs) -> anyhow::Result<()> {
    validate_file(&args.reference)?;
    validate_file(&args.encoded)?;

    viser_compare::serve(viser_compare::Opts {
        reference: args.reference,
        encoded: args.encoded,
        vmaf_data: args.vmaf_data.unwrap_or_default(),
        port: args.port,
    })
    .await
}

// ── Helpers ──

fn parse_resolutions(names: &[String]) -> anyhow::Result<Vec<viser_ffmpeg::Resolution>> {
    names.iter().map(|n| n.parse::<viser_ffmpeg::Resolution>()).collect()
}

fn parse_codecs(names: &[String]) -> anyhow::Result<Vec<viser_ffmpeg::Codec>> {
    names.iter().map(|n| n.parse::<viser_ffmpeg::Codec>()).collect()
}

fn build_delivery_output_path(
    output_dir: &str,
    source: &str,
    rung: &viser_ladder::Rung,
    extension: &str,
) -> PathBuf {
    let source_stem =
        Path::new(source).file_stem().and_then(|stem| stem.to_str()).unwrap_or("source");
    let ext = extension.trim_start_matches('.');
    let file_name = format!(
        "{source_stem}_rung{:02}_{}_{}_{:.0}k.{}",
        rung.index + 1,
        rung.point.resolution.label(),
        rung.point.codec.as_str(),
        rung.point.bitrate,
        ext
    );
    Path::new(output_dir).join(file_name)
}

fn effective_parallel(value: i32) -> usize {
    if value > 0 {
        return value as usize;
    }
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).max(1)
}

fn validate_file(path: &str) -> anyhow::Result<()> {
    if !Path::new(path).exists() {
        anyhow::bail!("file not found: {path}");
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct DeliveryPlan {
    rung_index: i32,
    resolution: String,
    codec: String,
    crf: i32,
    target_bitrate: f64,
    target_vmaf: f64,
    output: String,
    rung: viser_ladder::Rung,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DeliveryArtifact {
    rung_index: i32,
    resolution: String,
    codec: String,
    crf: i32,
    target_bitrate: f64,
    actual_bitrate: f64,
    target_vmaf: f64,
    output: String,
    mode: String,
    chunk_count: usize,
    duration_secs: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DeliveryManifest {
    analysis: String,
    source: String,
    output_dir: String,
    preset: String,
    mode: String,
    chunk_seconds: Option<f64>,
    extension: String,
    generated_count: usize,
    artifacts: Vec<DeliveryArtifact>,
}

async fn run_delivery_job(
    job: &DeliveryPlan,
    source: &str,
    preset: &str,
    mode: viser_ffmpeg::RateControlMode,
    bufsize_factor: f64,
    chunk_seconds: Option<f64>,
    source_duration: f64,
) -> anyhow::Result<viser_ffmpeg::EncodeResult> {
    if let Some(chunk_seconds) = chunk_seconds {
        return run_chunked_delivery_job(
            job,
            source,
            preset,
            mode,
            bufsize_factor,
            chunk_seconds,
            source_duration,
        )
        .await;
    }

    let encode_job =
        build_delivery_encode_job(job, source, &job.output, preset, mode, bufsize_factor, vec![]);
    viser_ffmpeg::encode(encode_job, None).await
}

async fn run_chunked_delivery_job(
    job: &DeliveryPlan,
    source: &str,
    preset: &str,
    mode: viser_ffmpeg::RateControlMode,
    bufsize_factor: f64,
    chunk_seconds: f64,
    source_duration: f64,
) -> anyhow::Result<viser_ffmpeg::EncodeResult> {
    let tmp_dir = tempfile::Builder::new().prefix("viser-delivery-").tempdir()?;
    let chunks = build_chunks(source_duration, chunk_seconds);
    let mut outputs = Vec::with_capacity(chunks.len());
    let started = std::time::Instant::now();

    for (index, (start, duration)) in chunks.iter().copied().enumerate() {
        let chunk_output = tmp_dir.path().join(format!("chunk_{index:03}.mp4"));
        let extra_args = vec![
            "-ss".to_string(),
            format!("{start:.6}"),
            "-t".to_string(),
            format!("{duration:.6}"),
        ];
        let encode_job = build_delivery_encode_job(
            job,
            source,
            &chunk_output.to_string_lossy(),
            preset,
            mode,
            bufsize_factor,
            extra_args,
        );
        viser_ffmpeg::encode(encode_job, None).await?;
        outputs.push(chunk_output.to_string_lossy().into_owned());
    }

    viser_ffmpeg::concat(&outputs, &job.output).await?;

    let meta = std::fs::metadata(&job.output)?;
    Ok(viser_ffmpeg::EncodeResult {
        job: build_delivery_encode_job(
            job,
            source,
            &job.output,
            preset,
            mode,
            bufsize_factor,
            vec![],
        ),
        bitrate: probe_average_bitrate(&job.output).await?,
        file_size: meta.len(),
        duration: started.elapsed(),
    })
}

fn build_delivery_encode_job(
    job: &DeliveryPlan,
    source: &str,
    output: &str,
    preset: &str,
    mode: viser_ffmpeg::RateControlMode,
    bufsize_factor: f64,
    extra_args: Vec<String>,
) -> viser_ffmpeg::EncodeJob {
    let max_bitrate = if matches!(mode, viser_ffmpeg::RateControlMode::CappedCrf) {
        job.rung.point.bitrate
    } else {
        0.0
    };
    let bufsize = if max_bitrate > 0.0 { max_bitrate * bufsize_factor } else { 0.0 };

    viser_ffmpeg::EncodeJob {
        input: source.to_string(),
        output: output.to_string(),
        resolution: Some(job.rung.point.resolution),
        codec: job.rung.point.codec,
        crf: job.rung.point.crf,
        rate_control: mode,
        target_bitrate: if matches!(mode, viser_ffmpeg::RateControlMode::Vbr) {
            job.rung.point.bitrate
        } else {
            0.0
        },
        max_bitrate,
        bufsize,
        preset: viser_encoding::preset_for_codec(job.rung.point.codec, preset),
        extra_args,
    }
}

async fn probe_average_bitrate(path: &str) -> anyhow::Result<f64> {
    Ok(viser_ffmpeg::probe(path).await?.format.bit_rate as f64 / 1000.0)
}

fn parse_delivery_mode(mode: &str) -> anyhow::Result<viser_ffmpeg::RateControlMode> {
    match normalize_mode(mode).as_str() {
        "vbr" => Ok(viser_ffmpeg::RateControlMode::Vbr),
        "capped-crf" => Ok(viser_ffmpeg::RateControlMode::CappedCrf),
        other => anyhow::bail!("unsupported delivery mode: {other} (expected vbr or capped-crf)"),
    }
}

fn normalize_mode(mode: &str) -> String {
    mode.trim().to_ascii_lowercase().replace('_', "-")
}

fn mode_label(mode: viser_ffmpeg::RateControlMode) -> &'static str {
    match mode {
        viser_ffmpeg::RateControlMode::Crf => "crf",
        viser_ffmpeg::RateControlMode::CappedCrf => "capped-crf",
        viser_ffmpeg::RateControlMode::Qp => "qp",
        viser_ffmpeg::RateControlMode::Vbr => "vbr",
    }
}

fn build_chunks(duration: f64, chunk_seconds: f64) -> Vec<(f64, f64)> {
    if duration <= 0.0 || chunk_seconds <= 0.0 {
        return vec![];
    }

    let mut start = 0.0;
    let mut chunks = Vec::new();
    while start < duration {
        let remaining = duration - start;
        let chunk_duration = remaining.min(chunk_seconds);
        chunks.push((start, chunk_duration));
        start += chunk_duration;
    }
    chunks
}

fn chunk_count(duration: f64, chunk_seconds: Option<f64>) -> usize {
    chunk_seconds.map(|seconds| build_chunks(duration, seconds).len()).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_delivery_output_path() {
        let rung = viser_ladder::Rung {
            point: viser_hull::Point {
                resolution: viser_ffmpeg::RES_1080P,
                codec: viser_ffmpeg::Codec::X264,
                crf: 23,
                bitrate: 3000.0,
                vmaf: 95.0,
                psnr: 40.0,
                ssim: 0.99,
            },
            index: 1,
        };

        let path = build_delivery_output_path("dist", "clips/demo.y4m", &rung, "mp4");
        assert_eq!(path, Path::new("dist").join("demo_rung02_1080p_libx264_3000k.mp4"));
    }

    #[test]
    fn test_effective_parallel_uses_explicit_value() {
        assert_eq!(effective_parallel(3), 3);
    }

    #[test]
    fn test_effective_parallel_auto_is_at_least_one() {
        assert!(effective_parallel(0) >= 1);
    }

    #[test]
    fn test_build_chunks_splits_remainder() {
        assert_eq!(build_chunks(25.0, 10.0), vec![(0.0, 10.0), (10.0, 10.0), (20.0, 5.0)]);
    }

    #[test]
    fn test_parse_delivery_mode_accepts_capped_crf_alias() {
        assert_eq!(
            parse_delivery_mode("capped_crf").unwrap(),
            viser_ffmpeg::RateControlMode::CappedCrf
        );
    }
}

async fn cmd_inspect_blackframes(file: &str, min_duration: f64) -> anyhow::Result<()> {
    let filter = format!("blackdetect=d={}:pic_th=0.98", min_duration);
    let output = tokio::process::Command::new(viser_ffmpeg::ffmpeg_path())
        .args(["-i", file, "-vf", &filter, "-f", "null", "-"])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("blackdetect failed: {stderr}");
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut found = false;

    for line in stderr.lines() {
        let line = line.trim();
        if line.contains("black_start:")
            || line.contains("black_end:")
            || line.contains("black_duration:")
        {
            if !found {
                println!("Black frame intervals:");
                found = true;
            }
            println!("  {line}");
        }
    }

    if !found {
        println!("No black frames detected (min duration: {min_duration}s).");
    }

    Ok(())
}

async fn cmd_inspect_loudness(file: &str) -> anyhow::Result<()> {
    let output = tokio::process::Command::new(viser_ffmpeg::ffmpeg_path())
        .args(["-i", file, "-af", "ebur128", "-f", "null", "-"])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("EBU R128 loudness measurement failed: {stderr}");
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("EBU R128 Loudness Report");
    println!("{}", "-".repeat(30));
    for line in stderr.lines() {
        let line = line.trim();
        if line.starts_with("I:") || line.starts_with("LRA:") || line.starts_with("L") {
            println!("  {line}");
        }
    }

    Ok(())
}
