## 日本語

fvCapture は、画面録画と同時にキーボード・マウス操作をビジュアルラベルとして重ねるデスクトップキャプチャアプリです。説明動画、バグ報告、手順共有向けに MP4 / GIF / WebM を出力できます。起動時に GitHub Releases の最新版を確認し、アップデートがある場合はユーザー確認後に自動で更新できます。

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

1. `録画範囲` で全画面、モニター、ウィンドウ、範囲を選びます。ウィンドウ録画では選択中ウィンドウのプレビューも確認できます。
2. 範囲指定では `画面上で範囲選択` から実際の画面上をドラッグして録画範囲を選べます。
3. `操作ラベル` でキーボード/マウス/カーソル表示、ラベル位置、色、表示時間を調整します。
4. `プレビュー` でラベルの見た目を確認します。`画面上でプレビュー` では実際の画面上に重ねた状態を確認できます。
5. `録画開始` で録画開始、`一時停止` / `再開`、`停止` で `プレビュー` タブに録画後プレビューを表示します。
6. 録画後プレビューで MP4 / GIF / WebM、サイズ、保存先を選びます。保存先の既定値はドキュメントフォルダです。プレビューでは再生範囲の先頭/末尾ハンドルを調整し、選択範囲だけを動画ファイルとして出力できます。出力後も同じプレビューから形式や範囲を変えて再出力できます。
7. アプリ起動中は、fvCapture が非アクティブでも F9 で録画開始/停止、F10 で一時停止/再開できます。`表示` からグローバルショートカット操作時の確認音も有効にできます。
8. 起動時にアップデートが見つかった場合は、確認ダイアログから更新できます。

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
