mod key_normalizer;

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use device_query::{DeviceQuery, DeviceState, Keycode};
use serde::{Deserialize, Serialize};

pub use key_normalizer::{keycode_to_key_code, normalized_combo};

pub type InputEventSender = Sender<InputEvent>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyCode {
    Control,
    Shift,
    Alt,
    Meta,
    Character(char),
    Number(u8),
    Function(u8),
    Enter,
    Space,
    Escape,
    Tab,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Unknown(String),
}

impl KeyCode {
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            KeyCode::Control | KeyCode::Shift | KeyCode::Alt | KeyCode::Meta
        )
    }

    pub fn label(&self) -> String {
        match self {
            KeyCode::Control => "Ctrl".to_string(),
            KeyCode::Shift => "Shift".to_string(),
            KeyCode::Alt => "Alt".to_string(),
            KeyCode::Meta => "Meta".to_string(),
            KeyCode::Character(ch) => ch.to_ascii_uppercase().to_string(),
            KeyCode::Number(n) => n.to_string(),
            KeyCode::Function(n) => format!("F{n}"),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Space => "Space".to_string(),
            KeyCode::Escape => "Esc".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Del".to_string(),
            KeyCode::ArrowUp => "Up".to_string(),
            KeyCode::ArrowDown => "Down".to_string(),
            KeyCode::ArrowLeft => "Left".to_string(),
            KeyCode::ArrowRight => "Right".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PgUp".to_string(),
            KeyCode::PageDown => "PgDn".to_string(),
            KeyCode::Insert => "Ins".to_string(),
            KeyCode::Unknown(name) => name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

impl MouseButton {
    pub fn from_device_index(index: usize) -> Self {
        match index {
            1 => MouseButton::Left,
            2 => MouseButton::Right,
            3 => MouseButton::Middle,
            other => MouseButton::Other(other as u8),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputEventKind {
    KeyDown(KeyCode),
    KeyUp(KeyCode),
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    MouseMove { x: f64, y: f64 },
    MouseWheel { delta_x: f64, delta_y: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputEvent {
    pub timestamp_ms: u64,
    pub kind: InputEventKind,
}

pub trait InputBackend {
    fn start_listening(&mut self, sender: InputEventSender) -> Result<()>;
    fn stop_listening(&mut self) -> Result<()>;
}

#[derive(Debug)]
pub struct PollingInputBackend {
    interval: Duration,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Default for PollingInputBackend {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(12),
            stop: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }
}

impl PollingInputBackend {
    pub fn with_interval(interval: Duration) -> Self {
        Self {
            interval,
            ..Default::default()
        }
    }
}

impl InputBackend for PollingInputBackend {
    fn start_listening(&mut self, sender: InputEventSender) -> Result<()> {
        if self.handle.is_some() {
            return Err(anyhow!("input listener is already running"));
        }

        self.stop.store(false, Ordering::SeqCst);
        let stop = Arc::clone(&self.stop);
        let interval = self.interval;
        self.handle = Some(thread::spawn(move || {
            let device_state = DeviceState::new();
            let started_at = Instant::now();
            let mut previous_keys: HashSet<Keycode> = HashSet::new();
            let mut previous_buttons: Vec<bool> = Vec::new();
            let mut previous_coords: Option<(i32, i32)> = None;

            while !stop.load(Ordering::SeqCst) {
                let timestamp_ms = started_at.elapsed().as_millis() as u64;

                let current_keys: HashSet<Keycode> = device_state.get_keys().into_iter().collect();
                for key in current_keys.difference(&previous_keys) {
                    if let Some(code) = keycode_to_key_code(*key) {
                        let _ = sender.send(InputEvent {
                            timestamp_ms,
                            kind: InputEventKind::KeyDown(code),
                        });
                    }
                }
                for key in previous_keys.difference(&current_keys) {
                    if let Some(code) = keycode_to_key_code(*key) {
                        let _ = sender.send(InputEvent {
                            timestamp_ms,
                            kind: InputEventKind::KeyUp(code),
                        });
                    }
                }
                previous_keys = current_keys;

                let mouse = device_state.get_mouse();
                if previous_coords != Some(mouse.coords) {
                    let _ = sender.send(InputEvent {
                        timestamp_ms,
                        kind: InputEventKind::MouseMove {
                            x: mouse.coords.0 as f64,
                            y: mouse.coords.1 as f64,
                        },
                    });
                    previous_coords = Some(mouse.coords);
                }

                let max_len = previous_buttons.len().max(mouse.button_pressed.len());
                for index in 1..max_len {
                    let was_pressed = previous_buttons.get(index).copied().unwrap_or(false);
                    let is_pressed = mouse.button_pressed.get(index).copied().unwrap_or(false);
                    if was_pressed != is_pressed {
                        let kind = if is_pressed {
                            InputEventKind::MouseDown(MouseButton::from_device_index(index))
                        } else {
                            InputEventKind::MouseUp(MouseButton::from_device_index(index))
                        };
                        let _ = sender.send(InputEvent { timestamp_ms, kind });
                    }
                }
                previous_buttons = mouse.button_pressed;

                thread::sleep(interval);
            }
        }));

        Ok(())
    }

    fn stop_listening(&mut self) -> Result<()> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| anyhow!("input listener thread panicked"))?;
        }
        Ok(())
    }
}
