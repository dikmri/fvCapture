# fvCapture 仕様書

## 1. アプリ概要

**アプリ名:** fvCapture  
**種別:** OSS・クロスプラットフォーム対応予定のデスクトップ画面キャプチャアプリ  
**主目的:** 画面録画と同時に、キーボード・マウス操作を言語非依存のビジュアルラベルとして自動合成し、説明用の MP4 / GIF / WebM をすぐ出力できるようにする。

fvCapture は、単なる画面録画アプリではなく、**操作説明動画を自動生成するキャプチャツール**である。

既存の録画・GIF作成ツールは、録画後にユーザーが編集したり、別ツールで入力表示を重ねたり、別エンコードツールで変換したりする必要がある。fvCapture は、以下の流れを 1 アプリで完結させる。

```text
起動 → 範囲選択 → 録画 → 操作ラベル自動合成 → プレビュー → MP4/GIF/WebM出力
```

## 2. 開発方針

### 2.1 最重要コンセプト

**「録るだけで、勝手に操作ラベルが乗る」** を最重要コンセプトとする。

操作ラベルは日本語や英語の文章ではなく、できるだけ言語非依存の視覚表現にする。

例:

```text
[ Ctrl ] + [ S ]
[ Enter ]
[ Space ]
[ Esc ]
[ ↑ ] [ ↓ ] [ ← ] [ → ]
```

マウス操作も文字説明中心ではなく、マウスアイコンのボタン点灯やホイール回転、ドラッグ軌跡などで表現する。

### 2.2 競合との差別化

fvCapture は以下を目指す。

- ScreenToGif のような「録画して編集するツール」ではなく、**説明用動画を自動生成するツール**にする。
- Keyviz / KeyCastr / Carnac のような「入力可視化ツール」ではなく、**録画・入力可視化・エンコードまで一体化したツール**にする。
- OBS + Input Overlay のような高機能構成ではなく、**起動してすぐ録れる軽量ツール**にする。

## 3. 技術選定

### 3.1 結論

初期実装は **Rust + egui/eframe** を推奨する。

理由:

- UIが凝っている必要がない。
- 録画中はタスクトレイや小型フローティングパネルに引っ込んでいてほしい。
- WebViewを持つ Tauri より、ネイティブRustのみで完結する egui/eframe の方が構成が単純。
- 録画・入力監視・エンコードなど重い処理はRust側が中心になるため、HTML/CSS UIのメリットが小さい。
- ユーザー体験としては「設定画面」より「録画体験」が重要。

### 3.2 Tauri を採用しない理由

Tauri は優秀であり、Svelte/TypeScript でUIを書けるメリットがある。Tauri はシステムWebViewを使うためElectronより軽量で、トレイ機能も持つ。

ただし fvCapture では、以下の理由から初期MVPでは egui を優先する。

- UIは簡素でよく、HTML/CSSによる表現力をあまり必要としない。
- Rust側のキャプチャ・入力・エンコード処理が主体になる。
- WebView、フロントエンド、Rustバックエンドのブリッジを挟むより、Rust単体構成の方が見通しがよい。
- 小型ユーティリティとしての軽快さを優先する。

### 3.3 将来の逃げ道

UIフレームワークに依存しないよう、コア処理は必ず `fv_capture_core` として分離する。

```text
crates/
  fv_capture_core/      # 録画・入力イベント・合成・エンコードの中核
  fv_capture_gui/       # egui/eframe UI
  fv_capture_cli/       # 将来のCLI/テスト用
```

これにより、将来的に Tauri / Slint / iced に乗り換える場合でも、コア処理を再利用できる。

## 4. 対象プラットフォーム

### 4.1 MVP

- Windows 10 / 11

### 4.2 将来対応

- macOS
- Linux X11
- Linux Wayland

### 4.3 注意点

macOS では以下の権限が必要になる可能性が高い。

- Screen Recording 権限
- Accessibility 権限

Linux Wayland はグローバル入力取得や画面キャプチャに制約があるため、X11より対応難度が高い。MVPでは Windows を優先し、Linux/macOS は抽象化レイヤーだけ先に設計しておく。

## 5. 採用予定ライブラリ候補

### 5.1 GUI

- `eframe`
- `egui`

### 5.2 トレイアイコン

候補:

- `tray-icon`
- `tao` / `winit` 系との組み合わせ

要件:

- 録画中はメインウィンドウを隠せること。
- トレイメニューから録画開始、停止、設定、終了を選べること。

### 5.3 画面キャプチャ

候補:

- `xcap`
- `scap`
- OS別ネイティブAPIを直接利用する独自実装

抽象インターフェースを必ず用意する。

```rust
pub trait CaptureBackend {
    fn list_sources(&self) -> Result<Vec<CaptureSource>>;
    fn start_capture(&mut self, config: CaptureConfig) -> Result<()>;
    fn next_frame(&mut self) -> Result<CapturedFrame>;
    fn stop_capture(&mut self) -> Result<()>;
}
```

### 5.4 グローバル入力監視

候補:

- `rdev`
- `raw-input`
- OS別ネイティブAPI

注意:

グローバル入力監視はOSごとの制約が大きい。特にmacOSとLinux Waylandは慎重に扱う。

抽象インターフェースを用意する。

```rust
pub trait InputBackend {
    fn start_listening(&mut self, sender: InputEventSender) -> Result<()>;
    fn stop_listening(&mut self) -> Result<()>;
}
```

### 5.5 動画エンコード

MVPでは以下の方針とする。

- 内部録画はフレーム列または一時動画として保持する。
- 最終出力は FFmpeg を利用する。
- Windows版では FFmpeg を同梱、または初回起動時に案内する。
- 将来的にネイティブエンコーダやWebM直接出力も検討する。

出力形式:

- MP4: H.264 + AACなし、または無音MP4
- GIF: 短尺・低FPS・減色あり
- WebM: VP9 または VP8

## 6. 主要機能

### 6.1 キャプチャ範囲選択

以下をサポートする。

- 全画面
- モニター単位
- ウィンドウ単位
- 範囲指定

MVPでは以下を優先する。

1. 全画面
2. モニター単位
3. 範囲指定
4. ウィンドウ単位

### 6.2 録画操作

- 録画開始
- 一時停止
- 再開
- 停止
- 録画キャンセル

録画中は小型のフローティングパネルを表示する。

```text
● REC 00:12   [Pause] [Stop]
```

設定でフローティングパネルを録画に含める/含めないを選択可能にする。

### 6.3 操作ラベル自動合成

録画中の入力イベントを収集し、動画に合成する。

#### キーボード表示

例:

```text
[ Ctrl ] + [ S ]
[ Shift ] + [ F4 ]
[ Enter ]
[ Space ]
```

要件:

- 文章ではなくキーキャップ風UIで表示する。
- 修飾キーは同時押しとしてまとめる。
- 短時間に連続入力された文字はまとめて表示する。
- パスワード入力らしき場面では文字内容を表示しない。

#### マウス表示

表示対象:

- 左クリック
- 右クリック
- ダブルクリック
- ホイールクリック
- スクロール上/下
- ドラッグ開始/移動/終了

表現:

- クリック位置にリングエフェクト
- マウスアイコンのボタン点灯
- ドラッグ時は軌跡表示
- スクロール時はホイール回転アイコンまたは上下矢印

### 6.4 操作ラベルの配置

初期設定:

- 画面下中央にキーボード操作ラベル
- クリック位置にマウスエフェクト
- 画面右下にマウス操作アイコン

設定項目:

- ラベルサイズ
- ラベル位置
- 表示時間
- 透明度
- テーマ
- キーボード表示ON/OFF
- マウス表示ON/OFF

### 6.5 録画後プレビュー

録画停止後、自動でプレビュー画面を表示する。

機能:

- 再生
- 一時停止
- 先頭へ戻る
- 出力形式選択
- 保存
- 破棄

MVPでは高度なタイムライン編集は不要。

### 6.6 軽量編集

MVPで必要な編集:

- 先頭/末尾のトリミング
- 操作ラベルのON/OFF
- 出力範囲の再指定
- GIF出力時のFPS指定
- 出力解像度指定

MVPでは不要な編集:

- フレーム単位編集
- テキスト字幕の手動追加
- 複雑なエフェクト
- BGM/音声編集
- トランジション

## 7. 出力仕様

### 7.1 MP4

用途:

- Slack / Teams / Discord / メール共有
- 手順説明
- バグ報告

推奨設定:

```text
Codec: H.264
FPS: 30 or 15
Audio: none
Preset: fast
CRF: 23〜28
```

### 7.2 GIF

用途:

- GitHub Issue
- Redmine
- README
- 短尺説明

推奨設定:

```text
FPS: 10〜15
Max width: 720px
Palette generation: enabled
Loop: infinite
```

長尺・高解像度GIFはファイルサイズが大きくなるため、出力時に警告を表示する。

### 7.3 WebM

用途:

- Web共有
- 軽量動画

推奨設定:

```text
Codec: VP9 or VP8
FPS: 30 or 15
Audio: none
```

## 8. UI仕様

### 8.1 メイン画面

```text
fvCapture

Capture Source
[ Full Screen      v ]
[ Select Area ]

Overlay
[✓] Show keyboard labels
[✓] Show mouse labels
Style: [ Minimal Keycaps v ]

Output
Format: [ MP4 v ]
FPS:    [ 30 v ]
Size:   [ Original v ]

[ Start Recording ]
```

### 8.2 録画中パネル

```text
● REC 00:12
[ Pause ] [ Stop ]
```

要件:

- 常に最前面表示可能。
- 録画に含めない設定を用意する。
- 邪魔にならない小型UIにする。

### 8.3 トレイメニュー

```text
fvCapture
----------------
Start Recording
Stop Recording
Pause Recording
Open Window
Settings
Quit
```

### 8.4 プレビュー画面

```text
Preview
[ video preview ]

Trim
Start: 00:00.0
End:   00:07.5

Output
Format: MP4 / GIF / WebM
Size: Original / 720p / 480p
FPS: 30 / 15 / 10

[ Save ] [ Discard ]
```

## 9. 内部アーキテクチャ

### 9.1 モジュール構成

```text
src/
  main.rs
  app.rs
  ui/
    main_window.rs
    recording_panel.rs
    preview_window.rs
    settings_window.rs
  core/
    capture/
      mod.rs
      capture_backend.rs
      windows_backend.rs
      macos_backend.rs
      linux_backend.rs
    input/
      mod.rs
      input_backend.rs
      input_event.rs
      key_normalizer.rs
    overlay/
      mod.rs
      keycap_renderer.rs
      mouse_renderer.rs
      overlay_timeline.rs
    encoder/
      mod.rs
      ffmpeg_encoder.rs
      gif_encoder.rs
    project/
      recording_session.rs
      temp_storage.rs
  config/
    app_config.rs
```

### 9.2 データフロー

```text
CaptureBackend → CapturedFrame
InputBackend   → InputEvent

CapturedFrame + InputEventTimeline
        ↓
OverlayRenderer
        ↓
CompositedFrame
        ↓
Encoder
        ↓
MP4 / GIF / WebM
```

### 9.3 イベントモデル

```rust
pub enum InputEventKind {
    KeyDown(KeyCode),
    KeyUp(KeyCode),
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    MouseMove { x: f64, y: f64 },
    MouseWheel { delta_x: f64, delta_y: f64 },
}

pub struct InputEvent {
    pub timestamp_ms: u64,
    pub kind: InputEventKind,
}
```

### 9.4 合成用イベント

生の入力イベントから、表示用イベントへ変換する。

```rust
pub enum OverlayEventKind {
    KeyCombo(Vec<KeyCode>),
    MouseClick { button: MouseButton, x: f64, y: f64 },
    MouseDoubleClick { button: MouseButton, x: f64, y: f64 },
    MouseDrag { start: Point, end: Point },
    MouseWheel { direction: WheelDirection },
}

pub struct OverlayEvent {
    pub start_ms: u64,
    pub duration_ms: u64,
    pub kind: OverlayEventKind,
}
```

## 10. セキュリティ・プライバシー要件

fvCaptureはグローバル入力を扱うため、プライバシー面の説明を明確にする。

必須要件:

- 録画中以外は入力監視しない。
- 入力ログを永続保存しない。
- 保存されるのは合成済み動画と、必要に応じた一時プロジェクトのみ。
- パスワード欄や機密入力の可能性がある場合、入力文字は表示しない。
- 設定でキーボード表示を完全OFFにできる。
- 設定でマウス表示を完全OFFにできる。
- ネットワーク送信は一切行わない。

## 11. MVPスコープ

### 11.1 MVPに含める

- Windows対応
- egui/eframe UI
- トレイ常駐
- 全画面録画
- 範囲指定録画
- グローバルキーボード入力取得
- グローバルマウスクリック取得
- キーキャップ風オーバーレイ
- マウスクリックリングエフェクト
- MP4出力
- GIF出力
- WebM出力
- 録画後プレビュー
- 先頭/末尾トリミング

### 11.2 MVPに含めない

- macOS/Linux完全対応
- 音声録音
- Webカメラ合成
- フレーム単位編集
- 手動字幕エディタ
- クラウドアップロード
- OCR
- AIによる手順文章生成

## 12. 将来機能

- macOS対応
- Linux X11対応
- Linux Wayland対応
- ウィンドウ単位キャプチャ
- ショートカット表示テーマ追加
- マウスアイコンテーマ追加
- 手順テキスト自動生成
- GitHub Issue用最適化プリセット
- Redmine添付用最適化プリセット
- Slack/Teams共有用MP4プリセット
- 個人情報自動マスク
- 操作ログから再現手順Markdown生成

## 13. 開発ルール

### 13.1 実装ルール

- Rustのコア処理とUI処理を分離する。
- OS依存処理は必ずtraitで抽象化する。
- UIから直接OS APIを呼ばない。
- 入力イベント、録画フレーム、合成フレーム、出力処理を明確に分離する。
- エラーは `anyhow` または独自エラー型で扱う。
- ログ出力を入れる。
- MVPではシンプルさを優先し、過剰な抽象化は避ける。

### 13.2 テスト方針

- 入力イベント正規化のユニットテスト
- キーコンボ判定のユニットテスト
- オーバーレイイベント生成のユニットテスト
- エンコードコマンド生成のテスト
- 設定ファイル読み書きのテスト

### 13.3 デバッグログ

録画処理はトラブルが起こりやすいため、以下をログに出す。

- アプリ起動
- キャプチャソース取得
- 録画開始
- 録画停止
- 入力監視開始
- 入力監視停止
- エンコード開始
- エンコード終了
- エラー詳細

## 14. Codexへの実装指示

以下の順番で実装する。

### Phase 1: プロジェクト雛形

- Rust workspaceを作成する。
- `fv_capture_core` と `fv_capture_gui` を分ける。
- egui/eframeでメイン画面を表示する。
- 設定画面のUIだけ作る。
- トレイアイコンは後回しでよい。

### Phase 2: 録画MVP

- Windowsで画面キャプチャを取得する。
- 全画面録画を実装する。
- 一時フレーム保存、またはパイプ経由でFFmpegに渡す。
- MP4出力を実装する。

### Phase 3: 入力監視

- Windowsでグローバルキーボードイベントを取得する。
- Windowsでグローバルマウスクリックイベントを取得する。
- 入力イベントをタイムスタンプ付きで記録する。

### Phase 4: オーバーレイ合成

- キーキャップ風ラベルを描画する。
- クリック位置にリングエフェクトを描画する。
- 入力イベントタイムラインを動画フレームに合成する。

### Phase 5: プレビューとGIF出力

- 録画後プレビューを実装する。
- 先頭/末尾トリミングを実装する。
- GIF出力を実装する。

### Phase 6: トレイ常駐

- トレイアイコンを実装する。
- トレイから録画開始/停止できるようにする。
- メインウィンドウを閉じても常駐できるようにする。

## 15. 重要な非目標

fvCaptureは動画編集ソフトではない。

以下を目指さない。

- Premiereのような編集機能
- ScreenToGifの完全代替
- OBSのような配信機能
- 高度なエフェクト編集

fvCaptureは、**操作が見える説明動画を最短で作るためのツール**である。

## 16. 参考情報

- Tauri: system tray support and small system-webview-based desktop apps
- egui: simple, fast, portable immediate mode GUI for Rust
- eframe: official egui framework for native/web apps
- xcap: Rust cross-platform screen capture library
- global-hotkey: cross-platform global hotkeys for desktop apps, Linux X11 limitation
- rdev/raw-input系: global keyboard/mouse input listening候補
