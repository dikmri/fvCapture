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
    capture_origin,
    encoder::{EncoderConfig, encode_png_sequence_range},
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

#[derive(Debug, Clone)]
pub struct RecordingProject {
    _temp: Arc<TempDir>,
    frame_dir: PathBuf,
    frame_count: usize,
    captured_frames: usize,
    duration_ms: u64,
    encoder: EncoderConfig,
    output_path: PathBuf,
}

impl RecordingProject {
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    pub fn captured_frames(&self) -> usize {
        self.captured_frames
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub fn fps(&self) -> u32 {
        self.encoder.normalized_fps()
    }

    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    pub fn frame_path(&self, frame_index: usize) -> PathBuf {
        self.frame_dir.join(format!("frame_{frame_index:06}.png"))
    }

    pub fn encode_range(
        &self,
        start_frame: usize,
        end_frame: usize,
        output_path: &Path,
    ) -> Result<RecordingSummary> {
        self.encode_range_with_config(start_frame, end_frame, &self.encoder, output_path)
    }

    pub fn encode_range_with_config(
        &self,
        start_frame: usize,
        end_frame: usize,
        encoder: &EncoderConfig,
        output_path: &Path,
    ) -> Result<RecordingSummary> {
        if self.frame_count == 0 {
            return Err(anyhow!("cannot encode an empty recording"));
        }

        let start_frame = start_frame.min(self.frame_count - 1);
        let end_frame = end_frame.min(self.frame_count - 1).max(start_frame);
        let frame_count = end_frame - start_frame + 1;
        let report = encode_png_sequence_range(
            &self.frame_dir,
            start_frame,
            frame_count,
            encoder,
            output_path,
        )?;

        Ok(RecordingSummary {
            output_path: report.output_path,
            captured_frames: self.captured_frames,
            encoded_frames: report.frame_count,
            duration_ms: self.duration_ms,
        })
    }

    pub fn encode_full(&self, output_path: &Path) -> Result<RecordingSummary> {
        self.encode_range(0, self.frame_count.saturating_sub(1), output_path)
    }
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
    handle: Option<JoinHandle<Result<RecordingProject>>>,
}

impl ActiveRecording {
    pub fn start(request: RecordingRequest) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_pause = Arc::clone(&pause);
        let handle =
            thread::spawn(move || record_to_project_blocking(request, worker_stop, worker_pause));

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

    pub fn stop(self) -> Result<RecordingSummary> {
        let project = self.stop_to_project()?;
        let output_path = project.output_path().to_path_buf();
        project.encode_full(&output_path)
    }

    pub fn stop_to_project(mut self) -> Result<RecordingProject> {
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
    let project = record_to_project_blocking(request, stop, pause)?;
    let output_path = project.output_path().to_path_buf();
    project.encode_full(&output_path)
}

pub fn record_to_project_blocking(
    request: RecordingRequest,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
) -> Result<RecordingProject> {
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

    let origin = capture_origin(&request.capture.selection).unwrap_or((0.0, 0.0));
    let encoded_frames = compose_frames(
        &frames,
        &input_events,
        origin,
        &request.overlay,
        &request.encoder,
        &composed_dir,
    )?;

    Ok(RecordingProject {
        _temp: Arc::new(temp),
        frame_dir: composed_dir,
        frame_count: encoded_frames,
        captured_frames: frames.len(),
        duration_ms: started_at.elapsed().as_millis() as u64,
        encoder: request.encoder,
        output_path: request.output_path,
    })
}

impl From<&RecordingProject> for RecordingSummary {
    fn from(project: &RecordingProject) -> Self {
        Self {
            output_path: project.output_path.clone(),
            captured_frames: project.captured_frames,
            encoded_frames: project.frame_count,
            duration_ms: project.duration_ms,
        }
    }
}

fn compose_frames(
    frames: &[FrameRecord],
    input_events: &[InputEvent],
    capture_origin: (f64, f64),
    overlay: &OverlaySettings,
    encoder: &EncoderConfig,
    output_dir: &Path,
) -> Result<usize> {
    let local_input_events = localize_input_events(input_events, capture_origin);
    let timeline = OverlayTimeline::from_input_events(&local_input_events, overlay);
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

fn localize_input_events(input_events: &[InputEvent], origin: (f64, f64)) -> Vec<InputEvent> {
    let (origin_x, origin_y) = origin;
    input_events
        .iter()
        .map(|event| {
            let kind = match event.kind {
                crate::InputEventKind::MouseMove { x, y } => crate::InputEventKind::MouseMove {
                    x: x - origin_x,
                    y: y - origin_y,
                },
                ref kind => kind.clone(),
            };
            InputEvent {
                timestamp_ms: event.timestamp_ms,
                kind,
            }
        })
        .collect()
}
