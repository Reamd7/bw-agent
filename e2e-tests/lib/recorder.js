import { spawn } from 'child_process';
import fs from 'fs';
import path from 'path';

let ffmpegProcess = null;

/**
 * Start screen recording via ffmpeg gdigrab (Windows).
 * Saves to outputPath as mp4.
 */
export function startRecording(outputPath) {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });

  ffmpegProcess = spawn('ffmpeg', [
    '-y',
    '-f', 'gdigrab',
    '-framerate', '15',
    '-i', 'desktop',
    '-vf', 'scale=1920:1080',
    '-c:v', 'libx264',
    '-preset', 'ultrafast',
    '-crf', '28',
    '-loglevel', 'error',
    outputPath,
  ], {
    stdio: ['pipe', 'ignore', 'ignore'],
  });

  ffmpegProcess.on('error', (err) => {
    console.error('[recorder] ffmpeg spawn error:', err.message);
  });
}

/**
 * Stop recording gracefully (send 'q' to ffmpeg stdin).
 * Returns a promise that resolves when ffmpeg has exited and flushed the file.
 */
export function stopRecording() {
  if (!ffmpegProcess) return Promise.resolve();

  const proc = ffmpegProcess;
  ffmpegProcess = null;

  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      proc.kill();
      resolve();
    }, 5000);

    proc.on('exit', () => {
      clearTimeout(timeout);
      resolve();
    });

    proc.stdin.write('q');
  });
}

/**
 * Take a screenshot via WebDriver and save to outputPath as PNG.
 */
export async function takeScreenshot(browser, outputPath) {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  const base64 = await browser.takeScreenshot();
  fs.writeFileSync(outputPath, Buffer.from(base64, 'base64'));
}

/**
 * Capture web console logs (JS console.log/warn/error) via WebDriver.
 * Saves as JSON array to outputPath.
 * Returns the captured log entries.
 */
export async function captureWebConsole(browser, outputPath) {
  let entries = [];
  try {
    const types = await browser.getLogTypes();
    if (types.includes('browser')) {
      entries = await browser.getLogs('browser');
    }
  } catch {
    // Some WebDriver sessions don't support getLogs
  }

  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  const data = entries.map((e) => ({
    level: e.level,
    time: e.timestamp,
    message: e.message,
    source: e.source || '',
  }));
  fs.writeFileSync(outputPath, JSON.stringify(data, null, 2));
  return data;
}

/**
 * Read the desktop (Tauri/Rust) log file written by env_logger.
 * Returns the content as string. Returns '' if file doesn't exist.
 */
export function readDesktopLog(logFilePath) {
  try {
    return fs.readFileSync(logFilePath, 'utf-8');
  } catch {
    return '';
  }
}

/**
 * Write desktop log content (or summary) to an output file.
 */
export function saveDesktopLog(logFilePath, outputPath) {
  const content = readDesktopLog(logFilePath);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, content);
  return content;
}

/**
 * Build a filesystem-safe name from a test title.
 */
export function safeName(title) {
  return title.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/(^_|_$)/g, '');
}
