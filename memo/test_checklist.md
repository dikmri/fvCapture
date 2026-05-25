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
