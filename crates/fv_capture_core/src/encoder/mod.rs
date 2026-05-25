use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    #[default]
    Mp4,
    Gif,
    WebM,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Mp4 => "mp4",
            OutputFormat::Gif => "gif",
            OutputFormat::WebM => "webm",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputSize {
    #[default]
    Original,
    P720,
    P480,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncoderConfig {
    pub format: OutputFormat,
    pub fps: u32,
    pub size: OutputSize,
    pub crf: u8,
    pub trim_start_ms: u64,
    pub trim_end_ms: Option<u64>,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::Mp4,
            fps: 15,
            size: OutputSize::Original,
            crf: 23,
            trim_start_ms: 0,
            trim_end_ms: None,
        }
    }
}

impl EncoderConfig {
    pub fn normalized_fps(&self) -> u32 {
        self.fps.clamp(1, 60)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeReport {
    pub output_path: PathBuf,
    pub frame_count: usize,
}

pub fn encode_png_sequence(
    frame_dir: &Path,
    frame_count: usize,
    config: &EncoderConfig,
    output_path: &Path,
) -> Result<EncodeReport> {
    if frame_count == 0 {
        return Err(anyhow!("cannot encode an empty frame sequence"));
    }

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory: {}", parent.display()))?;
    }

    let pattern = frame_dir.join("frame_%06d.png");
    let args = build_ffmpeg_args(&pattern, config, output_path);
    tracing::info!(?args, "starting ffmpeg encode");
    let output = Command::new(ffmpeg_binary())
        .args(args.iter().map(OsString::from))
        .output()
        .context("failed to start ffmpeg. Install FFmpeg or set FVCAPTURE_FFMPEG")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg failed: {stderr}"));
    }

    tracing::info!(path = %output_path.display(), "ffmpeg encode finished");
    Ok(EncodeReport {
        output_path: output_path.to_path_buf(),
        frame_count,
    })
}

pub fn build_ffmpeg_args(
    input_pattern: &Path,
    config: &EncoderConfig,
    output_path: &Path,
) -> Vec<String> {
    let fps = config.normalized_fps().to_string();
    let mut args = vec![
        "-y".to_string(),
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-framerate".to_string(),
        fps.clone(),
        "-start_number".to_string(),
        "0".to_string(),
        "-i".to_string(),
        input_pattern.to_string_lossy().to_string(),
    ];

    match config.format {
        OutputFormat::Mp4 => {
            if let Some(filter) = video_filter(config) {
                args.extend(["-vf".to_string(), filter]);
            }
            args.extend([
                "-an".to_string(),
                "-c:v".to_string(),
                "libx264".to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-preset".to_string(),
                "fast".to_string(),
                "-crf".to_string(),
                config.crf.to_string(),
            ]);
        }
        OutputFormat::Gif => {
            args.extend([
                "-vf".to_string(),
                gif_filter(config),
                "-loop".to_string(),
                "0".to_string(),
            ]);
        }
        OutputFormat::WebM => {
            if let Some(filter) = video_filter(config) {
                args.extend(["-vf".to_string(), filter]);
            }
            args.extend([
                "-an".to_string(),
                "-c:v".to_string(),
                "libvpx-vp9".to_string(),
                "-b:v".to_string(),
                "0".to_string(),
                "-crf".to_string(),
                "32".to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
            ]);
        }
    }

    args.push(output_path.to_string_lossy().to_string());
    args
}

fn video_filter(config: &EncoderConfig) -> Option<String> {
    match config.size {
        OutputSize::Original => None,
        OutputSize::P720 => Some("scale=720:-2:flags=lanczos".to_string()),
        OutputSize::P480 => Some("scale=480:-2:flags=lanczos".to_string()),
    }
}

fn gif_filter(config: &EncoderConfig) -> String {
    let size = match config.size {
        OutputSize::Original => "scale=iw:ih:flags=lanczos".to_string(),
        OutputSize::P720 => "scale=720:-2:flags=lanczos".to_string(),
        OutputSize::P480 => "scale=480:-2:flags=lanczos".to_string(),
    };
    format!(
        "fps={},{} ,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse",
        config.normalized_fps(),
        size
    )
    .replace(" ,", ",")
}

fn ffmpeg_binary() -> OsString {
    std::env::var_os("FVCAPTURE_FFMPEG").unwrap_or_else(|| OsString::from("ffmpeg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp4_command_uses_h264_without_audio() {
        let config = EncoderConfig {
            format: OutputFormat::Mp4,
            ..Default::default()
        };
        let args = build_ffmpeg_args(
            Path::new("frames/frame_%06d.png"),
            &config,
            Path::new("out.mp4"),
        );

        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"-an".to_string()));
        assert!(args.contains(&"yuv420p".to_string()));
    }

    #[test]
    fn gif_command_uses_palette_filter() {
        let config = EncoderConfig {
            format: OutputFormat::Gif,
            fps: 10,
            size: OutputSize::P720,
            ..Default::default()
        };
        let args = build_ffmpeg_args(
            Path::new("frames/frame_%06d.png"),
            &config,
            Path::new("out.gif"),
        );

        let vf = args
            .windows(2)
            .find(|pair| pair[0] == "-vf")
            .map(|pair| pair[1].clone())
            .unwrap();
        assert!(vf.contains("palettegen"));
        assert!(vf.contains("fps=10"));
        assert!(vf.contains("scale=720"));
    }
}
