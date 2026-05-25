# fvCapture

fvCapture は、画面録画と同時にキーボード・マウス操作をビジュアルラベルとして重ねるデスクトップキャプチャアプリです。説明動画、バグ報告、手順共有向けに MP4 / GIF / WebM を出力できます。

## ダウンロード

GitHub Releases から各OS向けのアーカイブをダウンロードしてください。

<https://github.com/dikmri/fvCapture/releases>

## インストール

1. Releases から自分のOS向けアーカイブをダウンロードします。
2. 任意のフォルダに展開します。
3. GUI は `fvCapture`、CLI は `fv-capture` を起動します。
4. エンコードには FFmpeg が必要です。`ffmpeg` にPATHを通すか、`FVCAPTURE_FFMPEG` に実行ファイルのパスを指定してください。

## 操作方法

GUI:

1. `Capture Source` で全画面、モニター、範囲を選びます。
2. `Overlay` でキーボード/マウス表示を切り替えます。
3. `Output` で MP4 / GIF / WebM、FPS、サイズ、保存先を選びます。
4. `Start Recording` で録画開始、`Pause` / `Resume`、`Stop` で保存します。

CLI:

```powershell
fv-capture --duration 3 --fps 15 --format mp4 --size p720 --output demo.mp4
fv-capture --list-sources
```

## ライセンス

MIT License
