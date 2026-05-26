#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use fv_capture_core::{
    ActiveRecording, AppConfig, CaptureBackend, CaptureConfig, CaptureSelection, CaptureSource,
    CaptureWindowSource, KeyCode, LabelPosition, MouseButton, OutputFormat, OutputSize,
    OverlayColor, OverlayEvent, OverlayEventKind, OverlayLabelFont, OverlaySettings,
    OverlayTimeline, Point, RecordingProject, RecordingRequest, RecordingSummary,
    XcapCaptureBackend, capture_origin, composite_frame,
};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey},
};
use image::{ImageReader, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

mod auto_update;
mod i18n;
mod ui_fonts;

use i18n::{LanguageChoice, Text, Tr};

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "fv_capture_core=info".to_string()),
        )
        .init();

    let mut viewport = egui::ViewportBuilder::default()
        .with_position([32.0, 32.0])
        .with_inner_size([780.0, 620.0])
        .with_min_inner_size([680.0, 520.0]);
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "fvCapture",
        options,
        Box::new(|cc| {
            let _ = ui_fonts::install(&cc.egui_ctx, &ui_fonts::UiFontConfig::default());
            Ok(Box::new(FvCaptureApp::new(cc.egui_ctx.clone())))
        }),
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum SourceMode {
    #[default]
    Primary,
    Monitor,
    Window,
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    Capture,
    Overlay,
    Appearance,
}

struct PreviewImage {
    image: egui::ColorImage,
    texture: Option<egui::TextureHandle>,
    origin: (i32, i32),
    source_size: (u32, u32),
}

impl PreviewImage {
    fn new(image: RgbaImage, origin: (i32, i32)) -> Self {
        let source_size = (image.width(), image.height());
        Self {
            image: rgba_image_to_color_image(&image),
            texture: None,
            origin,
            source_size,
        }
    }

    fn texture_id(&mut self, ctx: &egui::Context, name: &str) -> egui::TextureId {
        self.texture
            .get_or_insert_with(|| {
                ctx.load_texture(name, self.image.clone(), egui::TextureOptions::LINEAR)
            })
            .id()
    }
}

struct RegionSelectorState {
    preview: PreviewImage,
    drag_start: Option<(u32, u32)>,
    selection: Option<(u32, u32, u32, u32)>,
}

struct ScreenOverlayPreviewState {
    preview: PreviewImage,
}

struct RecordingPreviewState {
    project: RecordingProject,
    current_frame: usize,
    trim_start_frame: usize,
    trim_end_frame: usize,
    playing: bool,
    loop_playback: bool,
    last_advance: Instant,
    frame_image: Option<PreviewImage>,
    frame_image_index: Option<usize>,
}

impl RecordingPreviewState {
    fn new(project: RecordingProject) -> Self {
        let trim_end_frame = project.frame_count().saturating_sub(1);
        Self {
            project,
            current_frame: 0,
            trim_start_frame: 0,
            trim_end_frame,
            playing: false,
            loop_playback: false,
            last_advance: Instant::now(),
            frame_image: None,
            frame_image_index: None,
        }
    }

    fn selected_frame_count(&self) -> usize {
        self.trim_end_frame
            .saturating_sub(self.trim_start_frame)
            .saturating_add(1)
    }

    fn clamp_to_trim(&mut self) {
        if self.project.frame_count() == 0 {
            self.current_frame = 0;
            self.trim_start_frame = 0;
            self.trim_end_frame = 0;
            return;
        }

        let last_frame = self.project.frame_count() - 1;
        self.trim_start_frame = self.trim_start_frame.min(last_frame);
        self.trim_end_frame = self
            .trim_end_frame
            .min(last_frame)
            .max(self.trim_start_frame);
        self.current_frame = self
            .current_frame
            .clamp(self.trim_start_frame, self.trim_end_frame);
    }

    fn update_playback(&mut self, ctx: &egui::Context) {
        if !self.playing {
            return;
        }

        self.clamp_to_trim();
        let frame_duration = Duration::from_secs_f64(1.0 / self.project.fps().max(1) as f64);
        let elapsed = self.last_advance.elapsed();
        if elapsed < frame_duration {
            ctx.request_repaint_after(frame_duration - elapsed);
            return;
        }

        let steps = (elapsed.as_secs_f64() / frame_duration.as_secs_f64())
            .floor()
            .max(1.0) as usize;
        self.last_advance = Instant::now();
        let next_frame = self.current_frame.saturating_add(steps);
        if next_frame <= self.trim_end_frame {
            self.current_frame = next_frame;
        } else if self.loop_playback {
            self.current_frame = self.trim_start_frame;
        } else {
            self.current_frame = self.trim_end_frame;
            self.playing = false;
        }
        ctx.request_repaint_after(frame_duration);
    }
}

struct GlobalShortcutState {
    _manager: GlobalHotKeyManager,
    action_rx: Receiver<ShortcutAction>,
}

impl GlobalShortcutState {
    fn register(ctx: egui::Context) -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new().map_err(|error| error.to_string())?;
        let start_stop = HotKey::new(None, Code::F9);
        let pause_resume = HotKey::new(None, Code::F10);
        let start_stop_id = start_stop.id();
        let pause_resume_id = pause_resume.id();
        let (action_tx, action_rx) = mpsc::channel();
        manager
            .register_all(&[start_stop, pause_resume])
            .map_err(|error| error.to_string())?;
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed {
                return;
            }
            let action = if event.id == start_stop_id {
                Some(ShortcutAction::StartStop)
            } else if event.id == pause_resume_id {
                Some(ShortcutAction::PauseResume)
            } else {
                None
            };
            if let Some(action) = action {
                let _ = action_tx.send(action);
                ctx.request_repaint();
            }
        }));
        Ok(Self {
            _manager: manager,
            action_rx,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    StartStop,
    PauseResume,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct SavedGuiSettings {
    config: AppConfig,
    source_mode: SourceMode,
    selected_monitor_id: Option<u32>,
    selected_window_id: Option<u32>,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    output_path: String,
    language: LanguageChoice,
}

impl Default for SavedGuiSettings {
    fn default() -> Self {
        Self {
            config: AppConfig::default(),
            source_mode: SourceMode::Primary,
            selected_monitor_id: None,
            selected_window_id: None,
            region_x: 0,
            region_y: 0,
            region_width: 1280,
            region_height: 720,
            output_path: default_output_path(OutputFormat::Mp4).display().to_string(),
            language: LanguageChoice::System,
        }
    }
}

struct FvCaptureApp {
    config: AppConfig,
    active_tab: AppTab,
    sources: Vec<CaptureSource>,
    window_sources: Vec<CaptureWindowSource>,
    source_mode: SourceMode,
    selected_monitor_id: Option<u32>,
    selected_window_id: Option<u32>,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    output_path: String,
    active: Option<ActiveRecording>,
    project_rx: Option<Receiver<Result<RecordingProject, String>>>,
    encoding_rx: Option<Receiver<(RecordingProject, Result<RecordingSummary, String>)>>,
    encoding_started_at: Option<Instant>,
    global_shortcuts: Option<GlobalShortcutState>,
    global_shortcut_error: Option<String>,
    update_rx: Option<Receiver<Result<Option<auto_update::UpdateInfo>, String>>>,
    update_available: Option<auto_update::UpdateInfo>,
    update_error: Option<String>,
    update_installing: bool,
    window_preview_for: Option<u32>,
    window_preview: Option<PreviewImage>,
    window_preview_error: Option<String>,
    region_selector: Option<RegionSelectorState>,
    region_preview: Option<PreviewImage>,
    screen_overlay_preview: Option<ScreenOverlayPreviewState>,
    recording_preview: Option<RecordingPreviewState>,
    last_summary: Option<RecordingSummary>,
    status: StatusKey,
    error: Option<String>,
    language: LanguageChoice,
    settings_path: Option<PathBuf>,
    last_saved_settings: SavedGuiSettings,
    settings_error: Option<String>,
    last_icon_status: Option<StatusKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKey {
    Ready,
    Recording,
    Paused,
    Encoding,
    PreviewReady,
    Saved,
}

impl FvCaptureApp {
    fn new(ctx: egui::Context) -> Self {
        let (global_shortcuts, global_shortcut_error) = match GlobalShortcutState::register(ctx) {
            Ok(shortcuts) => (Some(shortcuts), None),
            Err(error) => (None, Some(error)),
        };
        let settings_path = settings_path();
        let (saved_settings, settings_error) = load_saved_settings(settings_path.as_deref());
        let output_path = if saved_settings.output_path.trim().is_empty() {
            default_output_path(saved_settings.config.encoder.format)
                .display()
                .to_string()
        } else {
            saved_settings.output_path.clone()
        };
        let mut app = Self {
            config: saved_settings.config.clone(),
            active_tab: AppTab::Capture,
            sources: Vec::new(),
            window_sources: Vec::new(),
            source_mode: saved_settings.source_mode,
            selected_monitor_id: saved_settings.selected_monitor_id,
            selected_window_id: saved_settings.selected_window_id,
            region_x: saved_settings.region_x,
            region_y: saved_settings.region_y,
            region_width: saved_settings.region_width,
            region_height: saved_settings.region_height,
            output_path,
            active: None,
            project_rx: None,
            encoding_rx: None,
            encoding_started_at: None,
            global_shortcuts,
            global_shortcut_error,
            update_rx: Some(auto_update::spawn_update_check()),
            update_available: None,
            update_error: None,
            update_installing: false,
            window_preview_for: None,
            window_preview: None,
            window_preview_error: None,
            region_selector: None,
            region_preview: None,
            screen_overlay_preview: None,
            recording_preview: None,
            last_summary: None,
            status: StatusKey::Ready,
            error: None,
            language: saved_settings.language,
            settings_path,
            last_saved_settings: saved_settings,
            settings_error,
            last_icon_status: None,
        };
        app.refresh_sources();
        app.last_saved_settings = app.saved_settings_snapshot();
        app
    }

    fn refresh_sources(&mut self) {
        let backend = XcapCaptureBackend::default();
        match backend.list_sources() {
            Ok(sources) => {
                if self
                    .selected_monitor_id
                    .is_none_or(|id| !sources.iter().any(|source| source.id == id))
                {
                    self.selected_monitor_id = sources
                        .iter()
                        .find(|source| source.is_primary)
                        .or_else(|| sources.first())
                        .map(|source| source.id);
                }
                if let Some(source) = sources
                    .iter()
                    .find(|source| Some(source.id) == self.selected_monitor_id)
                {
                    self.region_width = source.width.min(self.region_width.max(1));
                    self.region_height = source.height.min(self.region_height.max(1));
                }
                self.sources = sources;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.to_string());
            }
        }

        match backend.list_windows() {
            Ok(windows) => {
                if self
                    .selected_window_id
                    .is_none_or(|id| !windows.iter().any(|window| window.id == id))
                {
                    self.selected_window_id = windows.first().map(|window| window.id);
                }
                self.window_sources = windows;
            }
            Err(error) => {
                self.window_sources.clear();
                if self.source_mode == SourceMode::Window {
                    self.error = Some(error.to_string());
                }
            }
        }
    }

    fn current_selection(&self) -> Result<CaptureSelection, String> {
        match self.source_mode {
            SourceMode::Primary => Ok(CaptureSelection::PrimaryMonitor),
            SourceMode::Monitor => Ok(self
                .selected_monitor_id
                .map(|id| CaptureSelection::Monitor { id })
                .unwrap_or(CaptureSelection::PrimaryMonitor)),
            SourceMode::Window => {
                let Some(id) = self.selected_window_id else {
                    return Err(self.tr(Text::NoWindowSelected).to_string());
                };
                Ok(CaptureSelection::Window { id })
            }
            SourceMode::Region => Ok(CaptureSelection::Region {
                monitor_id: self.selected_monitor_id,
                x: self.region_x,
                y: self.region_y,
                width: self.region_width.max(1),
                height: self.region_height.max(1),
            }),
        }
    }

    fn saved_settings_snapshot(&self) -> SavedGuiSettings {
        let mut settings = SavedGuiSettings {
            config: self.config.clone(),
            source_mode: self.source_mode,
            selected_monitor_id: self.selected_monitor_id,
            selected_window_id: self.selected_window_id,
            region_x: self.region_x,
            region_y: self.region_y,
            region_width: self.region_width,
            region_height: self.region_height,
            output_path: self.output_path.clone(),
            language: self.language,
        };
        if let Ok(selection) = self.current_selection() {
            settings.config.capture.selection = selection;
        }
        settings
    }

    fn save_settings_if_changed(&mut self) {
        let settings = self.saved_settings_snapshot();
        if settings == self.last_saved_settings {
            return;
        }

        let Some(path) = &self.settings_path else {
            self.last_saved_settings = settings;
            return;
        };

        match save_saved_settings(path, &settings) {
            Ok(()) => {
                self.last_saved_settings = settings;
                self.settings_error = None;
            }
            Err(error) => {
                self.settings_error = Some(error);
            }
        }
    }

    fn update_window_icon(&mut self, ctx: &egui::Context) {
        if self.last_icon_status == Some(self.status) {
            return;
        }
        if let Some(icon) = load_status_icon(self.status) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon))));
            self.last_icon_status = Some(self.status);
        }
    }

    fn start_recording(&mut self) {
        if self.active.is_some() || self.project_rx.is_some() || self.encoding_rx.is_some() {
            return;
        }

        let output_path = PathBuf::from(self.output_path.trim());
        let selection = match self.current_selection() {
            Ok(selection) => selection,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };

        self.config.capture.selection = selection;
        self.config.encoder.fps = self.config.capture.fps;
        let request = RecordingRequest {
            capture: self.config.capture.clone(),
            overlay: self.config.overlay.clone(),
            encoder: self.config.encoder.clone(),
            output_path,
            max_duration: None,
        };

        self.active = Some(ActiveRecording::start(request));
        self.recording_preview = None;
        self.last_summary = None;
        self.status = StatusKey::Recording;
        self.error = None;
    }

    fn stop_recording(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        self.status = StatusKey::Encoding;
        self.encoding_started_at = Some(Instant::now());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = active.stop_to_project().map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
        self.project_rx = Some(rx);
    }

    fn update_output_extension(&mut self) {
        let path = PathBuf::from(self.output_path.trim());
        let extension = self.config.encoder.format.extension();
        let updated = path.with_extension(extension);
        self.output_path = updated.display().to_string();
    }

    fn poll_project(&mut self) {
        let result = self.project_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some(result) = result else {
            return;
        };

        self.project_rx = None;
        self.encoding_started_at = None;
        match result {
            Ok(project) => {
                self.status = StatusKey::PreviewReady;
                self.recording_preview = Some(RecordingPreviewState::new(project));
                self.last_summary = None;
                self.error = None;
            }
            Err(error) => {
                self.status = StatusKey::Ready;
                self.error = Some(error);
            }
        }
    }

    fn poll_encoding(&mut self) {
        let result = self.encoding_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some((project, result)) = result else {
            return;
        };

        self.encoding_rx = None;
        self.encoding_started_at = None;
        match result {
            Ok(summary) => {
                self.status = StatusKey::Saved;
                self.last_summary = Some(summary);
                self.error = None;
            }
            Err(error) => {
                self.status = StatusKey::PreviewReady;
                self.recording_preview = Some(RecordingPreviewState::new(project));
                self.error = Some(error);
            }
        }
    }

    fn poll_update_check(&mut self) {
        let result = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some(result) = result else {
            return;
        };

        self.update_rx = None;
        match result {
            Ok(Some(info)) => {
                self.update_available = Some(info);
                self.update_error = None;
            }
            Ok(None) => {
                self.update_error = None;
            }
            Err(error) => {
                self.update_error = Some(error);
            }
        }
    }

    fn poll_global_shortcuts(&mut self) {
        let Some(shortcuts) = &self.global_shortcuts else {
            return;
        };
        let mut actions = Vec::new();
        while let Ok(action) = shortcuts.action_rx.try_recv() {
            actions.push(action);
        }
        for action in actions {
            let handled = self.handle_shortcut_action(action);
            if handled && self.config.shortcut_feedback_sound {
                play_feedback_sound();
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.global_shortcuts.is_some() {
            return;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::F9)) {
            self.handle_shortcut_action(ShortcutAction::StartStop);
        }

        if ctx.input(|input| input.key_pressed(egui::Key::F10)) {
            self.handle_shortcut_action(ShortcutAction::PauseResume);
        }
    }

    fn handle_shortcut_action(&mut self, action: ShortcutAction) -> bool {
        match action {
            ShortcutAction::StartStop => {
                if self.active.is_some() {
                    self.stop_recording();
                    true
                } else if self.project_rx.is_none() && self.encoding_rx.is_none() {
                    self.start_recording();
                    self.active.is_some()
                } else {
                    false
                }
            }
            ShortcutAction::PauseResume => {
                if let Some(active) = &self.active {
                    if active.is_paused() {
                        active.resume();
                        self.status = StatusKey::Recording;
                    } else {
                        active.pause();
                        self.status = StatusKey::Paused;
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        self.poll_project();
        self.poll_encoding();
        self.poll_update_check();
        self.poll_global_shortcuts();
        self.sync_runtime_state(ctx);
    }

    fn sync_runtime_state(&mut self, ctx: &egui::Context) {
        if self.active.is_some()
            || self.project_rx.is_some()
            || self.encoding_rx.is_some()
            || self.update_rx.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.update_window_icon(ctx);
        self.save_settings_if_changed();
    }
}

impl eframe::App for FvCaptureApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);

        egui::Frame::default()
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.content_ui(ui);
                });
            });
        self.update_dialog_ui(&ctx);
        self.region_selector_viewport(&ctx);
        self.screen_overlay_preview_viewport(&ctx);
        self.sync_runtime_state(&ctx);
    }
}

impl FvCaptureApp {
    fn content_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("fvCapture");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.language_ui(ui);
            });
        });
        ui.label(self.tr(Text::Intro));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(format!(
                "{}: {}",
                self.tr(Text::Status),
                self.status_label(self.status)
            ));
            if self.active.is_some() {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        });
        if self.project_rx.is_some() {
            self.progress_ui(ui, self.tr(Text::PreparingPreview));
        }
        if self.encoding_rx.is_some() {
            self.progress_ui(ui, self.tr(Text::EncodingProgress));
        }
        if self.update_rx.is_some() {
            ui.label(self.tr(Text::UpdateChecking));
        }
        if self.update_installing {
            ui.label(self.tr(Text::UpdateInstalling));
        }

        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
        }
        if let Some(error) = &self.global_shortcut_error {
            ui.colored_label(
                egui::Color32::from_rgb(220, 160, 70),
                format!("{}: {error}", self.tr(Text::GlobalShortcutUnavailable)),
            );
        }
        if let Some(error) = &self.update_error {
            ui.colored_label(
                egui::Color32::from_rgb(220, 80, 80),
                format!("{}: {error}", self.tr(Text::UpdateCheckFailed)),
            );
        }
        if let Some(error) = &self.settings_error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
        }
        if let Some(summary) = &self.last_summary {
            ui.label(format!(
                "{}: {} ({} {})",
                self.tr(Text::Saved),
                summary.output_path.display(),
                summary.encoded_frames,
                self.tr(Text::Frames)
            ));
        }

        ui.add_space(8.0);
        self.tab_bar_ui(ui);
        ui.separator();
        match self.active_tab {
            AppTab::Capture => self.source_ui(ui),
            AppTab::Overlay => self.overlay_ui(ui),
            AppTab::Appearance => self.app_appearance_ui(ui),
        }
        ui.add_space(8.0);
        ui.separator();
        self.action_ui(ui);
        self.recording_preview_ui(ui);
    }
}

impl FvCaptureApp {
    fn tab_bar_ui(&mut self, ui: &mut egui::Ui) {
        let capture = self.tr(Text::CaptureTab);
        let overlay = self.tr(Text::OverlayTab);
        let appearance = self.tr(Text::AppearanceTab);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, AppTab::Capture, capture);
            ui.selectable_value(&mut self.active_tab, AppTab::Overlay, overlay);
            ui.selectable_value(&mut self.active_tab, AppTab::Appearance, appearance);
        });
    }

    fn source_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr(Text::CaptureSource));
        let full_screen = self.tr(Text::FullScreen);
        let monitor = self.tr(Text::Monitor);
        let window = self.tr(Text::Window);
        let select_area = self.tr(Text::SelectArea);
        let refresh = self.tr(Text::Refresh);
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.source_mode, SourceMode::Primary, full_screen);
            ui.radio_value(&mut self.source_mode, SourceMode::Monitor, monitor);
            ui.radio_value(&mut self.source_mode, SourceMode::Window, window);
            ui.radio_value(&mut self.source_mode, SourceMode::Region, select_area);
            if ui.button(refresh).clicked() {
                self.refresh_sources();
            }
        });
        ui.horizontal(|ui| {
            ui.label(self.tr(Text::Fps));
            ui.add(
                egui::DragValue::new(&mut self.config.capture.fps)
                    .range(1..=60)
                    .speed(1),
            );
        });

        if matches!(self.source_mode, SourceMode::Monitor | SourceMode::Region) {
            egui::ComboBox::from_label(monitor)
                .selected_text(self.selected_monitor_label())
                .show_ui(ui, |ui| {
                    for source in &self.sources {
                        ui.selectable_value(
                            &mut self.selected_monitor_id,
                            Some(source.id),
                            format!(
                                "{} - {}x{}{}",
                                source.name,
                                source.width,
                                source.height,
                                if source.is_primary { " (primary)" } else { "" }
                            ),
                        );
                    }
                });
        }

        if self.source_mode == SourceMode::Window {
            let mut changed_window = false;
            egui::ComboBox::from_label(window)
                .selected_text(self.selected_window_label())
                .show_ui(ui, |ui| {
                    for window in &self.window_sources {
                        changed_window |= ui
                            .selectable_value(
                                &mut self.selected_window_id,
                                Some(window.id),
                                window_label(window),
                            )
                            .changed();
                    }
                });
            if changed_window {
                self.window_preview_for = None;
            }
            self.window_preview_ui(ui);
        }

        if self.source_mode == SourceMode::Region {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if ui.button(self.tr(Text::SelectOnScreen)).clicked() {
                        self.open_region_selector();
                    }
                    ui.label(format!(
                        "{}: {}, {}  {}: {}  {}: {}",
                        self.tr(Text::Position),
                        self.region_x,
                        self.region_y,
                        self.tr(Text::Width),
                        self.region_width,
                        self.tr(Text::Height),
                        self.region_height
                    ));
                });
                if let Some(preview) = &mut self.region_preview {
                    draw_preview_image(ui, preview, egui::vec2(260.0, 160.0));
                }
            });
        }
    }

    fn overlay_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr(Text::Overlay));
        let show_keyboard = self.tr(Text::ShowKeyboardLabels);
        let show_mouse = self.tr(Text::ShowMouseLabels);
        let show_cursor = self.tr(Text::ShowCursor);
        let label_size = self.tr(Text::LabelSize);
        let opacity = self.tr(Text::Opacity);
        let label_position = self.tr(Text::LabelPosition);
        let label_font = self.tr(Text::LabelFont);
        let bottom = self.tr(Text::Bottom);
        let top = self.tr(Text::Top);
        let compact = self.tr(Text::Compact);
        let regular = self.tr(Text::Regular);
        let bold = self.tr(Text::Bold);
        let display_duration = self.tr(Text::DisplayDurationMs);
        ui.checkbox(&mut self.config.overlay.show_keyboard, show_keyboard);
        ui.checkbox(&mut self.config.overlay.show_mouse, show_mouse);
        ui.checkbox(&mut self.config.overlay.show_cursor, show_cursor);
        egui::ComboBox::from_label(label_position)
            .selected_text(self.label_position_label(self.config.overlay.label_position))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.config.overlay.label_position,
                    LabelPosition::BottomCenter,
                    bottom,
                );
                ui.selectable_value(
                    &mut self.config.overlay.label_position,
                    LabelPosition::TopCenter,
                    top,
                );
            });
        egui::ComboBox::from_label(label_font)
            .selected_text(self.overlay_font_label(self.config.overlay.label_font))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.config.overlay.label_font,
                    OverlayLabelFont::Compact,
                    compact,
                );
                ui.selectable_value(
                    &mut self.config.overlay.label_font,
                    OverlayLabelFont::Regular,
                    regular,
                );
                ui.selectable_value(
                    &mut self.config.overlay.label_font,
                    OverlayLabelFont::Bold,
                    bold,
                );
            });
        ui.add(
            egui::Slider::new(&mut self.config.overlay.label_scale, 0.75..=2.0).text(label_size),
        );
        ui.add(egui::Slider::new(&mut self.config.overlay.opacity, 0.2..=1.0).text(opacity));
        ui.add(
            egui::Slider::new(&mut self.config.overlay.display_ms, 300..=3_000)
                .text(display_duration),
        );
        egui::Grid::new("overlay_color_grid")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                color_control(
                    ui,
                    self.tr(Text::KeyBackground),
                    &mut self.config.overlay.keyboard_background,
                );
                color_control(
                    ui,
                    self.tr(Text::KeyLabel),
                    &mut self.config.overlay.keyboard_text,
                );
                color_control(
                    ui,
                    self.tr(Text::KeyBorder),
                    &mut self.config.overlay.keyboard_border,
                );
                color_control(
                    ui,
                    self.tr(Text::MouseColor),
                    &mut self.config.overlay.mouse_primary,
                );
                color_control(
                    ui,
                    self.tr(Text::CursorColor),
                    &mut self.config.overlay.cursor_color,
                );
            });
        if ui.button(self.tr(Text::PreviewOnScreen)).clicked() {
            self.open_screen_overlay_preview();
        }
        self.overlay_preview_ui(ui);
    }

    fn output_settings_ui(&mut self, ui: &mut egui::Ui) {
        let mut format_changed = false;
        let format = self.tr(Text::Format);
        let size = self.tr(Text::Size);
        let original = self.tr(Text::Original);
        egui::Grid::new("output_grid")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(format);
                egui::ComboBox::from_id_salt("format")
                    .selected_text(format_label(self.config.encoder.format))
                    .show_ui(ui, |ui| {
                        format_changed |= ui
                            .selectable_value(
                                &mut self.config.encoder.format,
                                OutputFormat::Mp4,
                                "MP4",
                            )
                            .changed();
                        format_changed |= ui
                            .selectable_value(
                                &mut self.config.encoder.format,
                                OutputFormat::Gif,
                                "GIF",
                            )
                            .changed();
                        format_changed |= ui
                            .selectable_value(
                                &mut self.config.encoder.format,
                                OutputFormat::WebM,
                                "WebM",
                            )
                            .changed();
                    });
                ui.end_row();

                ui.label(size);
                egui::ComboBox::from_id_salt("size")
                    .selected_text(self.size_label(self.config.encoder.size))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config.encoder.size,
                            OutputSize::Original,
                            original,
                        );
                        ui.selectable_value(
                            &mut self.config.encoder.size,
                            OutputSize::P720,
                            "720p",
                        );
                        ui.selectable_value(
                            &mut self.config.encoder.size,
                            OutputSize::P480,
                            "480p",
                        );
                    });
                ui.end_row();
            });

        if format_changed {
            self.update_output_extension();
        }

        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.output_path);
            if ui.button(self.tr(Text::Browse)).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name(format!(
                        "fvCapture.{}",
                        self.config.encoder.format.extension()
                    ))
                    .save_file()
            {
                self.output_path = path.display().to_string();
            }
        });
        ui.horizontal(|ui| {
            if ui.button(self.tr(Text::ChooseOutputFolder)).clicked() {
                self.choose_output_folder();
            }
            if ui.button(self.tr(Text::OpenOutputFolder)).clicked()
                && let Err(error) = self.open_output_folder()
            {
                self.error = Some(error);
            }
        });
    }

    fn action_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let recording = self.active.is_some();
            let busy = self.project_rx.is_some() || self.encoding_rx.is_some();
            if ui
                .add_enabled(
                    !recording && !busy,
                    egui::Button::new(format!("{} (F9)", self.tr(Text::StartRecording))),
                )
                .clicked()
            {
                self.start_recording();
            }

            if let Some(active) = &self.active {
                let paused = active.is_paused();
                let label = if paused {
                    self.tr(Text::Resume)
                } else {
                    self.tr(Text::Pause)
                };
                if ui.button(format!("{label} (F10)")).clicked() {
                    if paused {
                        active.resume();
                        self.status = StatusKey::Recording;
                    } else {
                        active.pause();
                        self.status = StatusKey::Paused;
                    }
                }
            }

            if ui
                .add_enabled(
                    recording,
                    egui::Button::new(format!("{} (F9)", self.tr(Text::Stop))),
                )
                .clicked()
            {
                self.stop_recording();
            }
        });
        let shortcut_hint = if self.global_shortcuts.is_some() {
            self.tr(Text::GlobalShortcutHint)
        } else {
            self.tr(Text::ShortcutHint)
        };
        ui.label(shortcut_hint);
    }

    fn update_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(info) = self.update_available.clone() else {
            return;
        };

        let can_update =
            self.active.is_none() && self.project_rx.is_none() && self.encoding_rx.is_none();
        egui::Window::new(self.tr(Text::UpdateAvailable))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(self.tr(Text::UpdateAvailableBody));
                ui.add_space(4.0);
                ui.label(format!(
                    "{}: {}",
                    self.tr(Text::CurrentVersion),
                    auto_update::CURRENT_VERSION
                ));
                ui.label(format!(
                    "{}: {}",
                    self.tr(Text::LatestVersion),
                    info.version
                ));
                ui.hyperlink_to(self.tr(Text::ReleasePage), &info.release_url);

                if let Some(notes) = &info.release_notes {
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .show(ui, |ui| {
                            ui.label(notes.trim());
                        });
                }

                if !can_update {
                    ui.add_space(8.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 160, 70),
                        self.tr(Text::StopBeforeUpdate),
                    );
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_update, egui::Button::new(self.tr(Text::UpdateNow)))
                        .clicked()
                    {
                        match auto_update::launch_updater(&info) {
                            Ok(()) => {
                                self.update_installing = true;
                                self.update_available = None;
                                self.update_error = None;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            Err(error) => {
                                self.update_error = Some(error.to_string());
                                self.update_available = Some(info.clone());
                            }
                        }
                    }
                    if ui.button(self.tr(Text::Later)).clicked() {
                        self.update_available = None;
                    }
                });
            });
    }

    fn choose_output_folder(&mut self) {
        let current_path = PathBuf::from(self.output_path.trim());
        let file_name = current_path
            .file_name()
            .map(|name| name.to_owned())
            .unwrap_or_else(|| {
                format!("fvCapture.{}", self.config.encoder.format.extension()).into()
            });
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.output_path = folder.join(file_name).display().to_string();
        }
    }

    fn output_folder(&self) -> PathBuf {
        let path = PathBuf::from(self.output_path.trim());
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn open_output_folder(&self) -> Result<(), String> {
        let folder = self.output_folder();
        if !folder.exists() {
            return Err(format!("folder does not exist: {}", folder.display()));
        }
        open_folder(&folder)
    }

    fn selected_monitor_label(&self) -> String {
        self.sources
            .iter()
            .find(|source| Some(source.id) == self.selected_monitor_id)
            .map(|source| source.name.clone())
            .unwrap_or_else(|| self.tr(Text::PrimaryMonitor).to_string())
    }

    fn selected_monitor_source(&self) -> Option<CaptureSource> {
        self.sources
            .iter()
            .find(|source| Some(source.id) == self.selected_monitor_id)
            .cloned()
            .or_else(|| self.sources.first().cloned())
    }

    fn selected_window_label(&self) -> String {
        self.window_sources
            .iter()
            .find(|window| Some(window.id) == self.selected_window_id)
            .map(window_label)
            .unwrap_or_else(|| self.tr(Text::NoWindowSelected).to_string())
    }

    fn window_preview_ui(&mut self, ui: &mut egui::Ui) {
        let Some(window_id) = self.selected_window_id else {
            return;
        };

        if self.window_preview_for != Some(window_id) {
            self.window_preview_for = Some(window_id);
            self.window_preview = None;
            self.window_preview_error = None;
            let config = CaptureConfig {
                selection: CaptureSelection::Window { id: window_id },
                fps: 1,
            };
            match XcapCaptureBackend::capture_once(&config, Instant::now()) {
                Ok(frame) => {
                    let origin = capture_origin(&config.selection).unwrap_or((0.0, 0.0));
                    self.window_preview = Some(PreviewImage::new(
                        frame.image,
                        (origin.0 as i32, origin.1 as i32),
                    ));
                }
                Err(error) => self.window_preview_error = Some(error.to_string()),
            }
        }

        ui.horizontal(|ui| {
            ui.label(self.tr(Text::WindowPreview));
            if ui.button(self.tr(Text::RefreshPreview)).clicked() {
                self.window_preview_for = None;
            }
        });

        if let Some(error) = &self.window_preview_error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
        }

        if let Some(preview) = &mut self.window_preview {
            draw_preview_image(
                ui,
                preview,
                egui::vec2(ui.available_width().min(520.0), 240.0),
            );
        }
    }

    fn open_region_selector(&mut self) {
        let Some(source) = self.selected_monitor_source() else {
            self.error = Some(self.tr(Text::NoMonitorSelected).to_string());
            return;
        };

        let config = CaptureConfig {
            selection: CaptureSelection::Monitor { id: source.id },
            fps: 1,
        };
        match XcapCaptureBackend::capture_once(&config, Instant::now()) {
            Ok(frame) => {
                self.region_preview = None;
                self.region_selector = Some(RegionSelectorState {
                    preview: PreviewImage::new(frame.image, (source.x, source.y)),
                    drag_start: None,
                    selection: Some((
                        self.region_x,
                        self.region_y,
                        self.region_width,
                        self.region_height,
                    )),
                });
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn overlay_preview_ui(&self, ui: &mut egui::Ui) {
        ui.label(self.tr(Text::Preview));
        let desired_size = egui::vec2(ui.available_width().min(500.0), 120.0);
        let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
        let image = render_overlay_preview_image(&self.config.overlay, 500, 120);
        let texture = ui.ctx().load_texture(
            "overlay_preview_inline",
            rgba_image_to_color_image(&image),
            egui::TextureOptions::NEAREST,
        );
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 76, 86)),
            egui::StrokeKind::Outside,
        );
    }

    fn open_screen_overlay_preview(&mut self) {
        let selection = match self.current_selection() {
            Ok(selection) => selection,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let config = CaptureConfig {
            selection: selection.clone(),
            fps: 1,
        };
        match XcapCaptureBackend::capture_once(&config, Instant::now()) {
            Ok(mut frame) => {
                let timeline = preview_timeline(frame.image.width(), frame.image.height());
                composite_frame(&mut frame.image, &timeline, 100, &self.config.overlay);
                let origin = capture_origin(&selection).unwrap_or((0.0, 0.0));
                self.screen_overlay_preview = Some(ScreenOverlayPreviewState {
                    preview: PreviewImage::new(frame.image, (origin.0 as i32, origin.1 as i32)),
                });
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn region_selector_viewport(&mut self, ctx: &egui::Context) {
        let viewport_id = egui::ViewportId::from_hash_of("region_selector");
        let action = {
            let Some(state) = &mut self.region_selector else {
                return;
            };

            let size = egui::vec2(
                state.preview.source_size.0.max(320) as f32,
                state.preview.source_size.1.max(240) as f32,
            );
            let position = egui::pos2(state.preview.origin.0 as f32, state.preview.origin.1 as f32);
            let builder = egui::ViewportBuilder::default()
                .with_title("fvCapture")
                .with_position(position)
                .with_inner_size(size)
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top();

            let mut action = RegionSelectorAction::None;
            ctx.show_viewport_immediate(viewport_id, builder, |ui, _| {
                action = region_selector_contents(ui, state);
            });
            action
        };

        match action {
            RegionSelectorAction::None => {}
            RegionSelectorAction::Cancel => {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
                self.region_selector = None;
            }
            RegionSelectorAction::Apply(x, y, width, height) => {
                self.region_preview = self
                    .region_selector
                    .as_ref()
                    .map(|state| crop_preview_image(&state.preview, x, y, width, height));
                self.region_x = x;
                self.region_y = y;
                self.region_width = width.max(1);
                self.region_height = height.max(1);
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
                self.region_selector = None;
            }
        }
    }

    fn screen_overlay_preview_viewport(&mut self, ctx: &egui::Context) {
        let close_preview = self.tr(Text::ClosePreview);
        let Some(state) = &mut self.screen_overlay_preview else {
            return;
        };

        let viewport_id = egui::ViewportId::from_hash_of("screen_overlay_preview");
        let position = egui::pos2(state.preview.origin.0 as f32, state.preview.origin.1 as f32);
        let size = egui::vec2(
            state.preview.source_size.0.max(320) as f32,
            state.preview.source_size.1.max(240) as f32,
        );
        let builder = egui::ViewportBuilder::default()
            .with_title("fvCapture Preview")
            .with_position(position)
            .with_inner_size(size)
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top();

        let mut close = false;
        ctx.show_viewport_immediate(viewport_id, builder, |ui, _| {
            let rect = ui.max_rect();
            let response = ui.interact(
                rect,
                ui.id().with("screen_overlay_preview_canvas"),
                egui::Sense::click(),
            );
            let texture_id = state.preview.texture_id(ui.ctx(), "screen_overlay_preview");
            ui.painter().image(
                texture_id,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) || response.double_clicked() {
                close = true;
            }
            let button_size = egui::vec2(180.0, 32.0);
            let button_rect =
                egui::Rect::from_min_size(rect.left_top() + egui::vec2(16.0, 16.0), button_size);
            if ui
                .put(button_rect, egui::Button::new(close_preview))
                .clicked()
            {
                close = true;
            }
        });
        if close {
            ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
            self.screen_overlay_preview = None;
        }
    }

    fn recording_preview_ui(&mut self, ui: &mut egui::Ui) {
        if self.recording_preview.is_none() {
            return;
        }

        let recording_preview = self.tr(Text::RecordingPreview);
        let play_preview = self.tr(Text::PlayPreview);
        let pause_preview = self.tr(Text::PausePreview);
        let loop_playback = self.tr(Text::LoopPlayback);
        let remove_first_frames = self.tr(Text::RemoveFirstFrames);
        let remove_last_frames = self.tr(Text::RemoveLastFrames);
        let trim_start_frame = self.tr(Text::TrimStartFrame);
        let trim_end_frame = self.tr(Text::TrimEndFrame);
        let export_selected_range = self.tr(Text::ExportSelectedRange);
        let frames_label = self.tr(Text::Frames);
        let output = self.tr(Text::Output);
        let can_export = self.encoding_rx.is_none();
        let output_path_label = self.output_path.clone();

        let mut export_request = None;
        let mut load_error = None;

        ui.add_space(8.0);
        ui.separator();
        ui.heading(recording_preview);
        ui.label(output);
        self.output_settings_ui(ui);
        ui.add_space(8.0);

        {
            let Some(preview) = &mut self.recording_preview else {
                return;
            };
            preview.update_playback(ui.ctx());
            preview.clamp_to_trim();

            let total_frames = preview.project.frame_count();
            let mut remove_first = preview.trim_start_frame;
            let mut remove_last = total_frames
                .saturating_sub(1)
                .saturating_sub(preview.trim_end_frame);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if preview.frame_image_index != Some(preview.current_frame) {
                        match load_project_frame(&preview.project, preview.current_frame) {
                            Ok(image) => {
                                preview.frame_image = Some(image);
                                preview.frame_image_index = Some(preview.current_frame);
                            }
                            Err(error) => {
                                load_error = Some(error);
                            }
                        }
                    }

                    if let Some(image) = &mut preview.frame_image {
                        draw_preview_image(
                            ui,
                            image,
                            egui::vec2(ui.available_width().min(520.0), 300.0),
                        );
                    }

                    let timeline_changed = trim_timeline_ui(
                        ui,
                        total_frames,
                        &mut preview.current_frame,
                        &mut preview.trim_start_frame,
                        &mut preview.trim_end_frame,
                    );
                    if timeline_changed {
                        preview.clamp_to_trim();
                    }
                });

                ui.vertical(|ui| {
                    ui.label(format!(
                        "{}: {} / {} {}",
                        frames_label,
                        preview.selected_frame_count(),
                        total_frames,
                        frames_label
                    ));
                    ui.label(format!("{}: {}", output, output_path_label));

                    ui.horizontal(|ui| {
                        let label = if preview.playing {
                            pause_preview
                        } else {
                            play_preview
                        };
                        if ui.button(label).clicked() {
                            preview.playing = !preview.playing;
                            preview.last_advance = Instant::now();
                            preview.clamp_to_trim();
                        }
                        ui.checkbox(&mut preview.loop_playback, loop_playback);
                    });

                    ui.add(
                        egui::Slider::new(
                            &mut preview.current_frame,
                            preview.trim_start_frame..=preview.trim_end_frame,
                        )
                        .text(frames_label),
                    );

                    ui.horizontal(|ui| {
                        ui.label(remove_first_frames);
                        if ui
                            .add(
                                egui::DragValue::new(&mut remove_first)
                                    .range(0..=total_frames.saturating_sub(1))
                                    .speed(1),
                            )
                            .changed()
                        {
                            preview.trim_start_frame = remove_first.min(preview.trim_end_frame);
                            preview.clamp_to_trim();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(remove_last_frames);
                        if ui
                            .add(
                                egui::DragValue::new(&mut remove_last)
                                    .range(0..=total_frames.saturating_sub(1))
                                    .speed(1),
                            )
                            .changed()
                        {
                            let last_frame = total_frames.saturating_sub(1);
                            preview.trim_end_frame = last_frame
                                .saturating_sub(remove_last)
                                .max(preview.trim_start_frame);
                            preview.clamp_to_trim();
                        }
                    });
                    ui.label(format!(
                        "{}: {}  {}: {}",
                        trim_start_frame,
                        preview.trim_start_frame,
                        trim_end_frame,
                        preview.trim_end_frame
                    ));

                    if ui
                        .add_enabled(can_export, egui::Button::new(export_selected_range))
                        .clicked()
                    {
                        export_request = Some((preview.trim_start_frame, preview.trim_end_frame));
                    }
                });
            });
        }

        if let Some(error) = load_error {
            self.error = Some(error);
        }
        if let Some((start_frame, end_frame)) = export_request {
            self.export_recording_preview(start_frame, end_frame);
        }
    }

    fn export_recording_preview(&mut self, start_frame: usize, end_frame: usize) {
        if self.encoding_rx.is_some() {
            return;
        }
        let Some(preview) = self.recording_preview.take() else {
            return;
        };

        let project = preview.project;
        let output_path = PathBuf::from(self.output_path.trim());
        let mut encoder = self.config.encoder.clone();
        encoder.fps = project.fps();
        self.status = StatusKey::Encoding;
        self.encoding_started_at = Some(Instant::now());
        self.error = None;

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = project
                .encode_range_with_config(start_frame, end_frame, &encoder, &output_path)
                .map_err(|error| error.to_string());
            let _ = tx.send((project, result));
        });
        self.encoding_rx = Some(rx);
    }

    fn progress_ui(&self, ui: &mut egui::Ui, text: &str) {
        let elapsed = self
            .encoding_started_at
            .map(|started| started.elapsed().as_secs_f32())
            .unwrap_or_default();
        let progress = (elapsed * 0.8).sin() * 0.5 + 0.5;
        ui.add(egui::ProgressBar::new(progress).animate(true).text(text));
    }

    fn app_appearance_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr(Text::Appearance));
        let shortcut_feedback_sound = self.tr(Text::ShortcutFeedbackSound);
        ui.checkbox(
            &mut self.config.shortcut_feedback_sound,
            shortcut_feedback_sound,
        );
    }

    fn language_ui(&mut self, ui: &mut egui::Ui) {
        let language = self.tr(Text::Language);
        let system = self.tr(Text::SystemLanguage);
        let english = self.tr(Text::English);
        let japanese = self.tr(Text::Japanese);
        let selected = self.language_label(self.language);
        ui.horizontal(|ui| {
            ui.label(language);
            egui::ComboBox::from_id_salt("language")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.language, LanguageChoice::System, system);
                    ui.selectable_value(&mut self.language, LanguageChoice::English, english);
                    ui.selectable_value(&mut self.language, LanguageChoice::Japanese, japanese);
                });
        });
    }

    fn status_label(&self, status: StatusKey) -> &'static str {
        match status {
            StatusKey::Ready => self.tr(Text::Ready),
            StatusKey::Recording => self.tr(Text::Recording),
            StatusKey::Paused => self.tr(Text::Paused),
            StatusKey::Encoding => self.tr(Text::Encoding),
            StatusKey::PreviewReady => self.tr(Text::PreviewReady),
            StatusKey::Saved => self.tr(Text::Saved),
        }
    }

    fn language_label(&self, language: LanguageChoice) -> &'static str {
        match language {
            LanguageChoice::System => self.tr(Text::SystemLanguage),
            LanguageChoice::English => self.tr(Text::English),
            LanguageChoice::Japanese => self.tr(Text::Japanese),
        }
    }

    fn size_label(&self, size: OutputSize) -> &'static str {
        match size {
            OutputSize::Original => self.tr(Text::Original),
            OutputSize::P720 => "720p",
            OutputSize::P480 => "480p",
        }
    }

    fn label_position_label(&self, position: LabelPosition) -> &'static str {
        match position {
            LabelPosition::BottomCenter => self.tr(Text::Bottom),
            LabelPosition::TopCenter => self.tr(Text::Top),
        }
    }

    fn overlay_font_label(&self, font: OverlayLabelFont) -> &'static str {
        match font {
            OverlayLabelFont::Compact => self.tr(Text::Compact),
            OverlayLabelFont::Regular => self.tr(Text::Regular),
            OverlayLabelFont::Bold => self.tr(Text::Bold),
        }
    }
}

impl Tr for FvCaptureApp {
    fn language_choice(&self) -> LanguageChoice {
        self.language
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionSelectorAction {
    None,
    Cancel,
    Apply(u32, u32, u32, u32),
}

fn region_selector_contents(
    ui: &mut egui::Ui,
    state: &mut RegionSelectorState,
) -> RegionSelectorAction {
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let texture_id = state.preview.texture_id(ui.ctx(), "region_selector");
    let painter = ui.painter_at(rect);
    painter.image(
        texture_id,
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        return RegionSelectorAction::Cancel;
    }

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let point = preview_to_source(rect, state.preview.source_size, pos);
        state.drag_start = Some(point);
        state.selection = Some((point.0, point.1, 1, 1));
    }

    if response.dragged()
        && let (Some(start), Some(pos)) = (state.drag_start, response.interact_pointer_pos())
    {
        let end = preview_to_source(rect, state.preview.source_size, pos);
        let min_x = start.0.min(end.0);
        let min_y = start.1.min(end.1);
        let max_x = start.0.max(end.0).min(state.preview.source_size.0);
        let max_y = start.1.max(end.1).min(state.preview.source_size.1);
        state.selection = Some((
            min_x,
            min_y,
            max_x.saturating_sub(min_x).max(1),
            max_y.saturating_sub(min_y).max(1),
        ));
    }

    if response.drag_stopped() {
        state.drag_start = None;
        if let Some((x, y, width, height)) = state.selection {
            return RegionSelectorAction::Apply(x, y, width, height);
        }
    }

    if let Some((x, y, width, height)) = state.selection {
        let selected_rect =
            source_rect_to_ui_rect(rect, state.preview.source_size, x, y, width, height)
                .intersect(rect);
        draw_dim_outside(&painter, rect, selected_rect);
        painter.rect_stroke(
            selected_rect,
            0.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(57, 255, 210)),
            egui::StrokeKind::Outside,
        );
        return RegionSelectorAction::None;
    }

    draw_dim_outside(&painter, rect, egui::Rect::NOTHING);
    RegionSelectorAction::None
}

fn draw_dim_outside(painter: &egui::Painter, rect: egui::Rect, selected: egui::Rect) {
    let dim = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 96);
    if selected.is_positive() {
        painter.rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, selected.min.y)),
            0.0,
            dim,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.min.x, selected.max.y), rect.max),
            0.0,
            dim,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, selected.min.y),
                egui::pos2(selected.min.x, selected.max.y),
            ),
            0.0,
            dim,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(selected.max.x, selected.min.y),
                egui::pos2(rect.max.x, selected.max.y),
            ),
            0.0,
            dim,
        );
    } else {
        painter.rect_filled(rect, 0.0, dim);
    }
}

fn draw_preview_image(ui: &mut egui::Ui, preview: &mut PreviewImage, max_size: egui::Vec2) {
    let source = egui::vec2(preview.source_size.0 as f32, preview.source_size.1 as f32);
    let scale = (max_size.x / source.x).min(max_size.y / source.y).min(1.0);
    let size = egui::vec2((source.x * scale).max(1.0), (source.y * scale).max(1.0));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let texture_id = preview.texture_id(ui.ctx(), "preview_image");
    ui.painter().image(
        texture_id,
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    ui.painter().rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 96, 106)),
        egui::StrokeKind::Outside,
    );
}

fn crop_preview_image(
    preview: &PreviewImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> PreviewImage {
    let source_width = preview.source_size.0 as usize;
    let source_height = preview.source_size.1 as usize;
    let x = (x as usize).min(source_width.saturating_sub(1));
    let y = (y as usize).min(source_height.saturating_sub(1));
    let width = (width as usize).min(source_width.saturating_sub(x)).max(1);
    let height = (height as usize)
        .min(source_height.saturating_sub(y))
        .max(1);
    let mut pixels = Vec::with_capacity(width * height);
    for row in y..y + height {
        let start = row * source_width + x;
        pixels.extend_from_slice(&preview.image.pixels[start..start + width]);
    }

    PreviewImage {
        image: egui::ColorImage::new([width, height], pixels),
        texture: None,
        origin: (preview.origin.0 + x as i32, preview.origin.1 + y as i32),
        source_size: (width as u32, height as u32),
    }
}

fn load_project_frame(
    project: &RecordingProject,
    frame_index: usize,
) -> Result<PreviewImage, String> {
    let path = project.frame_path(frame_index);
    let image = ImageReader::open(&path)
        .map_err(|error| format!("failed to open preview frame: {error}"))?
        .decode()
        .map_err(|error| format!("failed to decode preview frame: {error}"))?
        .to_rgba8();
    Ok(PreviewImage::new(image, (0, 0)))
}

fn trim_timeline_ui(
    ui: &mut egui::Ui,
    total_frames: usize,
    current_frame: &mut usize,
    trim_start_frame: &mut usize,
    trim_end_frame: &mut usize,
) -> bool {
    if total_frames == 0 {
        return false;
    }

    let desired_size = egui::vec2(ui.available_width().min(520.0), 42.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.center().y - 4.0),
        egui::pos2(rect.right(), rect.center().y + 4.0),
    );
    let last_frame = total_frames - 1;
    let frame_to_x = |frame: usize| {
        let t = if last_frame == 0 {
            0.0
        } else {
            frame as f32 / last_frame as f32
        };
        egui::lerp(bar.left()..=bar.right(), t)
    };
    let x_to_frame = |x: f32| {
        let t = ((x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0);
        (t * last_frame as f32).round() as usize
    };

    let mut changed = false;
    *trim_start_frame = (*trim_start_frame).min(last_frame);
    *trim_end_frame = (*trim_end_frame).min(last_frame).max(*trim_start_frame);
    *current_frame = (*current_frame).clamp(*trim_start_frame, *trim_end_frame);

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        *current_frame = x_to_frame(pos.x).clamp(*trim_start_frame, *trim_end_frame);
        changed = true;
    }

    let start_x = frame_to_x(*trim_start_frame);
    let end_x = frame_to_x(*trim_end_frame);
    let current_x = frame_to_x(*current_frame);
    let selected_bar = egui::Rect::from_min_max(
        egui::pos2(start_x, bar.top()),
        egui::pos2(end_x, bar.bottom()),
    );
    painter.rect_filled(bar, 3.0, egui::Color32::from_rgb(58, 64, 72));
    painter.rect_filled(selected_bar, 3.0, egui::Color32::from_rgb(57, 255, 210));
    painter.line_segment(
        [
            egui::pos2(current_x, rect.top() + 6.0),
            egui::pos2(current_x, rect.bottom() - 6.0),
        ],
        egui::Stroke::new(2.0, egui::Color32::WHITE),
    );

    let handle_size = egui::vec2(12.0, 30.0);
    let start_rect =
        egui::Rect::from_center_size(egui::pos2(start_x, rect.center().y), handle_size);
    let end_rect = egui::Rect::from_center_size(egui::pos2(end_x, rect.center().y), handle_size);
    let start_response = ui.interact(
        start_rect,
        ui.id().with("trim_start_handle"),
        egui::Sense::drag(),
    );
    let end_response = ui.interact(
        end_rect,
        ui.id().with("trim_end_handle"),
        egui::Sense::drag(),
    );

    if start_response.dragged()
        && let Some(pos) = start_response.interact_pointer_pos()
    {
        *trim_start_frame = x_to_frame(pos.x).min(*trim_end_frame);
        *current_frame = (*current_frame).max(*trim_start_frame);
        changed = true;
    }
    if end_response.dragged()
        && let Some(pos) = end_response.interact_pointer_pos()
    {
        *trim_end_frame = x_to_frame(pos.x).max(*trim_start_frame);
        *current_frame = (*current_frame).min(*trim_end_frame);
        changed = true;
    }

    painter.rect_filled(start_rect, 3.0, egui::Color32::from_rgb(245, 247, 250));
    painter.rect_filled(end_rect, 3.0, egui::Color32::from_rgb(245, 247, 250));
    painter.rect_stroke(
        start_rect,
        3.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(25, 31, 36)),
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(
        end_rect,
        3.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(25, 31, 36)),
        egui::StrokeKind::Outside,
    );

    changed
}

fn render_overlay_preview_image(settings: &OverlaySettings, width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(width, height, Rgba([24, 27, 31, 255]));
    let timeline = preview_timeline(width, height);
    composite_frame(&mut image, &timeline, 100, settings);
    image
}

fn preview_timeline(width: u32, height: u32) -> OverlayTimeline {
    let x = width as f64 * 0.72;
    let y = height as f64 * 0.45;
    OverlayTimeline {
        events: vec![
            OverlayEvent {
                start_ms: 0,
                duration_ms: 1_000,
                kind: OverlayEventKind::KeyCombo(vec![
                    KeyCode::Control,
                    KeyCode::Shift,
                    KeyCode::Character('S'),
                ]),
            },
            OverlayEvent {
                start_ms: 0,
                duration_ms: 700,
                kind: OverlayEventKind::MouseClick {
                    button: MouseButton::Left,
                    x,
                    y,
                },
            },
        ],
        mouse_positions: vec![(0, Point { x, y })],
    }
}

fn rgba_image_to_color_image(image: &RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    )
}

fn load_app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/fvCapture.png")).ok()
}

fn load_status_icon(status: StatusKey) -> Option<egui::IconData> {
    let mut image = image::load_from_memory(include_bytes!("../../../assets/icons/fvCapture.png"))
        .ok()?
        .to_rgba8();

    match status {
        StatusKey::Recording => {
            draw_icon_badge(&mut image, Rgba([224, 32, 32, 255]), IconBadgeGlyph::None)
        }
        StatusKey::Paused => {
            draw_icon_badge(&mut image, Rgba([245, 166, 35, 255]), IconBadgeGlyph::Pause)
        }
        StatusKey::Ready | StatusKey::Encoding | StatusKey::PreviewReady | StatusKey::Saved => {}
    }

    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconBadgeGlyph {
    None,
    Pause,
}

fn draw_icon_badge(image: &mut RgbaImage, color: Rgba<u8>, glyph: IconBadgeGlyph) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    let radius = (width.min(height) as f32 * 0.18).round() as i32;
    let center_x = width - radius - 14;
    let center_y = height - radius - 14;

    for y in center_y - radius..=center_y + radius {
        for x in center_x - radius..=center_x + radius {
            let dx = x - center_x;
            let dy = y - center_y;
            if dx * dx + dy * dy <= radius * radius {
                blend_icon_pixel(image, x, y, color);
            }
        }
    }

    if glyph == IconBadgeGlyph::Pause {
        let bar_width = (radius as f32 * 0.23).round() as i32;
        let bar_height = (radius as f32 * 0.95).round() as i32;
        let gap = (radius as f32 * 0.22).round() as i32;
        let top = center_y - bar_height / 2;
        let left = center_x - gap / 2 - bar_width;
        let right = center_x + gap / 2;
        let white = Rgba([255, 255, 255, 255]);
        fill_icon_rect(image, left, top, bar_width, bar_height, white);
        fill_icon_rect(image, right, top, bar_width, bar_height, white);
    }
}

fn fill_icon_rect(image: &mut RgbaImage, x: i32, y: i32, width: i32, height: i32, color: Rgba<u8>) {
    for py in y..y + height {
        for px in x..x + width {
            blend_icon_pixel(image, px, py, color);
        }
    }
}

fn blend_icon_pixel(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
        return;
    }
    image.put_pixel(x as u32, y as u32, color);
}

fn open_folder(folder: &Path) -> Result<(), String> {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("explorer");
        command.arg(folder);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(folder);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(folder);
        command
    };
    suppress_console_window(&mut command);
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

#[cfg(windows)]
fn play_feedback_sound() {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBeep(u_type: u32) -> i32;
    }

    const MB_ICONASTERISK: u32 = 0x0000_0040;
    unsafe {
        let _ = MessageBeep(MB_ICONASTERISK);
    }
}

#[cfg(not(windows))]
fn play_feedback_sound() {}

fn preview_to_source(rect: egui::Rect, source_size: (u32, u32), pos: egui::Pos2) -> (u32, u32) {
    let local = (pos - rect.min).clamp(egui::Vec2::ZERO, rect.size());
    (
        ((local.x / rect.width().max(1.0)) * source_size.0 as f32)
            .round()
            .max(0.0)
            .min(source_size.0 as f32) as u32,
        ((local.y / rect.height().max(1.0)) * source_size.1 as f32)
            .round()
            .max(0.0)
            .min(source_size.1 as f32) as u32,
    )
}

fn source_rect_to_ui_rect(
    rect: egui::Rect,
    source_size: (u32, u32),
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> egui::Rect {
    let scale_x = rect.width() / source_size.0.max(1) as f32;
    let scale_y = rect.height() / source_size.1.max(1) as f32;
    egui::Rect::from_min_size(
        rect.min + egui::vec2(x as f32 * scale_x, y as f32 * scale_y),
        egui::vec2(width as f32 * scale_x, height as f32 * scale_y),
    )
}

fn window_label(window: &CaptureWindowSource) -> String {
    let title = if window.title.trim().is_empty() {
        "(untitled)"
    } else {
        window.title.trim()
    };
    if window.app_name.trim().is_empty() {
        format!("{} - {}x{}", title, window.width, window.height)
    } else {
        format!(
            "{} - {} ({}x{})",
            window.app_name.trim(),
            title,
            window.width,
            window.height
        )
    }
}

fn color_control(ui: &mut egui::Ui, label: &str, color: &mut OverlayColor) {
    ui.label(label);
    let mut ui_color = egui::Color32::from_rgb(color.r, color.g, color.b);
    if egui::color_picker::color_edit_button_srgba(
        ui,
        &mut ui_color,
        egui::color_picker::Alpha::Opaque,
    )
    .changed()
    {
        color.r = ui_color.r();
        color.g = ui_color.g();
        color.b = ui_color.b();
    }
    ui.end_row();
}

fn default_output_path(format: OutputFormat) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    PathBuf::from(format!("fvCapture-{stamp}.{}", format.extension()))
}

fn settings_path() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("fvCapture").join("settings.json"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("fvCapture")
                .join("settings.json")
        })
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .map(|path| path.join("fvCapture").join("settings.json"))
    }
}

fn load_saved_settings(path: Option<&Path>) -> (SavedGuiSettings, Option<String>) {
    let Some(path) = path else {
        return (
            SavedGuiSettings::default(),
            Some("settings folder was not found".to_string()),
        );
    };
    if !path.exists() {
        return (SavedGuiSettings::default(), None);
    }

    match std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read settings: {error}"))
        .and_then(|content| {
            serde_json::from_str::<SavedGuiSettings>(&content)
                .map_err(|error| format!("failed to parse settings: {error}"))
        }) {
        Ok(settings) => (settings, None),
        Err(error) => (SavedGuiSettings::default(), Some(error)),
    }
}

fn save_saved_settings(path: &Path, settings: &SavedGuiSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create settings folder: {error}"))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("failed to serialize settings: {error}"))?;
    std::fs::write(path, content).map_err(|error| format!("failed to write settings: {error}"))
}

fn format_label(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Mp4 => "MP4",
        OutputFormat::Gif => "GIF",
        OutputFormat::WebM => "WebM",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_positions_map_to_source_coordinates() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 100.0));

        assert_eq!(
            preview_to_source(rect, (400, 200), egui::pos2(60.0, 70.0)),
            (100, 100)
        );
        assert_eq!(
            preview_to_source(rect, (400, 200), egui::pos2(-10.0, -10.0)),
            (0, 0)
        );
    }
}
