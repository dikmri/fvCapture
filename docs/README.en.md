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
2. Toggle keyboard and mouse labels in `Overlay`.
3. Choose MP4 / GIF / WebM, FPS, size, and destination in `Output`.
4. Use `Start Recording`, `Pause` / `Resume`, and `Stop` to save the recording.

CLI:

```powershell
fv-capture --duration 3 --fps 15 --format mp4 --size p720 --output demo.mp4
fv-capture --list-sources
```

### Localization

The app UI and README support Japanese and English. `README.md` is generated from `docs/README.ja.md` and `docs/README.en.md` by `scripts/build_readme.py`, and CI checks that the generated README stays in sync.

### License

fvCapture itself is licensed under the MIT License.

Release archives bundle an FFmpeg binary to reduce setup work for users. FFmpeg is distributed under its own license, separate from fvCapture. See `THIRD_PARTY_NOTICES.md` and `third_party/ffmpeg/` in the archive for details.
