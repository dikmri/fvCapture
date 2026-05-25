use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, ValueEnum};
use fv_capture_core::{
    CaptureBackend, CaptureConfig, CaptureSelection, EncoderConfig, OutputFormat, OutputSize,
    OverlaySettings, RecordingRequest, XcapCaptureBackend, record_blocking,
};

#[derive(Debug, Parser)]
#[command(name = "fv-capture")]
#[command(about = "Record a short fvCapture screen capture from the command line.")]
struct Cli {
    #[arg(long)]
    list_sources: bool,

    #[arg(long, default_value_t = 3.0)]
    duration: f64,

    #[arg(long, default_value_t = 15)]
    fps: u32,

    #[arg(long, value_enum, default_value_t = CliFormat::Mp4)]
    format: CliFormat,

    #[arg(long, value_enum, default_value_t = CliSize::Original)]
    size: CliSize,

    #[arg(long)]
    output: Option<PathBuf>,

    #[arg(long)]
    monitor_id: Option<u32>,

    #[arg(long, value_parser = parse_region)]
    region: Option<(u32, u32, u32, u32)>,

    #[arg(long)]
    no_keyboard: bool,

    #[arg(long)]
    no_mouse: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliFormat {
    Mp4,
    Gif,
    Webm,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliSize {
    Original,
    P720,
    P480,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "fv_capture_core=info".to_string()),
        )
        .init();

    let cli = Cli::parse();
    if cli.list_sources {
        list_sources()?;
        return Ok(());
    }

    let format = OutputFormat::from(cli.format);
    let output_path = cli.output.unwrap_or_else(|| default_output_path(format));
    let selection = match (cli.region, cli.monitor_id) {
        (Some((x, y, width, height)), monitor_id) => CaptureSelection::Region {
            monitor_id,
            x,
            y,
            width,
            height,
        },
        (None, Some(id)) => CaptureSelection::Monitor { id },
        (None, None) => CaptureSelection::PrimaryMonitor,
    };

    let request = RecordingRequest {
        capture: CaptureConfig {
            selection,
            fps: cli.fps,
        },
        overlay: OverlaySettings {
            show_keyboard: !cli.no_keyboard,
            show_mouse: !cli.no_mouse,
            ..Default::default()
        },
        encoder: EncoderConfig {
            format,
            fps: cli.fps,
            size: OutputSize::from(cli.size),
            ..Default::default()
        },
        output_path,
        max_duration: Some(Duration::from_secs_f64(cli.duration.max(0.1))),
    };

    let summary = record_blocking(
        request,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    )?;

    println!(
        "Saved {} ({} encoded frames, {} ms)",
        summary.output_path.display(),
        summary.encoded_frames,
        summary.duration_ms
    );
    Ok(())
}

fn list_sources() -> Result<()> {
    let backend = XcapCaptureBackend::default();
    let sources = backend.list_sources()?;
    for source in sources {
        println!(
            "{}\t{}\t{}x{}\t({}, {})\tprimary={}",
            source.id,
            source.name,
            source.width,
            source.height,
            source.x,
            source.y,
            source.is_primary
        );
    }
    Ok(())
}

fn parse_region(value: &str) -> Result<(u32, u32, u32, u32)> {
    let parts: Vec<_> = value.split(',').collect();
    if parts.len() != 4 {
        return Err(anyhow!("region must be x,y,width,height"));
    }

    Ok((
        parts[0].parse().context("invalid region x")?,
        parts[1].parse().context("invalid region y")?,
        parts[2].parse().context("invalid region width")?,
        parts[3].parse().context("invalid region height")?,
    ))
}

fn default_output_path(format: OutputFormat) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    PathBuf::from(format!("fvCapture-{stamp}.{}", format.extension()))
}

impl From<CliFormat> for OutputFormat {
    fn from(value: CliFormat) -> Self {
        match value {
            CliFormat::Mp4 => OutputFormat::Mp4,
            CliFormat::Gif => OutputFormat::Gif,
            CliFormat::Webm => OutputFormat::WebM,
        }
    }
}

impl From<CliSize> for OutputSize {
    fn from(value: CliSize) -> Self {
        match value {
            CliSize::Original => OutputSize::Original,
            CliSize::P720 => OutputSize::P720,
            CliSize::P480 => OutputSize::P480,
        }
    }
}
