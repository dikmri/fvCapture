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

This notice is informational and is not legal advice.
