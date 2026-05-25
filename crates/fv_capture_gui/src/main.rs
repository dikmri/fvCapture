use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;
use fv_capture_core::{
    ActiveRecording, AppConfig, CaptureBackend, CaptureSelection, CaptureSource, OutputFormat,
    OutputSize, RecordingRequest, RecordingSummary, XcapCaptureBackend,
};

mod i18n;

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
        Box::new(|_cc| Ok(Box::new(FvCaptureApp::new()))),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceMode {
    Primary,
    Monitor,
    Region,
}

struct FvCaptureApp {
    config: AppConfig,
    sources: Vec<CaptureSource>,
    source_mode: SourceMode,
    selected_monitor_id: Option<u32>,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    output_path: String,
    active: Option<ActiveRecording>,
    last_summary: Option<RecordingSummary>,
    status: StatusKey,
    error: Option<String>,
    language: LanguageChoice,
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
            source_mode: SourceMode::Primary,
            selected_monitor_id: None,
            region_x: 0,
            region_y: 0,
            region_width: 1280,
            region_height: 720,
            output_path: default_output_path(OutputFormat::Mp4).display().to_string(),
            active: None,
            last_summary: None,
            status: StatusKey::Ready,
            error: None,
            language: LanguageChoice::System,
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
    }

    fn start_recording(&mut self) {
        if self.active.is_some() {
            return;
        }

        let output_path = PathBuf::from(self.output_path.trim());
        let selection = match self.source_mode {
            SourceMode::Primary => CaptureSelection::PrimaryMonitor,
            SourceMode::Monitor => self
                .selected_monitor_id
                .map(|id| CaptureSelection::Monitor { id })
                .unwrap_or(CaptureSelection::PrimaryMonitor),
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
        match active.stop() {
            Ok(summary) => {
                self.status = StatusKey::Saved;
                self.last_summary = Some(summary);
                self.error = None;
            }
            Err(error) => {
                self.status = StatusKey::Ready;
                self.error = Some(error.to_string());
            }
        }
    }

    fn update_output_extension(&mut self) {
        let path = PathBuf::from(self.output_path.trim());
        let extension = self.config.encoder.format.extension();
        let updated = path.with_extension(extension);
        self.output_path = updated.display().to_string();
    }
}

impl eframe::App for FvCaptureApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("fvCapture");
            ui.label(self.tr(Text::Intro));
            ui.separator();

            self.language_ui(ui);

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

            if let Some(error) = &self.error {
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
            self.action_ui(ui);
        });
    }
}

impl FvCaptureApp {
    fn source_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr(Text::CaptureSource));
        let full_screen = self.tr(Text::FullScreen);
        let monitor = self.tr(Text::Monitor);
        let select_area = self.tr(Text::SelectArea);
        let refresh = self.tr(Text::Refresh);
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.source_mode, SourceMode::Primary, full_screen);
            ui.radio_value(&mut self.source_mode, SourceMode::Monitor, monitor);
            ui.radio_value(&mut self.source_mode, SourceMode::Region, select_area);
            if ui.button(refresh).clicked() {
                self.refresh_sources();
            }
        });

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

        if self.source_mode == SourceMode::Region {
            egui::Grid::new("region_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("X");
                    ui.add(egui::DragValue::new(&mut self.region_x).speed(1));
                    ui.end_row();
                    ui.label("Y");
                    ui.add(egui::DragValue::new(&mut self.region_y).speed(1));
                    ui.end_row();
                    ui.label(self.tr(Text::Width));
                    ui.add(
                        egui::DragValue::new(&mut self.region_width)
                            .range(1..=16_384)
                            .speed(2),
                    );
                    ui.end_row();
                    ui.label(self.tr(Text::Height));
                    ui.add(
                        egui::DragValue::new(&mut self.region_height)
                            .range(1..=16_384)
                            .speed(2),
                    );
                    ui.end_row();
                });
        }
    }

    fn overlay_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr(Text::Overlay));
        let show_keyboard = self.tr(Text::ShowKeyboardLabels);
        let show_mouse = self.tr(Text::ShowMouseLabels);
        let label_size = self.tr(Text::LabelSize);
        let opacity = self.tr(Text::Opacity);
        ui.checkbox(&mut self.config.overlay.show_keyboard, show_keyboard);
        ui.checkbox(&mut self.config.overlay.show_mouse, show_mouse);
        ui.add(
            egui::Slider::new(&mut self.config.overlay.label_scale, 0.75..=2.0).text(label_size),
        );
        ui.add(egui::Slider::new(&mut self.config.overlay.opacity, 0.2..=1.0).text(opacity));
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
            if ui
                .add_enabled(!recording, egui::Button::new(self.tr(Text::StartRecording)))
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
                if ui.button(label).clicked() {
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
                .add_enabled(recording, egui::Button::new(self.tr(Text::Stop)))
                .clicked()
            {
                self.stop_recording();
            }
        });
    }

    fn selected_monitor_label(&self) -> String {
        self.sources
            .iter()
            .find(|source| Some(source.id) == self.selected_monitor_id)
            .map(|source| source.name.clone())
            .unwrap_or_else(|| self.tr(Text::PrimaryMonitor).to_string())
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
}

impl Tr for FvCaptureApp {
    fn language_choice(&self) -> LanguageChoice {
        self.language
    }
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
