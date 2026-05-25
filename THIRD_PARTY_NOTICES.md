# Third Party Notices

## FFmpeg

fvCapture release archives bundle an FFmpeg executable for encoding MP4, GIF, and WebM files.

FFmpeg is a separate program from fvCapture. fvCapture is licensed under MIT; the bundled FFmpeg executable is licensed under the LGPL/GPL terms that apply to the specific FFmpeg build.

The release workflow currently stages FFmpeg via `ffmpeg-static@5.3.0`, which downloads platform-specific FFmpeg binaries. The binary package and the downloaded FFmpeg binary include their own README and LICENSE files under `third_party/ffmpeg/` in each release archive.

Relevant source and license references:

- FFmpeg: <https://ffmpeg.org/>
- FFmpeg legal information: <https://ffmpeg.org/legal.html>
- FFmpeg source downloads: <https://ffmpeg.org/download.html>
- ffmpeg-static: <https://github.com/eugeneware/ffmpeg-static>
- ffmpeg-static binary release used by 5.3.0: <https://github.com/eugeneware/ffmpeg-static/releases/tag/b6.1.1>

## M PLUS 1

fvCapture embeds the M PLUS 1 font so the GUI can render Japanese and English text consistently across platforms.

M PLUS 1 is licensed under the SIL Open Font License, Version 1.1. The license text is stored in this repository at `assets/fonts/MPLUS1-OFL.txt` and is included in release archives under `third_party/fonts/MPLUS1-OFL.txt`.

Relevant source and license references:

- Google Fonts source: <https://github.com/google/fonts/tree/main/ofl/mplus1>
- M PLUS fonts project: <https://github.com/coz-m/MPLUS_FONTS>

This notice is informational and is not legal advice.
