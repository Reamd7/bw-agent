import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// ── Resolve version ──────────────────────────────────────────────────
const raw = process.env.RELEASE_VERSION || "";
const version = raw.replace(/^v/, ""); // strip leading 'v'

if (!version) {
  console.error("RELEASE_VERSION env var is empty");
  process.exit(1);
}

console.log(`Syncing version: ${version}`);

// ── MSI-safe version ─────────────────────────────────────────────────
// MSI/WiX requires pre-release identifiers to be purely numeric.
// e.g. "0.0.0-alpha.0" → "0.0.0-0", "2.0.0-rc.1" → "2.0.0-1"
// Plain "1.2.3" stays unchanged.
const msiVersion = version.replace(/-[a-zA-Z]+\.(\d+)$/, "-$1");

console.log(`  tauri.conf.json → ${msiVersion}`);

// ── package.json ─────────────────────────────────────────────────────
patchJson("package.json", (obj) => {
  obj.version = version;
});

// ── src-tauri/tauri.conf.json ────────────────────────────────────────
patchJson("src-tauri/tauri.conf.json", (obj) => {
  obj.version = msiVersion;
});

// ── Cargo.toml files ─────────────────────────────────────────────────
const cargoFiles = [
  "src-tauri/Cargo.toml",
  "crates/bw-agent/Cargo.toml",
  "crates/bw-core/Cargo.toml",
];

for (const rel of cargoFiles) {
  const abs = join(process.cwd(), rel);
  try {
    let content = readFileSync(abs, "utf8");
    content = content.replace(
      /^version\s*=\s*"[^"]*"/m,
      `version = "${version}"`
    );
    writeFileSync(abs, content);
    console.log(`  ${rel} → ${version}`);
  } catch {
    console.log(`  ${rel} — skipped (not found)`);
  }
}

// ── helpers ───────────────────────────────────────────────────────────
function patchJson(relPath, mutate) {
  const abs = join(process.cwd(), relPath);
  const obj = JSON.parse(readFileSync(abs, "utf8"));
  mutate(obj);
  writeFileSync(abs, JSON.stringify(obj, null, 2) + "\n");
  console.log(`  ${relPath} → ${obj.version}`);
}
