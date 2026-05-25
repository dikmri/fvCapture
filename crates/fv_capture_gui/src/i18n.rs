#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageChoice {
    System,
    English,
    Japanese,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    English,
    Japanese,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    Intro,
    Language,
    SystemLanguage,
    English,
    Japanese,
    Status,
    Ready,
    Recording,
    Paused,
    Encoding,
    Saved,
    Frames,
    CaptureSource,
    FullScreen,
    Monitor,
    SelectArea,
    Refresh,
    PrimaryMonitor,
    Width,
    Height,
    Overlay,
    ShowKeyboardLabels,
    ShowMouseLabels,
    LabelSize,
    Opacity,
    Output,
    Format,
    Fps,
    Size,
    Original,
    Browse,
    StartRecording,
    Pause,
    Resume,
    Stop,
}

pub trait Tr {
    fn language_choice(&self) -> LanguageChoice;

    fn tr(&self, text: Text) -> &'static str {
        translate(effective_language(self.language_choice()), text)
    }
}

fn effective_language(choice: LanguageChoice) -> Language {
    match choice {
        LanguageChoice::English => Language::English,
        LanguageChoice::Japanese => Language::Japanese,
        LanguageChoice::System => sys_locale::get_locale()
            .map(|locale| {
                if locale.to_ascii_lowercase().starts_with("ja") {
                    Language::Japanese
                } else {
                    Language::English
                }
            })
            .unwrap_or(Language::English),
    }
}

fn translate(language: Language, text: Text) -> &'static str {
    match language {
        Language::English => match text {
            Text::Intro => "Record screen actions with keyboard and mouse overlays.",
            Text::Language => "Language",
            Text::SystemLanguage => "System",
            Text::English => "English",
            Text::Japanese => "Japanese",
            Text::Status => "Status",
            Text::Ready => "Ready",
            Text::Recording => "Recording",
            Text::Paused => "Paused",
            Text::Encoding => "Encoding",
            Text::Saved => "Saved",
            Text::Frames => "frames",
            Text::CaptureSource => "Capture Source",
            Text::FullScreen => "Full Screen",
            Text::Monitor => "Monitor",
            Text::SelectArea => "Select Area",
            Text::Refresh => "Refresh",
            Text::PrimaryMonitor => "Primary monitor",
            Text::Width => "Width",
            Text::Height => "Height",
            Text::Overlay => "Overlay",
            Text::ShowKeyboardLabels => "Show keyboard labels",
            Text::ShowMouseLabels => "Show mouse labels",
            Text::LabelSize => "Label size",
            Text::Opacity => "Opacity",
            Text::Output => "Output",
            Text::Format => "Format",
            Text::Fps => "FPS",
            Text::Size => "Size",
            Text::Original => "Original",
            Text::Browse => "Browse",
            Text::StartRecording => "Start Recording",
            Text::Pause => "Pause",
            Text::Resume => "Resume",
            Text::Stop => "Stop",
        },
        Language::Japanese => match text {
            Text::Intro => "画面操作をキーボード・マウス表示付きで録画します。",
            Text::Language => "言語",
            Text::SystemLanguage => "システム",
            Text::English => "英語",
            Text::Japanese => "日本語",
            Text::Status => "状態",
            Text::Ready => "待機中",
            Text::Recording => "録画中",
            Text::Paused => "一時停止中",
            Text::Encoding => "エンコード中",
            Text::Saved => "保存済み",
            Text::Frames => "フレーム",
            Text::CaptureSource => "録画範囲",
            Text::FullScreen => "全画面",
            Text::Monitor => "モニター",
            Text::SelectArea => "範囲指定",
            Text::Refresh => "更新",
            Text::PrimaryMonitor => "プライマリモニター",
            Text::Width => "幅",
            Text::Height => "高さ",
            Text::Overlay => "操作ラベル",
            Text::ShowKeyboardLabels => "キーボード表示",
            Text::ShowMouseLabels => "マウス表示",
            Text::LabelSize => "ラベルサイズ",
            Text::Opacity => "透明度",
            Text::Output => "出力",
            Text::Format => "形式",
            Text::Fps => "FPS",
            Text::Size => "サイズ",
            Text::Original => "元サイズ",
            Text::Browse => "参照",
            Text::StartRecording => "録画開始",
            Text::Pause => "一時停止",
            Text::Resume => "再開",
            Text::Stop => "停止",
        },
    }
}
