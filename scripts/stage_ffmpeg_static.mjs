import { copyFileSync, chmodSync, existsSync, mkdirSync } from "node:fs";
import { basename, join } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const destination = process.argv[2];

if (!destination) {
  console.error("usage: node scripts/stage_ffmpeg_static.mjs <destination-dir>");
  process.exit(2);
}

const ffmpegPath = require(`${process.cwd()}/.ffmpeg/node_modules/ffmpeg-static`);
if (!ffmpegPath || !existsSync(ffmpegPath)) {
  console.error("ffmpeg-static did not provide a binary path");
  process.exit(1);
}

mkdirSync(destination, { recursive: true });

const binaryName = process.platform === "win32" ? "ffmpeg.exe" : "ffmpeg";
const binaryDestination = join(destination, binaryName);
copyFileSync(ffmpegPath, binaryDestination);
chmodSync(binaryDestination, 0o755);

for (const suffix of [".README", ".LICENSE"]) {
  const source = `${ffmpegPath}${suffix}`;
  if (existsSync(source)) {
    const targetName = suffix === ".README" ? "README.txt" : "LICENSE.txt";
    copyFileSync(source, join(destination, targetName));
  }
}

console.log(`staged ${basename(ffmpegPath)} to ${destination}`);
