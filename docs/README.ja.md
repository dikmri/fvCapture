## 日本語

fvCapture は、画面録画と同時にキーボード・マウス操作をビジュアルラベルとして重ねるデスクトップキャプチャアプリです。説明動画、バグ報告、手順共有向けに MP4 / GIF / WebM を出力できます。

### ダウンロード

最新版は GitHub Releases から各OS向けのアーカイブをダウンロードできます。

<https://github.com/dikmri/fvCapture/releases>

コマンドだけで入手する場合は、次のコマンドで最新リリースをダウンロードして展開できます。

Windows PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/dikmri/fvCapture/main/scripts/install.ps1 | iex"
```

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/dikmri/fvCapture/main/scripts/install.sh | sh
```

### インストール

1. コマンドインストールの場合、Windows は `%LOCALAPPDATA%\fvCapture`、macOS / Linux は `~/.local/share/fvCapture` に配置されます。
2. 手動インストールの場合、Releases から自分のOS向けアーカイブをダウンロードし、任意のフォルダに展開します。
3. GUI は `fvCapture`、CLI は `fv-capture` を起動します。
4. FFmpeg は同梱されています。別のFFmpegを使いたい場合だけ、`FVCAPTURE_FFMPEG` に実行ファイルのパスを指定してください。

### 操作方法

GUI:

1. `録画範囲` で全画面、モニター、範囲を選びます。
2. 範囲指定ではプレビュー上をドラッグして録画範囲を選べます。ウィンドウ単位の録画も選択できます。
3. `操作ラベル` でキーボード/マウス/カーソル表示、ラベル位置、色、表示時間を調整します。
4. `プレビュー` でラベルの見た目を確認します。
5. `出力` で MP4 / GIF / WebM、FPS、サイズ、保存先を選びます。
6. `録画開始` で録画開始、`一時停止` / `再開`、`停止` で保存します。F9で録画開始/停止、F10で一時停止/再開できます。

CLI:

```powershell
fv-capture --duration 3 --fps 15 --format mp4 --size p720 --output demo.mp4
fv-capture --list-sources
```

### 多言語対応

アプリUIとREADMEは日本語/英語に対応しています。READMEは `docs/README.ja.md` と `docs/README.en.md` から `scripts/build_readme.py` で生成し、CIで同期漏れを検出します。

### ライセンス

fvCapture 本体は MIT License です。

リリースアーカイブにはユーザーの負担を減らすため FFmpeg バイナリを同梱し、GUI表示用フォントをアプリに埋め込んでいます。これらは fvCapture とは別ライセンスで配布されます。詳細は `THIRD_PARTY_NOTICES.md` と、アーカイブ内の `third_party/` を確認してください。
