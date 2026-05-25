use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use xcap::{Monitor, Window};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CaptureSelection {
    PrimaryMonitor,
    Monitor {
        id: u32,
    },
    Window {
        id: u32,
    },
    Region {
        monitor_id: Option<u32>,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureConfig {
    pub selection: CaptureSelection,
    pub fps: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            selection: CaptureSelection::PrimaryMonitor,
            fps: 15,
        }
    }
}

impl CaptureConfig {
    pub fn normalized_fps(&self) -> u32 {
        self.fps.clamp(1, 60)
    }

    pub fn validate(&self) -> Result<()> {
        match &self.selection {
            CaptureSelection::Region { width, height, .. } if *width == 0 || *height == 0 => Err(
                anyhow!("capture region must have non-zero width and height"),
            ),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureSource {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureWindowSource {
    pub id: u32,
    pub app_name: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_minimized: bool,
    pub is_focused: bool,
}

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub timestamp_ms: u64,
    pub image: RgbaImage,
}

pub trait CaptureBackend {
    fn list_sources(&self) -> Result<Vec<CaptureSource>>;
    fn list_windows(&self) -> Result<Vec<CaptureWindowSource>>;
    fn start_capture(&mut self, config: CaptureConfig) -> Result<()>;
    fn next_frame(&mut self) -> Result<CapturedFrame>;
    fn stop_capture(&mut self) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct XcapCaptureBackend {
    active_config: Option<CaptureConfig>,
    started_at: Option<Instant>,
}

impl XcapCaptureBackend {
    pub fn capture_once(config: &CaptureConfig, started_at: Instant) -> Result<CapturedFrame> {
        config.validate()?;
        let monitor = selected_monitor(&config.selection)?;
        let image = match &config.selection {
            CaptureSelection::Window { id } => selected_window(*id)?
                .capture_image()
                .context("failed to capture window image")?,
            CaptureSelection::Region {
                x,
                y,
                width,
                height,
                ..
            } => monitor
                .capture_region(*x, *y, *width, *height)
                .context("failed to capture monitor region")?,
            CaptureSelection::PrimaryMonitor | CaptureSelection::Monitor { .. } => monitor
                .capture_image()
                .context("failed to capture monitor image")?,
        };

        Ok(CapturedFrame {
            timestamp_ms: started_at.elapsed().as_millis() as u64,
            image,
        })
    }
}

impl CaptureBackend for XcapCaptureBackend {
    fn list_sources(&self) -> Result<Vec<CaptureSource>> {
        let monitors = Monitor::all().context("failed to enumerate monitors")?;
        monitors
            .into_iter()
            .map(source_from_monitor)
            .collect::<Result<Vec<_>>>()
    }

    fn list_windows(&self) -> Result<Vec<CaptureWindowSource>> {
        let current_pid = std::process::id();
        Window::all()
            .context("failed to enumerate windows")?
            .into_iter()
            .filter_map(|window| source_from_window(window, current_pid).transpose())
            .collect()
    }

    fn start_capture(&mut self, config: CaptureConfig) -> Result<()> {
        config.validate()?;
        self.active_config = Some(config);
        self.started_at = Some(Instant::now());
        Ok(())
    }

    fn next_frame(&mut self) -> Result<CapturedFrame> {
        let config = self
            .active_config
            .as_ref()
            .ok_or_else(|| anyhow!("capture has not been started"))?;
        let started_at = self
            .started_at
            .ok_or_else(|| anyhow!("capture clock has not been started"))?;
        Self::capture_once(config, started_at)
    }

    fn stop_capture(&mut self) -> Result<()> {
        self.active_config = None;
        self.started_at = None;
        Ok(())
    }
}

fn selected_monitor(selection: &CaptureSelection) -> Result<Monitor> {
    let monitors = Monitor::all().context("failed to enumerate monitors")?;
    if monitors.is_empty() {
        return Err(anyhow!("no capture monitor found"));
    }

    match selection {
        CaptureSelection::PrimaryMonitor => monitors
            .iter()
            .find(|monitor| monitor.is_primary().unwrap_or(false))
            .cloned()
            .or_else(|| monitors.first().cloned())
            .ok_or_else(|| anyhow!("no primary monitor found")),
        CaptureSelection::Monitor { id } => monitors
            .into_iter()
            .find(|monitor| monitor.id().unwrap_or_default() == *id)
            .ok_or_else(|| anyhow!("monitor id {id} was not found")),
        CaptureSelection::Window { id } => selected_window(*id)?
            .current_monitor()
            .context("failed to read selected window monitor"),
        CaptureSelection::Region { monitor_id, .. } => {
            if let Some(monitor_id) = monitor_id {
                monitors
                    .into_iter()
                    .find(|monitor| monitor.id().unwrap_or_default() == *monitor_id)
                    .ok_or_else(|| anyhow!("monitor id {monitor_id} was not found"))
            } else {
                monitors
                    .iter()
                    .find(|monitor| monitor.is_primary().unwrap_or(false))
                    .cloned()
                    .or_else(|| monitors.first().cloned())
                    .ok_or_else(|| anyhow!("no monitor found"))
            }
        }
    }
}

fn selected_window(id: u32) -> Result<Window> {
    Window::all()
        .context("failed to enumerate windows")?
        .into_iter()
        .find(|window| window.id().unwrap_or_default() == id)
        .ok_or_else(|| anyhow!("window id {id} was not found"))
}

fn source_from_monitor(monitor: Monitor) -> Result<CaptureSource> {
    let id = monitor.id().context("failed to read monitor id")?;
    let name = monitor
        .friendly_name()
        .or_else(|_| monitor.name())
        .unwrap_or_else(|_| format!("Monitor {id}"));
    Ok(CaptureSource {
        id,
        name,
        x: monitor.x().unwrap_or_default(),
        y: monitor.y().unwrap_or_default(),
        width: monitor.width().unwrap_or_default(),
        height: monitor.height().unwrap_or_default(),
        scale_factor: monitor.scale_factor().unwrap_or(1.0),
        is_primary: monitor.is_primary().unwrap_or(false),
    })
}

fn source_from_window(window: Window, current_pid: u32) -> Result<Option<CaptureWindowSource>> {
    let id = window.id().context("failed to read window id")?;
    let pid = window.pid().unwrap_or_default();
    if pid == current_pid {
        return Ok(None);
    }

    let title = window.title().unwrap_or_default();
    let app_name = window.app_name().unwrap_or_default();
    let width = window.width().unwrap_or_default();
    let height = window.height().unwrap_or_default();
    if width == 0 || height == 0 || (title.trim().is_empty() && app_name.trim().is_empty()) {
        return Ok(None);
    }

    Ok(Some(CaptureWindowSource {
        id,
        app_name,
        title,
        x: window.x().unwrap_or_default(),
        y: window.y().unwrap_or_default(),
        width,
        height,
        is_minimized: window.is_minimized().unwrap_or(false),
        is_focused: window.is_focused().unwrap_or(false),
    }))
}

pub fn capture_origin(selection: &CaptureSelection) -> Result<(f64, f64)> {
    match selection {
        CaptureSelection::PrimaryMonitor | CaptureSelection::Monitor { .. } => {
            let monitor = selected_monitor(selection)?;
            Ok((
                monitor.x().unwrap_or_default() as f64,
                monitor.y().unwrap_or_default() as f64,
            ))
        }
        CaptureSelection::Region {
            monitor_id, x, y, ..
        } => {
            let monitor = selected_monitor(&CaptureSelection::Region {
                monitor_id: *monitor_id,
                x: *x,
                y: *y,
                width: 1,
                height: 1,
            })?;
            Ok((
                monitor.x().unwrap_or_default() as f64 + *x as f64,
                monitor.y().unwrap_or_default() as f64 + *y as f64,
            ))
        }
        CaptureSelection::Window { id } => {
            let window = selected_window(*id)?;
            Ok((
                window.x().unwrap_or_default() as f64,
                window.y().unwrap_or_default() as f64,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_is_clamped_to_reasonable_capture_range() {
        let mut config = CaptureConfig {
            fps: 0,
            ..Default::default()
        };
        assert_eq!(config.normalized_fps(), 1);
        config.fps = 120;
        assert_eq!(config.normalized_fps(), 60);
    }

    #[test]
    fn rejects_empty_region() {
        let config = CaptureConfig {
            selection: CaptureSelection::Region {
                monitor_id: None,
                x: 0,
                y: 0,
                width: 0,
                height: 50,
            },
            fps: 15,
        };

        assert!(config.validate().is_err());
    }
}
