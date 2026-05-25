pub mod capture;
pub mod config;
pub mod encoder;
pub mod input;
pub mod overlay;
pub mod project;

pub use capture::{
    CaptureBackend, CaptureConfig, CaptureSelection, CaptureSource, CaptureWindowSource,
    CapturedFrame, XcapCaptureBackend, capture_origin,
};
pub use config::AppConfig;
pub use encoder::{EncodeReport, EncoderConfig, OutputFormat, OutputSize};
pub use input::{
    InputBackend, InputEvent, InputEventKind, KeyCode, MouseButton, PollingInputBackend,
};
pub use overlay::{
    LabelPosition, OverlayColor, OverlayEvent, OverlayEventKind, OverlayLabelFont, OverlaySettings,
    OverlayTimeline,
};
pub use project::{ActiveRecording, RecordingRequest, RecordingSummary, record_blocking};
