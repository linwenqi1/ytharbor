# Third-party components

YtHarbor invokes, but does not modify, the following separately distributed executables:

- [yt-dlp](https://github.com/yt-dlp/yt-dlp): refer to the license shipped by the selected release. Some standalone builds include GPL-licensed components.
- [FFmpeg](https://ffmpeg.org/): LGPL or GPL depending on the chosen build configuration.
- [Deno](https://github.com/denoland/deno): MIT license. Used as the JavaScript runtime recommended by yt-dlp for YouTube extraction.

Before distributing an installer, include the exact license texts and source-code notices required by the binaries prepared for that release. The Windows preparation script currently downloads the latest upstream builds; production releases should pin versions and record checksums.
