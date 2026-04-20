#!/usr/bin/env node
// Stages the bw-agent CLI binary as a Tauri sidecar for git signing.
// The binary is copied as bw-agent-git-sign-<triple> so that argv[0]
// detection routes to the git-sign code path at runtime.
//
// Usage:
//   node scripts/stage-sidecar.mjs [target-triple]
//
// If target-triple is omitted, defaults to the host triple from `rustc`.

import { execSync } from "node:child_process";
import { existsSync, mkdirSync, copyFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const binariesDir = join(root, "src-tauri", "binaries");

const triple = process.argv[2] || execSync("rustc --print host-tuple", { encoding: "utf-8" }).trim();
const isWindows = triple.includes("windows");
const ext = isWindows ? ".exe" : "";

const srcName = `bw-agent${ext}`;
const dstName = `bw-agent-git-sign-${triple}${ext}`;

// Check debug first, then release
const candidates = [
  join(root, "target", "debug", srcName),
  join(root, "target", triple, "debug", srcName),
  join(root, "target", "release", srcName),
  join(root, "target", triple, "release", srcName),
];

let srcPath = null;
for (const candidate of candidates) {
  if (existsSync(candidate)) {
    srcPath = candidate;
    break;
  }
}

if (!srcPath) {
  console.log(`bw-agent binary not found, building...`);
  execSync("cargo build -p bw-agent", { stdio: "inherit" });
  // Re-check after build
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      srcPath = candidate;
      break;
    }
  }
  if (!srcPath) {
    console.error(`Error: ${srcName} still not found after build.`);
    process.exit(1);
  }
}

mkdirSync(binariesDir, { recursive: true });
const dstPath = join(binariesDir, dstName);
copyFileSync(srcPath, dstPath);
console.log(`Staged sidecar: ${dstPath}`);
