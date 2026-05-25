#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use fv_capture_core::{
    ActiveRecording, AppConfig, CaptureBackend, CaptureConfig, CaptureSelection, CaptureSource,
    CaptureWindowSource, KeyCode, LabelPosition, MouseButton, OutputFormat, OutputSize,
    OverlayColor, OverlayEvent, OverlayEventKind, OverlayLabelFont, OverlayTimeline, Point,
    RecordingRequest, RecordingSummary, XcapCaptureBackend, capture_origin, composite_frame,
};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey},
};
use image::RgbaImage;

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
        .with_inner_size([820.0, 680.0])
        .with_min_inner_size([760.0, 620.0]);
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
            Ok(Box::new(FvCaptureApp::new()))
        }),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceMode {
    Primary,
    Monitor,
    Window,
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    Capture,
    Overlay,
    Output,
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

struct GlobalShortcutState {
    _manager: GlobalHotKeyManager,
    start_stop_id: u32,
    pause_resume_id: u32,
}

impl GlobalShortcutState {
    fn register() -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new().map_err(|error| error.to_string())?;
        let start_stop = HotKey::new(None, Code::F9);
        let pause_resume = HotKey::new(None, Code::F10);
        manager
            .register_all(&[start_stop, pause_resume])
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _manager: manager,
            start_stop_id: start_stop.id(),
            pause_resume_id: pause_resume.id(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    StartStop,
    PauseResume,
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
    encoding_rx: Option<Receiver<Result<RecordingSummary, String>>>,
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
    screen_overlay_preview: Option<ScreenOverlayPreviewState>,
    last_summary: Option<RecordingSummary>,
    status: StatusKey,
    error: Option<String>,
    language: LanguageChoice,
    ui_font_config: ui_fonts::UiFontConfig,
    ui_font_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKey {
    Ready,
    Recording,
    Paused,
    Encoding,
    Saved,
}

impl FvCaptureApp {
    fn new() -> Self {
        let (global_shortcuts, global_shortcut_error) = match GlobalShortcutState::register() {
            Ok(shortcuts) => (Some(shortcuts), None),
            Err(error) => (None, Some(error)),
        };
        let mut app = Self {
            config: AppConfig::default(),
            active_tab: AppTab::Capture,
            sources: Vec::new(),
            window_sources: Vec::new(),
            source_mode: SourceMode::Primary,
            selected_monitor_id: None,
            selected_window_id: None,
            region_x: 0,
            region_y: 0,
            region_width: 1280,
            region_height: 720,
            output_path: default_output_path(OutputFormat::Mp4).display().to_string(),
            active: None,
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
            screen_overlay_preview: None,
            last_summary: None,
            status: StatusKey::Ready,
            error: None,
            language: LanguageChoice::System,
            ui_font_config: ui_fonts::UiFontConfig::default(),
            ui_font_error: None,
        };
        app.refresh_sources();
        app
    }

    fn refresh_sources(&mut self) {
        let backend = XcapCaptureBackend::default();
        match backend.list_sources() {
            Ok(sources) => {
                self.selected_monitor_id = sources
                    .iter()
                    .find(|source| source.is_primary)
                    .or_else(|| sources.first())
                    .map(|source| source.id);
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

    fn start_recording(&mut self) {
        if self.active.is_some() || self.encoding_rx.is_some() {
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
            let result = active.stop().map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
        self.encoding_rx = Some(rx);
    }

    fn update_output_extension(&mut self) {
        let path = PathBuf::from(self.output_path.trim());
        let extension = self.config.encoder.format.extension();
        let updated = path.with_extension(extension);
        self.output_path = updated.display().to_string();
    }

    fn poll_encoding(&mut self) {
        let result = self.encoding_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some(result) = result else {
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
                self.status = StatusKey::Ready;
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
        let start_stop_id = shortcuts.start_stop_id;
        let pause_resume_id = shortcuts.pause_resume_id;
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state != HotKeyState::Pressed {
                continue;
            }
            if event.id == start_stop_id {
                self.handle_shortcut_action(ShortcutAction::StartStop);
            } else if event.id == pause_resume_id {
                self.handle_shortcut_action(ShortcutAction::PauseResume);
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

    fn handle_shortcut_action(&mut self, action: ShortcutAction) {
        match action {
            ShortcutAction::StartStop => {
                if self.active.is_some() {
                    self.stop_recording();
                } else if self.encoding_rx.is_none() {
                    self.start_recording();
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
                }
            }
        }
    }
}

impl eframe::App for FvCaptureApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_encoding();
        self.poll_update_check();
        self.poll_global_shortcuts();
        self.handle_shortcuts(&ctx);
        if self.active.is_some() || self.encoding_rx.is_some() || self.update_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::Frame::default()
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                self.content_ui(ui);
            });
        self.update_dialog_ui(&ctx);
        self.region_selector_viewport(&ctx);
        self.screen_overlay_preview_viewport(&ctx);
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
        if self.encoding_rx.is_some() {
            self.encoding_progress_ui(ui);
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
        if let Some(error) = &self.ui_font_error {
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
            AppTab::Output => self.output_ui(ui),
            AppTab::Appearance => self.app_appearance_ui(ui),
        }
        ui.add_space(8.0);
        ui.separator();
        self.action_ui(ui);
    }
}

impl FvCaptureApp {
    fn tab_bar_ui(&mut self, ui: &mut egui::Ui) {
        let capture = self.tr(Text::CaptureTab);
        let overlay = self.tr(Text::OverlayTab);
        let output = self.tr(Text::OutputTab);
        let appearance = self.tr(Text::AppearanceTab);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, AppTab::Capture, capture);
            ui.selectable_value(&mut self.active_tab, AppTab::Overlay, overlay);
            ui.selectable_value(&mut self.active_tab, AppTab::Output, output);
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

    fn output_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr(Text::Output));
        let mut format_changed = false;
        let format = self.tr(Text::Format);
        let fps = self.tr(Text::Fps);
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

                ui.label(fps);
                ui.add(
                    egui::DragValue::new(&mut self.config.capture.fps)
                        .range(1..=60)
                        .speed(1),
                );
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
            let encoding = self.encoding_rx.is_some();
            if ui
                .add_enabled(
                    !recording && !encoding,
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

        let can_update = self.active.is_none() && self.encoding_rx.is_none();
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
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0, egui::Color32::from_rgb(24, 27, 31));
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 76, 86)),
            egui::StrokeKind::Outside,
        );

        if self.config.overlay.show_keyboard {
            let key_bg = color32(self.config.overlay.keyboard_background, 220);
            let key_text = color32(self.config.overlay.keyboard_text, 255);
            let key_border = color32(self.config.overlay.keyboard_border, 190);
            let scale = self.config.overlay.label_scale;
            let key_h = 30.0 * scale;
            let labels = ["Ctrl", "Shift", "S"];
            let widths = [64.0 * scale, 78.0 * scale, 42.0 * scale];
            let total_width = widths.iter().sum::<f32>() + 8.0 * scale * 2.0;
            let mut x = rect.center().x - total_width / 2.0;
            let y = match self.config.overlay.label_position {
                LabelPosition::BottomCenter => rect.bottom() - key_h - 18.0,
                LabelPosition::TopCenter => rect.top() + 18.0,
            };
            for (label, width) in labels.iter().zip(widths) {
                let key_rect =
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, key_h));
                painter.rect_filled(key_rect, 5.0, key_bg);
                painter.rect_stroke(
                    key_rect,
                    5.0,
                    egui::Stroke::new(1.5, key_border),
                    egui::StrokeKind::Outside,
                );
                let font_size = match self.config.overlay.label_font {
                    OverlayLabelFont::Compact => 13.0,
                    OverlayLabelFont::Regular => 14.0,
                    OverlayLabelFont::Bold => 15.0,
                } * scale;
                painter.text(
                    key_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    *label,
                    egui::FontId::proportional(font_size),
                    key_text,
                );
                x += width + 8.0 * scale;
            }
        }

        if self.config.overlay.show_mouse {
            let center = egui::pos2(rect.left() + 74.0, rect.center().y);
            painter.circle_stroke(
                center,
                24.0,
                egui::Stroke::new(3.0, color32(self.config.overlay.mouse_primary, 220)),
            );
            painter.circle_stroke(
                center,
                12.0,
                egui::Stroke::new(2.0, color32(self.config.overlay.mouse_secondary, 170)),
            );
        }

        if self.config.overlay.show_cursor {
            let pos = egui::pos2(rect.right() - 86.0, rect.center().y - 24.0);
            draw_egui_cursor(
                &painter,
                pos,
                color32(self.config.overlay.cursor_color, 245),
            );
        }
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
        let Some(state) = &mut self.region_selector else {
            return;
        };

        let viewport_id = egui::ViewportId::from_hash_of("region_selector");
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

        match action {
            RegionSelectorAction::None => {}
            RegionSelectorAction::Cancel => {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
                self.region_selector = None;
            }
            RegionSelectorAction::Apply(x, y, width, height) => {
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
            let response = ui.allocate_rect(rect, egui::Sense::click());
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
            egui::Area::new("screen_overlay_preview_actions".into())
                .fixed_pos(rect.left_top() + egui::vec2(16.0, 16.0))
                .show(ui.ctx(), |ui| {
                    if ui.button(close_preview).clicked() {
                        close = true;
                    }
                });
        });
        if close {
            ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
            self.screen_overlay_preview = None;
        }
    }

    fn encoding_progress_ui(&self, ui: &mut egui::Ui) {
        let elapsed = self
            .encoding_started_at
            .map(|started| started.elapsed().as_secs_f32())
            .unwrap_or_default();
        let progress = (elapsed * 0.8).sin() * 0.5 + 0.5;
        ui.add(
            egui::ProgressBar::new(progress)
                .animate(true)
                .text(self.tr(Text::EncodingProgress)),
        );
    }

    fn app_appearance_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr(Text::Appearance));
        let mut changed = false;
        let ui_font_weight = self.tr(Text::UiFontWeight);
        let regular = self.tr(Text::Regular);
        let medium = self.tr(Text::Medium);
        let bold = self.tr(Text::Bold);
        egui::ComboBox::from_label(ui_font_weight)
            .selected_text(self.ui_font_weight_label(self.ui_font_config.weight))
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(
                        &mut self.ui_font_config.weight,
                        ui_fonts::UiFontWeight::Regular,
                        regular,
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut self.ui_font_config.weight,
                        ui_fonts::UiFontWeight::Medium,
                        medium,
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut self.ui_font_config.weight,
                        ui_fonts::UiFontWeight::Bold,
                        bold,
                    )
                    .changed();
            });

        ui.horizontal(|ui| {
            let label = self
                .ui_font_config
                .custom_font_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| self.tr(Text::BundledFont).to_string());
            ui.label(format!("{}: {label}", self.tr(Text::UiFont)));
            if ui.button(self.tr(Text::Browse)).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Font", &["ttf", "otf", "ttc"])
                    .pick_file()
            {
                self.ui_font_config.custom_font_path = Some(path);
                changed = true;
            }
            if ui.button(self.tr(Text::Reset)).clicked() {
                self.ui_font_config.custom_font_path = None;
                changed = true;
            }
        });

        if changed {
            match ui_fonts::install(ui.ctx(), &self.ui_font_config) {
                Ok(()) => self.ui_font_error = None,
                Err(error) => self.ui_font_error = Some(error),
            }
        }
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

    fn ui_font_weight_label(&self, weight: ui_fonts::UiFontWeight) -> &'static str {
        match weight {
            ui_fonts::UiFontWeight::Regular => self.tr(Text::Regular),
            ui_fonts::UiFontWeight::Medium => self.tr(Text::Medium),
            ui_fonts::UiFontWeight::Bold => self.tr(Text::Bold),
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

    if (response.drag_started() || response.clicked())
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
        let mut action = RegionSelectorAction::None;
        egui::Area::new("region_selector_actions".into())
            .fixed_pos(selected_rect.left_top() + egui::vec2(12.0, 12.0))
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        action = RegionSelectorAction::Apply(x, y, width, height);
                    }
                    if ui.button("Cancel").clicked() {
                        action = RegionSelectorAction::Cancel;
                    }
                });
            });
        return action;
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

fn draw_egui_cursor(painter: &egui::Painter, pos: egui::Pos2, fill: egui::Color32) {
    let outline = egui::Color32::from_rgb(3, 8, 12);
    let outer = [
        egui::pos2(pos.x, pos.y),
        egui::pos2(pos.x, pos.y + 29.0),
        egui::pos2(pos.x + 8.0, pos.y + 21.0),
        egui::pos2(pos.x + 13.0, pos.y + 33.0),
        egui::pos2(pos.x + 18.0, pos.y + 31.0),
        egui::pos2(pos.x + 13.0, pos.y + 19.0),
        egui::pos2(pos.x + 24.0, pos.y + 19.0),
    ];
    let inner = [
        egui::pos2(pos.x + 3.0, pos.y + 7.0),
        egui::pos2(pos.x + 3.0, pos.y + 22.0),
        egui::pos2(pos.x + 9.0, pos.y + 16.0),
        egui::pos2(pos.x + 14.0, pos.y + 28.0),
        egui::pos2(pos.x + 15.0, pos.y + 27.0),
        egui::pos2(pos.x + 10.0, pos.y + 15.0),
        egui::pos2(pos.x + 18.0, pos.y + 16.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        outer.to_vec(),
        outline,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        inner.to_vec(),
        fill,
        egui::Stroke::NONE,
    ));
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

fn color32(color: OverlayColor, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, alpha)
}

fn default_output_path(format: OutputFormat) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    PathBuf::from(format!("fvCapture-{stamp}.{}", format.extension()))
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
