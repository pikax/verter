/**
 * Extension-side SHARED tsgo editor-attach wiring (pure, `vscode`-free).
 *
 * The OWNED tsgo baseline is always the LSP's default; SHARED editor-attach is an
 * ADDITIVE, OPT-IN, FAIL-CLOSED overlay. For the LSP to engage SHARED it must be
 * given BOTH `--shared-control-dir` and `--shared-session-key` (see
 * `crates/verter_lsp/src/main.rs` `shared_rendezvous()` — both required), AND a
 * `verter-relay-shim` must be advertising a live real-tsgo attach under that
 * control-dir + session-key (see `crates/verter_relay_shim/src/main.rs`).
 *
 * This module owns the PURE decision logic the extension needs to drive that:
 *   - discover the built/packaged `verter-relay-shim` binary,
 *   - discover the native-preview tsgo the shim relays to (the real engine),
 *   - mint a session key + establish an isolated control dir,
 *   - build the `--shared-*` LSP args and the shim spawn args.
 *
 * Every step FAILS CLOSED: an absent shim OR an absent tsgo yields a non-engaged
 * plan (the extension stays OWNED), never a throw and never a partial rendezvous
 * (one `--shared-*` arg without the other would make the LSP's `shared_rendezvous`
 * refuse anyway). No `vscode` import — the whole surface is unit-testable without
 * the extension host.
 */
import { existsSync, mkdirSync } from "fs";
import { dirname, join } from "path";
import { randomBytes } from "crypto";

/** The fixed on-disk stem of the relay shim binary (mirrors `stage-bin.mjs`). */
export const RELAY_SHIM_STEM = "verter-relay-shim";

/** The shim binary basename for a platform (the `.exe` suffix follows the host). */
export function relayShimBasename(platform: NodeJS.Platform = process.platform): string {
  return platform === "win32" ? `${RELAY_SHIM_STEM}.exe` : RELAY_SHIM_STEM;
}

/** Map `process.platform` + `process.arch` to the native-preview package's arch token. */
function nativePreviewArchDir(platform: NodeJS.Platform, arch: string): string | undefined {
  const os =
    platform === "win32"
      ? "win32"
      : platform === "darwin"
        ? "darwin"
        : platform === "linux"
          ? "linux"
          : undefined;
  const cpu = arch === "x64" || arch === "x86_64" ? "x64" : arch === "arm64" ? "arm64" : undefined;
  if (!os || !cpu) return undefined;
  return `typescript-${os}-${cpu}`;
}

/**
 * Ordered candidate source paths for the relay shim binary:
 *   1. `VERTER_RELAY_SHIM_BINARY` (an explicit absolute path — the CI / dev seam), else
 *   2. dev: `<repoRoot>/target/{debug,release}/<basename>` walking up from the extension
 *      path (freshest local build preferred — mirrors `findLspBinary`), else
 *   3. packaged: `<extensionPath>/bin/<basename>` (the staged VSIX shim).
 */
export function relayShimCandidates(opts: {
  extensionPath: string;
  env?: Record<string, string | undefined>;
  platform?: NodeJS.Platform;
}): string[] {
  const { extensionPath, env = process.env, platform = process.platform } = opts;
  const explicit = env.VERTER_RELAY_SHIM_BINARY;
  if (explicit) return [explicit];

  const base = relayShimBasename(platform);
  const candidates: string[] = [];
  // Dev builds first (freshest) — walk up to the monorepo root's target/ dir.
  let dir = extensionPath;
  for (let i = 0; i < 5; i++) {
    for (const profile of ["debug", "release"]) {
      candidates.push(join(dir, "target", profile, base));
    }
    dir = dirname(dir);
  }
  // Packaged VSIX bin/ last (the real-install location, where no target/ exists).
  candidates.push(join(extensionPath, "bin", base));
  return candidates;
}

/** Resolve the first existing relay-shim candidate, or `undefined` (fail-closed). */
export function discoverRelayShim(opts: {
  extensionPath: string;
  env?: Record<string, string | undefined>;
  platform?: NodeJS.Platform;
  exists?: (p: string) => boolean;
}): string | undefined {
  const exists = opts.exists ?? existsSync;
  for (const candidate of relayShimCandidates(opts)) {
    if (exists(candidate)) return candidate;
  }
  return undefined;
}

/**
 * Discover the native-preview tsgo binary the shim relays to (the REAL engine).
 * Order:
 *   1. `VERTER_TSGO_BIN` (an explicit absolute path — the provisioning seam), else
 *   2. `typescript.native-preview.tsdk` (a dir → `<tsdk>/tsc(.exe)`), else
 *   3. `<workspaceRoot>/node_modules/@typescript/typescript-<os>-<cpu>/lib/tsc(.exe)`.
 *
 * Returns `undefined` (fail-closed) when nothing resolves — the extension stays OWNED.
 */
export function discoverNativePreviewTsgo(opts: {
  env?: Record<string, string | undefined>;
  nativePreviewTsdk?: string;
  workspaceRoot?: string;
  platform?: NodeJS.Platform;
  arch?: string;
  exists?: (p: string) => boolean;
}): string | undefined {
  const {
    env = process.env,
    nativePreviewTsdk,
    workspaceRoot,
    platform = process.platform,
    arch = process.arch,
  } = opts;
  const exists = opts.exists ?? existsSync;
  const binName = platform === "win32" ? "tsc.exe" : "tsc";

  const explicit = env.VERTER_TSGO_BIN;
  if (explicit && exists(explicit)) return explicit;

  if (nativePreviewTsdk) {
    const candidate = join(nativePreviewTsdk, binName);
    if (exists(candidate)) return candidate;
  }

  if (workspaceRoot) {
    const archDir = nativePreviewArchDir(platform, arch);
    if (archDir) {
      const candidate = join(workspaceRoot, "node_modules", "@typescript", archDir, "lib", binName);
      if (exists(candidate)) return candidate;
    }
  }

  return undefined;
}

/** Mint a 128-bit hex session key (CSPRNG). Distinguishes concurrent rendezvous. */
export function mintSessionKey(rng: (n: number) => Buffer = randomBytes): string {
  return rng(16).toString("hex");
}

/**
 * Establish (create) an isolated control dir for the rendezvous, under `root`.
 * The parent dir MUST exist before the shim binds its control endpoint (the shim's
 * UDS/named-pipe bind does not create it), so the extension creates it here.
 */
export function establishControlDir(opts: {
  root: string;
  sessionKey: string;
  mkdir?: (p: string) => void;
}): string {
  const dir = join(opts.root, `verter-shared-${opts.sessionKey}`);
  (opts.mkdir ?? ((p: string) => void mkdirSync(p, { recursive: true })))(dir);
  return dir;
}

/**
 * The `--shared-*` LSP args that arm SHARED editor-attach. BOTH are always emitted
 * together — the LSP's `shared_rendezvous()` engages SHARED only when BOTH are
 * present, so a partial pair is forbidden (would be a silent no-op). Throws on an
 * empty component (an internal invariant — a caller must supply a real rendezvous).
 */
export function buildSharedLspArgs(rendezvous: {
  controlDir: string;
  sessionKey: string;
}): string[] {
  if (!rendezvous.controlDir || !rendezvous.sessionKey) {
    throw new Error(
      "buildSharedLspArgs requires BOTH controlDir and sessionKey (the LSP engages SHARED " +
        "only when --shared-control-dir AND --shared-session-key are both present)",
    );
  }
  return [
    `--shared-control-dir=${rendezvous.controlDir}`,
    `--shared-session-key=${rendezvous.sessionKey}`,
  ];
}

/**
 * The shim spawn args: `--real-tsgo <engine> --control-dir <dir> --session-key <key>
 * -- --lsp --stdio`. Everything after `--` is forwarded to the real tsgo verbatim;
 * the shim advertises into `<control-dir>` under `<session-key>` and relays the
 * `--lsp` stdio (see `crates/verter_relay_shim/src/main.rs`).
 */
export function buildShimSpawnArgs(rendezvous: {
  realTsgo: string;
  controlDir: string;
  sessionKey: string;
}): string[] {
  return [
    "--real-tsgo",
    rendezvous.realTsgo,
    "--control-dir",
    rendezvous.controlDir,
    "--session-key",
    rendezvous.sessionKey,
    "--",
    "--lsp",
    "--stdio",
  ];
}

/** A non-engaged plan carries the fail-closed reason (the extension stays OWNED). */
export interface SharedTsgoNotEngaged {
  engaged: false;
  reason: string;
}

/** An engaged plan carries everything needed to spawn the shim + arm the LSP. */
export interface SharedTsgoEngaged {
  engaged: true;
  shimPath: string;
  realTsgo: string;
  controlDir: string;
  sessionKey: string;
  /** The `--shared-*` args to append to the verter-lsp argv. */
  lspArgs: string[];
  /** The argv to spawn `verter-relay-shim` with. */
  shimArgs: string[];
}

export type SharedTsgoPlan = SharedTsgoEngaged | SharedTsgoNotEngaged;

/**
 * Plan SHARED editor-attach, FAIL-CLOSED. Returns a non-engaged plan (never throws)
 * when the shim or the native-preview tsgo cannot be resolved — the extension then
 * launches the OWNED baseline with no `--shared-*` args. Only when BOTH resolve does
 * it mint a session key, create the control dir, and return the engaged plan.
 */
export function planSharedTsgo(opts: {
  extensionPath: string;
  controlDirRoot: string;
  env?: Record<string, string | undefined>;
  nativePreviewTsdk?: string;
  workspaceRoot?: string;
  platform?: NodeJS.Platform;
  arch?: string;
  exists?: (p: string) => boolean;
  mkdir?: (p: string) => void;
  rng?: (n: number) => Buffer;
}): SharedTsgoPlan {
  // Explicit opt-out: `VERTER_DISABLE_SHARED_TSGO` forces the OWNED baseline (no shim
  // spawned, no `--shared-*` args). Fail-closed escape hatch for isolating whether a
  // behaviour is OWNED- or SHARED-attributable, and for users who want the OWNED baseline.
  const env = opts.env ?? process.env;
  if (env.VERTER_DISABLE_SHARED_TSGO) {
    return { engaged: false, reason: "disabled by VERTER_DISABLE_SHARED_TSGO (OWNED baseline)" };
  }
  const shimPath = discoverRelayShim({
    extensionPath: opts.extensionPath,
    env: opts.env,
    platform: opts.platform,
    exists: opts.exists,
  });
  if (!shimPath) {
    return { engaged: false, reason: "relay shim binary not found (OWNED baseline)" };
  }

  const realTsgo = discoverNativePreviewTsgo({
    env: opts.env,
    nativePreviewTsdk: opts.nativePreviewTsdk,
    workspaceRoot: opts.workspaceRoot,
    platform: opts.platform,
    arch: opts.arch,
    exists: opts.exists,
  });
  if (!realTsgo) {
    return {
      engaged: false,
      reason: "native-preview tsgo (tsdk) not found (OWNED baseline)",
    };
  }

  const sessionKey = mintSessionKey(opts.rng);
  const controlDir = establishControlDir({
    root: opts.controlDirRoot,
    sessionKey,
    mkdir: opts.mkdir,
  });
  const lspArgs = buildSharedLspArgs({ controlDir, sessionKey });
  const shimArgs = buildShimSpawnArgs({ realTsgo, controlDir, sessionKey });
  return { engaged: true, shimPath, realTsgo, controlDir, sessionKey, lspArgs, shimArgs };
}

/** Whether an effective type-provider value routes tsgo (SHARED is a tsgo overlay). */
export function typeProviderRoutesTsgo(typeProvider: string | undefined): boolean {
  return typeProvider === "tsgo" || typeProvider === "shared-tsgo";
}

// ── SHARED armed-handshake verification (Q3) ────────────────────────────────────
//
// "[shared-tsgo] armed" is a legitimate wiring-liveness assertion ONLY when tied to
// an OBSERVABLE handshake — a bare log line can be emitted without the shim ever
// starting or the rendezvous ever reaching the LSP. These pure helpers reduce the
// dual-written test log + the on-disk control dir to the two observables that PROVE
// the SHARED bootstrap is really armed (not owned-only masquerading):
//   1. the shim STARTED + ADVERTISED — a `verter-relay-shim-*.json` advertisement is
//      present in the control dir the extension logged (`controlDir=…`); and
//   2. the `--shared-*` rendezvous PROPAGATED into the verter-lsp argv — the
//      `[buildServerOptions] … args=[…]` line carries `--shared-control-dir=` +
//      `--shared-session-key=<key>` for THIS session.
// Both are read from artifacts the extension actually produced, so removing the shim
// advertisement (or dropping the args) flips the verdict RED. They are `vscode`-free
// + string/array-only so the discrimination is unit-testable without the editor host.

/**
 * Parse the rendezvous control dir the extension logs on the `[shared-tsgo] armed`
 * line (`… controlDir=<path> (SHARED editor-attach overlay …`). Returns `undefined`
 * when the armed line is absent or carries no control dir — i.e. SHARED was never
 * armed (the owned-only / severed-wiring case). Path-separator-agnostic: the captured
 * path is used only as the advertisement-lookup anchor + the session-key source.
 */
export function parseArmedControlDir(logText: string): string | undefined {
  const m = /\[shared-tsgo\] armed:[^\n]*?\bcontrolDir=(.+?) \(SHARED editor-attach/.exec(logText);
  return m ? m[1].trim() : undefined;
}

/**
 * The session key embedded in a rendezvous control dir basename
 * (`…/verter-shared-<key>`), or `undefined`. Used to match the propagated
 * `--shared-session-key=<key>` arg WITHOUT comparing raw absolute paths (the
 * JSON-encoded argv escapes Windows backslashes, so a full-path compare is unsafe).
 */
export function sessionKeyFromControlDir(controlDir: string): string | undefined {
  const base = controlDir.replace(/[\\/]+$/, "");
  const m = /verter-shared-([0-9a-fA-F]+)$/.exec(base);
  return m ? m[1] : undefined;
}

/** Whether a filename is a relay-shim rendezvous advertisement (`verter-relay-shim-*.json`). */
export function isShimAdvertisement(fileName: string): boolean {
  return fileName.startsWith(`${RELAY_SHIM_STEM}-`) && fileName.endsWith(".json");
}

/**
 * Whether the extension armed the verter-lsp argv with BOTH `--shared-*` rendezvous
 * flags for `controlDir`'s session — the `[buildServerOptions] … args=[…]` observable
 * that the rendezvous PROPAGATED into the LSP command line (not merely a log string).
 * Cross-platform: matches the hex session key + the `verter-shared-<key>` dir basename
 * (both backslash-free) rather than the JSON-escaped absolute path.
 */
export function lspArgsPropagated(logText: string, controlDir: string): boolean {
  const key = sessionKeyFromControlDir(controlDir);
  if (!key) return false;
  const hasSessionKeyArg = logText.includes(`--shared-session-key=${key}`);
  const hasControlDirArg =
    logText.includes("--shared-control-dir=") && logText.includes(`verter-shared-${key}`);
  return hasSessionKeyArg && hasControlDirArg;
}

/** The observations the SHARED armed-handshake verdict is computed from. */
export interface SharedArmedObservation {
  /** The dual-written extension test log (`log.*` calls). */
  logText: string;
  /** The filenames present in the parsed control dir (the caller lists/polls the FS). */
  controlDirEntries: string[];
}

/** The SHARED armed-handshake verdict — `ok` ONLY when both observables hold. */
export interface SharedArmedVerdict {
  ok: boolean;
  /** The control dir parsed from the armed line, when present. */
  controlDir?: string;
  /** The relay-shim advertisements observed in the control dir. */
  advertisements: string[];
  /** Whether the `--shared-*` rendezvous propagated into the verter-lsp argv. */
  argsPropagated: boolean;
  /** Why the verdict is not `ok` (for the failing assertion message). */
  reason?: string;
}

/**
 * Verify the SHARED editor-attach handshake is OBSERVABLE (Q3). Accepts ONLY when the
 * extension armed the LSP argv with the `--shared-*` rendezvous AND the shim actually
 * STARTED + ADVERTISED (its advertisement file is present in the logged control dir).
 * A bare `[shared-tsgo] armed` line WITHOUT the propagated args OR WITHOUT a live
 * advertisement is REFUSED — that is exactly the owned-only / severed-handshake case a
 * bare log-string check would false-green. Removing the shim advertisement or dropping
 * the args flips this RED (the §1a discrimination the spec exercises).
 */
export function verifySharedArmedHandshake(obs: SharedArmedObservation): SharedArmedVerdict {
  const controlDir = parseArmedControlDir(obs.logText);
  const advertisements = controlDir ? obs.controlDirEntries.filter(isShimAdvertisement) : [];
  const argsPropagated = controlDir ? lspArgsPropagated(obs.logText, controlDir) : false;
  if (!controlDir) {
    return {
      ok: false,
      advertisements,
      argsPropagated,
      reason:
        "no `[shared-tsgo] armed` controlDir in the log — the extension never armed SHARED (owned-only)",
    };
  }
  if (!argsPropagated) {
    return {
      ok: false,
      controlDir,
      advertisements,
      argsPropagated,
      reason:
        "the --shared-* rendezvous did not propagate into the verter-lsp argv " +
        "([buildServerOptions] args=[…] carries no --shared-control-dir/--shared-session-key for this session)",
    };
  }
  if (advertisements.length === 0) {
    return {
      ok: false,
      controlDir,
      advertisements,
      argsPropagated,
      reason: `no relay-shim advertisement (verter-relay-shim-*.json) in ${controlDir} — the shim did not start/advertise; a bare "armed" log line is not a handshake`,
    };
  }
  return { ok: true, controlDir, advertisements, argsPropagated };
}
