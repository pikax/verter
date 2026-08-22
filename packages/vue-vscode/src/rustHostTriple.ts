import { existsSync } from "node:fs";

/**
 * Standard musl-libc filesystem signature, independent of Alpine
 * specifically — the musl dynamic linker's well-known path, arch-qualified.
 * A positive hit here is as strong a musl signal as the Alpine marker file.
 */
const MUSL_DYNAMIC_LINKER_BY_ARCH: Record<string, string> = {
  x64: "/lib/ld-musl-x86_64.so.1",
  arm64: "/lib/ld-musl-aarch64.so.1",
};

/**
 * Best-effort detection of the REAL running host's Linux libc flavor —
 * mirrors `packages/dx-harness/src/core/rustHostTriple.ts`'s identical
 * helper. Deliberately keys off `process.platform`/actual filesystem
 * markers, NOT whatever `platform` a caller passed to `hostRustTriples` —
 * so a caller simulating a different platform (tests) never gets a false
 * libc filter applied against the real host it happens to be running on.
 *
 * - `process.report`'s `header.glibcVersionRuntime` is present only when
 *   the running Node binary is itself glibc-linked (the same technique
 *   the `detect-libc` package uses, without adding a dependency).
 * - Absent that, `/etc/alpine-release` is the standard filesystem signal
 *   for an Alpine/musl host (a musl-linked Node gives no report signal).
 * - Absent that too, the musl dynamic linker's filesystem path is a second,
 *   Alpine-independent positive musl signal (covers musl-based distros that
 *   aren't Alpine).
 * - Neither signal firing means "can't tell" (`undefined`). Callers must
 *   treat this as "assume gnu", NOT "offer both ABI candidates" — see
 *   `hostRustTriples` below. An open ambiguity here previously let a
 *   downstream "pick the newest candidate" search select an
 *   ABI-incompatible cross-build purely because it had a newer mtime; gnu
 *   is the overwhelmingly common non-Alpine Linux case, so defaulting to it
 *   is the safer failure mode even though a genuinely-musl,
 *   non-Alpine, non-ld-musl-marker host is not perfectly covered.
 */
export function detectLinuxLibc(): "gnu" | "musl" | undefined {
  if (process.platform !== "linux") return undefined;
  try {
    const header = (
      process.report?.getReport() as { header?: { glibcVersionRuntime?: string } } | undefined
    )?.header;
    if (header?.glibcVersionRuntime) return "gnu";
  } catch {
    // process.report unavailable — fall through to the filesystem heuristics
  }
  if (existsSync("/etc/alpine-release")) return "musl";
  const muslLinker = MUSL_DYNAMIC_LINKER_BY_ARCH[process.arch];
  if (muslLinker && existsSync(muslLinker)) return "musl";
  return undefined;
}

/**
 * Ordered candidate Rust host-target triples for the current platform/arch,
 * matching the triples cargo writes under `target/<triple>/<profile>/` when
 * a build is invoked with an explicit `--target` (see `packages/native`'s
 * `napi.targets` and `scripts/host-target.mjs` — both list the same set).
 *
 * Root build scripts (`build:lsp`, `build:native`) now pass an explicit
 * `--target` so NAPI and the LSP share one host-target layout (see the
 * "one explicit host target" build-lane rule). Dev-mode binary discovery in
 * this extension must therefore look in the triple-qualified directory
 * FIRST, falling back to the legacy untriple-qualified `target/<profile>/`
 * directory a bare `cargo build` (no `--target`) still produces.
 *
 * Linux libc (glibc vs musl) is not observable from `process.*` alone in
 * general. When `detectLinuxLibc()` DOES resolve the real host's flavor, the
 * incompatible triple is dropped so `findLspBinary`'s "pick the newest
 * candidate" search can never select an ABI-incompatible cross-build purely
 * because it is newer than the compatible one. When detection is
 * inconclusive, this fails CLOSED to the gnu triple only (NOT both) — see
 * `detectLinuxLibc`'s doc comment for why an open "offer both" default was
 * the bug this closes.
 */
export function hostRustTriples(
  platform: NodeJS.Platform = process.platform,
  arch: string = process.arch,
): string[] {
  if (platform === "darwin") {
    if (arch === "arm64") return ["aarch64-apple-darwin"];
    if (arch === "x64") return ["x86_64-apple-darwin"];
    return [];
  }
  if (platform === "win32") {
    if (arch === "x64") return ["x86_64-pc-windows-msvc"];
    return [];
  }
  if (platform === "linux") {
    let triples: string[];
    if (arch === "x64") triples = ["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"];
    else if (arch === "arm64")
      triples = ["aarch64-unknown-linux-gnu", "aarch64-unknown-linux-musl"];
    else return [];
    const libc = detectLinuxLibc() ?? "gnu";
    return triples.filter((triple) => triple.endsWith(libc));
  }
  return [];
}
