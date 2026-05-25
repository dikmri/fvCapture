# fvCapture テストリスト

作業中に更新するテストリスト。

- [x] `cargo fmt --all -- --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --release`
- [x] `python scripts/build_readme.py --check`
- [x] CLI で短時間 MP4 録画を実行する
- [x] CLI で短時間 GIF 録画を実行する
- [x] CLI で短時間 WebM 録画を実行する
- [x] PATH を空にして同梱 FFmpeg による短時間 MP4 録画を実行する
- [x] 生成ファイルを `ffprobe` で検査する
- [x] v0.2.0 リリースビルド後に PATH なしの同梱 FFmpeg 録画を再検証する
- [x] 生成 MP4 を同梱 `ffmpeg` でデコード検査する
- [x] 日本語フォント登録の単体テストを実行する
- [x] Windows release GUI 実行ファイルの PE Subsystem が Windows GUI であることを検査する
- [x] GUI を起動して日本語UIが四角表示にならないことを確認する
- [x] ローカル release 相当アーカイブに FFmpeg とフォントライセンスが含まれることを確認する
- [x] v0.2.1 GitHub Actions release を実行し、Windows/macOS/Linux成果物を確認する
- [x] v0.2.1 Windows リリースアーカイブを展開し、GUI subsystem と同梱物を確認する
- [x] `cargo fmt --all -- --check`
- [x] `python scripts/build_readme.py --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --release`
- [x] GUIを起動してウィンドウ選択、操作ラベルプレビュー、フォント設定が描画されることを確認する
- [x] PATHなしの同梱FFmpeg録画を再検証する
- [x] v0.3.0 GitHub Actions release を実行し、Windows/macOS/Linux成果物と日本語リリースノートを確認する
- [x] v0.3.0 Windows リリースアーカイブを展開し、GUI subsystem と同梱物を確認する
- [x] `python scripts/build_readme.py --check`
- [x] Windows コマンドインストールスクリプトを一時フォルダに実行し、GUI/CLI/同梱FFmpeg/shimの展開を確認する
- [x] `bash -n scripts/install.sh`
