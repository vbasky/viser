use std::path::Path;
use std::time::Duration;

use clap::{Parser, Subcommand};
use viser_encoding::clean_stale_temp_dirs;

#[derive(Parser)]
#[command(name = "viser", about = "Video Encoding Optimizer")]
struct Cli {
    /// Enable debug logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
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
    /// CRF value
    #[arg(long, default_value_t = 23)]
    crf: i32,
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
    let cli = Cli::parse();

    // Init logging
    let level: tracing::Level =
        if cli.verbose { tracing::Level::DEBUG } else { tracing::Level::WARN };
    tracing_subscriber::fmt().with_max_level(level).with_writer(std::io::stderr).init();

    // Clean stale temp dirs
    clean_stale_temp_dirs(Duration::from_secs(24 * 3600));

    match cli.command {
        Commands::Encode(args) => cmd_encode(args).await,
        Commands::Inspect { command } => match command {
            InspectCommands::Probe { file } => cmd_inspect_probe(&file).await,
        },
        Commands::Quality { command } => match command {
            QualityCommands::Measure(args) => cmd_quality_measure(args).await,
        },
        Commands::PerTitle { command } => match command {
            PerTitleCommands::Analyze(args) => cmd_pertitle_analyze(args).await,
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
        rate_control: viser_ffmpeg::RateControlMode::Crf,
        target_bitrate: 0.0,
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

fn validate_file(path: &str) -> anyhow::Result<()> {
    if !Path::new(path).exists() {
        anyhow::bail!("file not found: {path}");
    }
    Ok(())
}
