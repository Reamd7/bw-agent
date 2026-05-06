import os from 'os';
import path from 'path';
import { spawn, spawnSync } from 'child_process';
import fs from 'fs';
import { fileURLToPath } from 'url';
import {
  startRecording, stopRecording,
  takeScreenshot, captureWebConsole,
  saveDesktopLog, safeName,
} from './lib/recorder.js';
import { getCredential } from './lib/credentials.js';

const __dirname = fileURLToPath(new URL('.', import.meta.url));
const resultsRoot = path.resolve(__dirname, 'results');

// keep track of the `tauri-driver` child process
let tauriDriver;
let exit = false;
// Fixed path where the Tauri app writes its Rust-side log.
// In debug builds, the app writes to %TEMP%/bw-agent-e2e.log and
// publishes the path in %TEMP%/bw-agent-e2e.log.path marker file.
const tempDir = process.env.TEMP || process.env.TMP || os.tmpdir();
const desktopLogPath = path.join(tempDir, 'bw-agent-e2e.log');

function specDir(testFile) {
  return path.join(resultsRoot, path.basename(testFile, '.js'));
}

export const config = {
  host: '127.0.0.1',
  port: 4444,
  specs: ['./test/specs/**/*.js'],
  maxInstances: 1,
  capabilities: [
    {
      maxInstances: 1,
      'tauri:options': {
        application: path.resolve(__dirname, '..', 'target', 'debug', 'bw-agent-desktop'),
      },
    },
  ],
  reporters: ['spec'],
  services: [
    ['visual', {
      baselineFolder: path.join(resultsRoot, 'visual-baseline'),
      formatImageName: '{tag}-{browserName}-{width}x{height}',
      screenshotPath: path.join(resultsRoot, '.tmp'),
      createJsonReportFiles: true,
    }],
  ],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 60000,
  },

  // ── Build ────────────────────────────────────────────────────
  onPrepare: () => {
    // Delete stale desktop log from previous runs
    try { fs.unlinkSync(desktopLogPath); } catch {}

    spawnSync('pnpm', ['tauri', 'build', '--debug', '--no-bundle'], {
      cwd: path.resolve(__dirname, '..'),
      stdio: 'inherit',
      shell: true,
      env: { ...process.env },
    });
  },

  // ── tauri-driver lifecycle ───────────────────────────────────
  beforeSession: () => {
    tauriDriver = spawn(
      path.resolve(os.homedir(), '.cargo', 'bin', 'tauri-driver'),
      [],
      {
        stdio: [null, process.stdout, process.stderr],
      },
    );

    tauriDriver.on('error', (error) => {
      console.error('tauri-driver error:', error);
      process.exit(1);
    });

    tauriDriver.on('exit', (code) => {
      if (!exit) {
        console.error('tauri-driver exited with code:', code);
        process.exit(1);
      }
    });
  },

  afterSession: () => {
    closeTauriDriver();
  },

  // ── Collect credentials once per session (before any test runs) ──
  before: async () => {
    // How many accounts are needed? Read from E2E_ACCOUNT_COUNT (default 1).
    const count = parseInt(process.env.E2E_ACCOUNT_COUNT || '1', 10);
    globalThis.__e2eAccounts = [];

    for (let i = 1; i <= count; i++) {
      console.log(`\n[e2e] Prompting for account ${i}/${count}...`);
      // eslint-disable-next-line no-await-in-loop
      const creds = await getCredential(i);
      globalThis.__e2eAccounts.push(creds);
      console.log(`[e2e] Account ${i} collected: ${creds.email}`);
    }
    console.log(`[e2e] All ${count} account(s) collected. Starting tests...\n`);
  },

  // ── Per-spec: screen recording + desktop log snapshot ────────
  beforeSuite: (suite) => {
    const dir = specDir(suite.file);
    startRecording(path.join(dir, 'recording.mp4'));
  },

  afterSuite: async (suite) => {
    await stopRecording();

    // Snapshot the accumulated desktop log into this spec's folder
    const dir = specDir(suite.file);
    saveDesktopLog(desktopLogPath, path.join(dir, 'desktop.log'));
  },

  // ── Per-test: screenshot + web console audit ─────────────────
  afterTest: async (test) => {
    const dir = specDir(test.file);
    const name = safeName(test.title);

    // Screenshot
    await takeScreenshot(browser, path.join(dir, `${name}.png`));

    // Web console (JS) logs
    await captureWebConsole(browser, path.join(dir, `${name}_console.json`));
  },
};

function closeTauriDriver() {
  exit = true;
  tauriDriver?.kill();
}

function onShutdown(fn) {
  const cleanup = () => {
    try {
      fn();
    } finally {
      process.exit();
    }
  };

  process.on('exit', cleanup);
  process.on('SIGINT', cleanup);
  process.on('SIGTERM', cleanup);
  process.on('SIGHUP', cleanup);
  process.on('SIGBREAK', cleanup);
}

onShutdown(() => {
  closeTauriDriver();
});
