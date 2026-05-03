// Tier 6 §8.2 / T9.1 — Windows-conditional post-build copy hook.
//
// On Windows (`process.platform === "win32"`), if napi build did not
// produce the expected `dist/verter-native.win32-x64-msvc.node`,
// this hook copies it from the cargo target directory. The napi-rs
// CLI normally produces the renamed file at the dist location
// directly, but a direct `cargo build --release --package
// verter_napi` (e.g. from a developer doing a quick rebuild) lands
// the DLL in `target/release/verter_napi.dll` without renaming. This
// hook closes that gap so `packages/native/index.js`'s tryLoad can
// find the binary.
//
// On non-Windows platforms this script is a no-op.
//
// The brief specifies the canonical source path as
// `target/x86_64-pc-windows-msvc/release/verter_napi.dll` (what
// `napi build --platform` produces). The fallback
// `target/release/verter_napi.dll` covers the direct-`cargo build`
// case documented in MEMORY.md.

import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = dirname(dirname(fileURLToPath(import.meta.url)));
const repoRoot = resolve(packageDir, "..", "..");
const distDir = join(packageDir, "dist");
const TARGET_FILENAME = "verter-native.win32-x64-msvc.node";
const targetPath = join(distDir, TARGET_FILENAME);

const CANDIDATE_DLL_PATHS = [
  // Canonical path produced by `napi build --platform` (it passes
  // `--target=x86_64-pc-windows-msvc` to cargo).
  resolve(repoRoot, "target", "x86_64-pc-windows-msvc", "release", "verter_napi.dll"),
  // Fallback: direct `cargo build --release --package verter_napi`
  // (the MEMORY.md "Quick rebuild native + copy" command).
  resolve(repoRoot, "target", "release", "verter_napi.dll"),
];

function copyArtefactIfNeeded() {
  if (process.platform !== "win32") {
    return { skipped: true, reason: "non-windows-platform" };
  }

  // If napi build already produced the platform-named file, do
  // nothing. Verify by checking that the existing file is recent
  // (no truncated zero-byte file from an interrupted prior build).
  if (existsSync(targetPath)) {
    const stat = statSync(targetPath);
    if (stat.size > 0) {
      return { skipped: true, reason: "target-already-exists", targetPath };
    }
  }

  for (const candidate of CANDIDATE_DLL_PATHS) {
    if (!existsSync(candidate)) {
      continue;
    }
    mkdirSync(distDir, { recursive: true });
    copyFileSync(candidate, targetPath);
    return { copied: true, source: candidate, target: targetPath };
  }

  return {
    skipped: true,
    reason: "no-source-dll-found",
    searched: CANDIDATE_DLL_PATHS,
  };
}

const result = copyArtefactIfNeeded();
if (result.copied) {
  process.stdout.write(`copied ${result.source} -> ${result.target}\n`);
} else if (result.reason === "non-windows-platform") {
  // Silent no-op on non-Windows — keeps `pnpm run build:native`
  // output clean on macOS/Linux.
} else if (result.reason === "target-already-exists") {
  process.stdout.write(`target already present: ${result.targetPath}\n`);
} else if (result.reason === "no-source-dll-found") {
  // Not necessarily an error: napi build may have failed with its
  // own error message and there's no target build either. Print a
  // diagnostic so the developer can investigate.
  process.stdout.write(
    `[copy-windows-artefact] no source DLL found, skipping. Searched:\n  ${result.searched.join(
      "\n  ",
    )}\n`,
  );
}

export { copyArtefactIfNeeded, CANDIDATE_DLL_PATHS, TARGET_FILENAME };
