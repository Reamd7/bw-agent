#!/usr/bin/env node
// Stages the bw-agent CLI binary as a Tauri sidecar.
// Run after `cargo build` — copies the binary to src-tauri/binaries/ with
// the target-triple suffix that Tauri's externalBin expects.
//
// Usage:
//   node scripts/stage-sidecar.mjs [target-triple]
//
// If target-triple is omitted, defaults to the host triple from `rustc`.

import { execSync } from "node:child_process";
import { existsSync, linkSync, mkdirSync, copyFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const binariesDir = join(root, "src-tauri", "binaries");

const triple = process.argv[2] || execSync("rustc --print host-tuple", { encoding: "utf-8" }).trim();
const isWindows = triple.includes("windows");
const ext = isWindows ? ".exe" : "";

const srcName = `bw-agent${ext}`;
const dstName = `bw-agent-${triple}${ext}`;

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
  console.error(`Error: ${srcName} not found. Run 'cargo build -p bw-agent' first.`);
  process.exit(1);
}

mkdirSync(binariesDir, { recursive: true });
const dstPath = join(binariesDir, dstName);
copyFileSync(srcPath, dstPath);
console.log(`Staged sidecar: ${dstPath}`);

// Create a hardlink named bw-agent-git-sign-<triple> for gpg.ssh.program.
// Git calls gpg.ssh.program as a single executable (no argument splitting),
// so we need a separate entry point that detects "git-sign" in argv[0].
const signDstName = `bw-agent-git-sign-${triple}${ext}`;
const signDstPath = join(binariesDir, signDstName);
if (!existsSync(signDstPath)) {
  linkSync(dstPath, signDstPath);
  console.log(`Staged git-sign link: ${signDstPath}`);
}
