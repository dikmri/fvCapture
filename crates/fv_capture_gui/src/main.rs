#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use fv_capture_core::{
    ActiveRecording, AppConfig, CaptureBackend, CaptureSelection, CaptureSource,
    CaptureWindowSource, LabelPosition, OutputFormat, OutputSize, OverlayColor, OverlayLabelFont,
    RecordingRequest, RecordingSummary, XcapCaptureBackend,
};

mod i18n;
mod ui_fonts;

use i18n::{LanguageChoice, Text, Tr};

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "fv_capture_core=info".to_string()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 650.0])
            .with_min_inner_size([440.0, 560.0]),
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

struct FvCaptureApp {
    config: AppConfig,
    sources: Vec<CaptureSource>,
    window_sources: Vec<CaptureWindowSource>,
    source_mode: SourceMode,
    selected_monitor_id: Option<u32>,
    selected_window_id: Option<u32>,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    region_drag_start: Option<(u32, u32)>,
    output_path: String,
    active: Option<ActiveRecording>,
    encoding_rx: Option<Receiver<Result<RecordingSummary, String>>>,
    encoding_started_at: Option<Instant>,
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
        let mut app = Self {
            config: AppConfig::default(),
            sources: Vec::new(),
            window_sources: Vec::new(),
            source_mode: SourceMode::Primary,
            selected_monitor_id: None,
            selected_window_id: None,
            region_x: 0,
            region_y: 0,
            region_width: 1280,
            region_height: 720,
            region_drag_start: None,
            output_path: default_output_path(OutputFormat::Mp4).display().to_string(),
            active: None,
            encoding_rx: None,
            encoding_started_at: None,
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

    fn start_recording(&mut self) {
        if self.active.is_some() || self.encoding_rx.is_some() {
            return;
        }

        let output_path = PathBuf::from(self.output_path.trim());
        let selection = match self.source_mode {
            SourceMode::Primary => CaptureSelection::PrimaryMonitor,
            SourceMode::Monitor => self
                .selected_monitor_id
                .map(|id| CaptureSelection::Monitor { id })
                .unwrap_or(CaptureSelection::PrimaryMonitor),
            SourceMode::Window => {
                let Some(id) = self.selected_window_id else {
                    self.error = Some(self.tr(Text::NoWindowSelected).to_string());
                    return;
                };
                CaptureSelection::Window { id }
            }
            SourceMode::Region => CaptureSelection::Region {
                monitor_id: self.selected_monitor_id,
                x: self.region_x,
                y: self.region_y,
                width: self.region_width.max(1),
                height: self.region_height.max(1),
            },
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

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.key_pressed(egui::Key::F9)) {
            if self.active.is_some() {
                self.stop_recording();
            } else if self.encoding_rx.is_none() {
                self.start_recording();
            }
        }

        if ctx.input(|input| input.key_pressed(egui::Key::F10))
            && let Some(active) = &self.active
        {
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

impl eframe::App for FvCaptureApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_encoding();
        self.handle_shortcuts(&ctx);
        if self.active.is_some() || self.encoding_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::Frame::default()
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.content_ui(ui));
            });
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

        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
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
        self.source_ui(ui);
        ui.separator();
        self.overlay_ui(ui);
        ui.separator();
        self.output_ui(ui);
        ui.separator();
        self.app_appearance_ui(ui);
        ui.separator();
        self.action_ui(ui);
    }
}

impl FvCaptureApp {
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
            egui::ComboBox::from_label(window)
                .selected_text(self.selected_window_label())
                .show_ui(ui, |ui| {
                    for window in &self.window_sources {
                        ui.selectable_value(
                            &mut self.selected_window_id,
                            Some(window.id),
                            window_label(window),
                        );
                    }
                });
        }

        if self.source_mode == SourceMode::Region {
            self.region_picker_ui(ui);
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
        ui.label(self.tr(Text::ShortcutHint));
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

    fn region_picker_ui(&mut self, ui: &mut egui::Ui) {
        let Some(source) = self.selected_monitor_source() else {
            ui.label(self.tr(Text::NoMonitorSelected));
            return;
        };

        ui.label(self.tr(Text::DragToSelectRegion));
        let max_width = ui.available_width().clamp(240.0, 500.0);
        let max_height = 240.0;
        let scale = (max_width / source.width as f32)
            .min(max_height / source.height as f32)
            .max(0.05);
        let preview_size = egui::vec2(source.width as f32 * scale, source.height as f32 * scale);
        let (rect, response) = ui.allocate_exact_size(preview_size, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(28, 31, 35));
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 96, 106)),
            egui::StrokeKind::Outside,
        );

        if (response.drag_started() || response.clicked())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let point = preview_to_source(rect, scale, pos);
            self.region_drag_start = Some(point);
            self.region_x = point.0.min(source.width.saturating_sub(1));
            self.region_y = point.1.min(source.height.saturating_sub(1));
            self.region_width = 1;
            self.region_height = 1;
        }

        if response.dragged()
            && let (Some(start), Some(pos)) =
                (self.region_drag_start, response.interact_pointer_pos())
        {
            let end = preview_to_source(rect, scale, pos);
            let min_x = start.0.min(end.0).min(source.width.saturating_sub(1));
            let min_y = start.1.min(end.1).min(source.height.saturating_sub(1));
            let max_x = start.0.max(end.0).min(source.width);
            let max_y = start.1.max(end.1).min(source.height);
            self.region_x = min_x;
            self.region_y = min_y;
            self.region_width = max_x.saturating_sub(min_x).max(1);
            self.region_height = max_y.saturating_sub(min_y).max(1);
        }

        if response.drag_stopped() {
            self.region_drag_start = None;
        }

        self.region_width = self
            .region_width
            .min(source.width.saturating_sub(self.region_x));
        self.region_height = self
            .region_height
            .min(source.height.saturating_sub(self.region_y));

        let selected_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(self.region_x as f32 * scale, self.region_y as f32 * scale),
            egui::vec2(
                self.region_width.max(1) as f32 * scale,
                self.region_height.max(1) as f32 * scale,
            ),
        )
        .intersect(rect);
        painter.rect_filled(
            selected_rect,
            2.0,
            egui::Color32::from_rgba_unmultiplied(57, 255, 210, 36),
        );
        painter.rect_stroke(
            selected_rect,
            2.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(57, 255, 210)),
            egui::StrokeKind::Outside,
        );

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
            let color = color32(self.config.overlay.cursor_color, 235);
            painter.line_segment(
                [pos, pos + egui::vec2(16.0, 28.0)],
                egui::Stroke::new(3.0, color),
            );
            painter.line_segment(
                [pos + egui::vec2(16.0, 28.0), pos + egui::vec2(4.0, 22.0)],
                egui::Stroke::new(3.0, color),
            );
            painter.line_segment(
                [pos + egui::vec2(4.0, 22.0), pos],
                egui::Stroke::new(3.0, color),
            );
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

fn preview_to_source(rect: egui::Rect, scale: f32, pos: egui::Pos2) -> (u32, u32) {
    let local = (pos - rect.min).clamp(egui::Vec2::ZERO, rect.size());
    (
        (local.x / scale).round().max(0.0) as u32,
        (local.y / scale).round().max(0.0) as u32,
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
            preview_to_source(rect, 0.5, egui::pos2(60.0, 70.0)),
            (100, 100)
        );
        assert_eq!(
            preview_to_source(rect, 0.5, egui::pos2(-10.0, -10.0)),
            (0, 0)
        );
    }
}
