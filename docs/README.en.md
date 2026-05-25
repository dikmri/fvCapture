## English

fvCapture is a desktop screen capture app that records the screen and overlays keyboard and mouse actions as visual labels. It exports MP4, GIF, and WebM files for tutorials, bug reports, and workflow sharing.

### Download

Download the archive for your OS from GitHub Releases.

<https://github.com/dikmri/fvCapture/releases>

### Installation

1. Download the archive for your OS from Releases.
2. Extract it to any folder.
3. Launch `fvCapture` for the GUI or `fv-capture` for the CLI.
4. FFmpeg is bundled. Set `FVCAPTURE_FFMPEG` only when you want to use a different FFmpeg executable.

### Usage

GUI:

1. Choose full screen, monitor, or area from `Capture Source`.
2. For area capture, drag on the preview to select the capture region. Window capture is also available.
3. In `Overlay`, adjust keyboard, mouse, cursor, label position, colors, and display duration.
4. Use `Preview` to check the label appearance before recording.
5. Choose MP4 / GIF / WebM, FPS, size, and destination in `Output`.
6. Use `Start Recording`, `Pause` / `Resume`, and `Stop` to save the recording. F9 starts/stops recording and F10 pauses/resumes.

CLI:

```powershell
fv-capture --duration 3 --fps 15 --format mp4 --size p720 --output demo.mp4
fv-capture --list-sources
```

### Localization

The app UI and README support Japanese and English. `README.md` is generated from `docs/README.ja.md` and `docs/README.en.md` by `scripts/build_readme.py`, and CI checks that the generated README stays in sync.

### License

fvCapture itself is licensed under the MIT License.

Release archives bundle an FFmpeg binary to reduce setup work for users, and the app embeds a UI font for reliable multilingual text rendering. These third-party components are distributed under their own licenses, separate from fvCapture. See `THIRD_PARTY_NOTICES.md` and `third_party/` in the archive for details.
