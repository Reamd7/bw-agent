import { readdirSync, renameSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2] || "./release_artifacts";
const raw = process.env.RELEASE_VERSION || "";
const version = raw.replace(/^v/, "");

if (!version) {
  console.error("RELEASE_VERSION env var is empty");
  process.exit(1);
}

// Compute MSI-safe version (same logic as sync-version.mjs)
// e.g. "0.0.0-alpha.0" → "0.0.0-0", "1.2.3" → "1.2.3"
const msiVersion = version.replace(/-[a-zA-Z]+\.(\d+)$/, "-$1");

if (msiVersion === version) {
  console.log("No MSI-safe rename needed, versions match.");
  process.exit(0);
}

console.log(`Renaming artifacts: ${msiVersion} → ${version}`);

const files = readdirSync(dir);
let count = 0;

for (const file of files) {
  if (file.includes(msiVersion)) {
    const newName = file.replaceAll(msiVersion, version);
    const oldPath = join(dir, file);
    const newPath = join(dir, newName);
    renameSync(oldPath, newPath);
    console.log(`  ${file} → ${newName}`);
    count++;
  }
}

console.log(`Renamed ${count} file(s).`);
