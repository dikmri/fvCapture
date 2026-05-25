use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use image::ImageReader;
use tempfile::TempDir;

use crate::{
    CaptureConfig,
    capture::XcapCaptureBackend,
    encoder::{EncoderConfig, encode_png_sequence},
    input::{InputBackend, InputEvent, PollingInputBackend},
    overlay::{OverlaySettings, OverlayTimeline, composite_frame},
};

#[derive(Debug, Clone)]
pub struct RecordingRequest {
    pub capture: CaptureConfig,
    pub overlay: OverlaySettings,
    pub encoder: EncoderConfig,
    pub output_path: PathBuf,
    pub max_duration: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingSummary {
    pub output_path: PathBuf,
    pub captured_frames: usize,
    pub encoded_frames: usize,
    pub duration_ms: u64,
}

#[derive(Debug)]
struct FrameRecord {
    path: PathBuf,
    timestamp_ms: u64,
}

#[derive(Debug)]
pub struct ActiveRecording {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<RecordingSummary>>>,
}

impl ActiveRecording {
    pub fn start(request: RecordingRequest) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_pause = Arc::clone(&pause);
        let handle = thread::spawn(move || record_blocking(request, worker_stop, worker_pause));

        Self {
            stop,
            pause,
            handle: Some(handle),
        }
    }

    pub fn pause(&self) {
        self.pause.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.pause.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.pause.load(Ordering::SeqCst)
    }

    pub fn stop(mut self) -> Result<RecordingSummary> {
        self.stop.store(true, Ordering::SeqCst);
        let handle = self
            .handle
            .take()
            .ok_or_else(|| anyhow!("recording worker has already been joined"))?;
        handle
            .join()
            .map_err(|_| anyhow!("recording worker panicked"))?
    }
}

impl Drop for ActiveRecording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn record_blocking(
    request: RecordingRequest,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
) -> Result<RecordingSummary> {
    request.capture.validate()?;

    let temp = TempDir::new().context("failed to create temporary recording directory")?;
    let raw_dir = temp.path().join("raw");
    let composed_dir = temp.path().join("composed");
    std::fs::create_dir_all(&raw_dir).context("failed to create raw frame directory")?;
    std::fs::create_dir_all(&composed_dir).context("failed to create composed frame directory")?;

    let (input_tx, input_rx) = mpsc::channel::<InputEvent>();
    let mut input_backend = PollingInputBackend::default();
    input_backend
        .start_listening(input_tx)
        .context("failed to start input polling")?;

    tracing::info!("recording started");
    let started_at = Instant::now();
    let frame_interval = Duration::from_secs_f64(1.0 / request.capture.normalized_fps() as f64);
    let mut next_frame_at = Instant::now();
    let mut frames = Vec::new();
    let mut frame_index = 0usize;

    while !stop.load(Ordering::SeqCst) {
        if let Some(max_duration) = request.max_duration
            && started_at.elapsed() >= max_duration
        {
            break;
        }

        if pause.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(30));
            next_frame_at = Instant::now();
            continue;
        }

        let now = Instant::now();
        if now < next_frame_at {
            thread::sleep(next_frame_at - now);
        }

        let frame = XcapCaptureBackend::capture_once(&request.capture, started_at)?;
        let frame_path = raw_dir.join(format!("raw_{frame_index:06}.png"));
        frame
            .image
            .save(&frame_path)
            .with_context(|| format!("failed to save raw frame: {}", frame_path.display()))?;
        frames.push(FrameRecord {
            path: frame_path,
            timestamp_ms: frame.timestamp_ms,
        });
        frame_index += 1;
        next_frame_at += frame_interval;
    }

    input_backend
        .stop_listening()
        .context("failed to stop input polling")?;
    tracing::info!("recording stopped");

    if frames.is_empty() {
        return Err(anyhow!("recording did not capture any frames"));
    }

    let mut input_events = Vec::new();
    while let Ok(event) = input_rx.try_recv() {
        input_events.push(event);
    }

    let encoded_frames = compose_frames(
        &frames,
        &input_events,
        &request.overlay,
        &request.encoder,
        &composed_dir,
    )?;

    let report = encode_png_sequence(
        &composed_dir,
        encoded_frames,
        &request.encoder,
        &request.output_path,
    )?;

    Ok(RecordingSummary {
        output_path: report.output_path,
        captured_frames: frames.len(),
        encoded_frames: report.frame_count,
        duration_ms: started_at.elapsed().as_millis() as u64,
    })
}

fn compose_frames(
    frames: &[FrameRecord],
    input_events: &[InputEvent],
    overlay: &OverlaySettings,
    encoder: &EncoderConfig,
    output_dir: &Path,
) -> Result<usize> {
    let timeline = OverlayTimeline::from_input_events(input_events, overlay);
    let trim_start = encoder.trim_start_ms;
    let trim_end = encoder.trim_end_ms.unwrap_or(u64::MAX);
    let mut output_index = 0usize;

    for frame in frames {
        if frame.timestamp_ms < trim_start || frame.timestamp_ms > trim_end {
            continue;
        }

        let mut image = ImageReader::open(&frame.path)
            .with_context(|| format!("failed to open raw frame: {}", frame.path.display()))?
            .decode()
            .with_context(|| format!("failed to decode raw frame: {}", frame.path.display()))?
            .to_rgba8();

        composite_frame(&mut image, &timeline, frame.timestamp_ms, overlay);

        let output_path = output_dir.join(format!("frame_{output_index:06}.png"));
        image
            .save(&output_path)
            .with_context(|| format!("failed to save composed frame: {}", output_path.display()))?;
        output_index += 1;
    }

    if output_index == 0 {
        return Err(anyhow!("trim settings removed all frames"));
    }

    Ok(output_index)
}
