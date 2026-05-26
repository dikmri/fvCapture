## English

fvCapture is a desktop screen capture app that records the screen and overlays keyboard and mouse actions as visual labels. It exports MP4, GIF, and WebM files for tutorials, bug reports, and workflow sharing. On startup, it checks GitHub Releases for a newer version and can update itself after user confirmation.

### Download

The latest release archives are available from GitHub Releases.

<https://github.com/dikmri/fvCapture/releases>

To install from a command, use one of the following commands. It downloads and extracts the latest release automatically.

Windows PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/dikmri/fvCapture/main/scripts/install.ps1 | iex"
```

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/dikmri/fvCapture/main/scripts/install.sh | sh
```

### Installation

1. Command installation places the app in `%LOCALAPPDATA%\fvCapture` on Windows and `~/.local/share/fvCapture` on macOS / Linux.
2. For manual installation, download the archive for your OS from Releases and extract it to any folder.
3. Launch `fvCapture` for the GUI or `fv-capture` for the CLI.
4. FFmpeg is bundled. Set `FVCAPTURE_FFMPEG` only when you want to use a different FFmpeg executable.

### Usage

GUI:

1. Choose full screen, monitor, window, or area from `Capture Source`. Window capture shows a preview of the selected window.
2. For area capture, use `Select on screen` and drag on the actual screen to select the capture region.
3. In `Overlay`, adjust keyboard, mouse, cursor, label position, colors, and display duration.
4. Use `Preview` to check the label appearance before recording. `Preview on screen` shows the labels over the actual screen.
5. Use `Start Recording`, `Pause` / `Resume`, and `Stop` to open the post-recording preview in the `Preview` tab.
6. In the post-recording preview, choose MP4 / GIF / WebM, size, and destination. The default destination is the Documents folder. Adjust the start/end handles and export only the selected range. After exporting, you can keep using the same preview to change the format or range and export again.
7. While the app is running, F9 starts/stops recording and F10 pauses/resumes even when fvCapture is not active. You can enable a feedback sound for global shortcut actions in `Appearance`.
8. If an update is found at startup, confirm the update dialog to install it.

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
