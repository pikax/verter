/** A node-style libc tag, as carried by `package.json#libc`. */
export type LibcTag = "glibc" | "musl";

/** One fully-reconciled platform row. */
export interface PlatformEntry {
  /** The rust target triple, e.g. `x86_64-unknown-linux-gnu`. */
  readonly rustTarget: string;
  /** The npm platform suffix, e.g. `linux-x64-gnu`. */
  readonly npmSuffix: string;
  /** The platform package name, e.g. `@verter/lsp-linux-x64-gnu`. */
  readonly packageName: string;
  /** Node `process.platform` value this row serves. */
  readonly os: "darwin" | "linux" | "win32";
  /** Node `process.arch` value this row serves. */
  readonly cpu: "arm64" | "x64";
  /** The npm `libc` tag, or `null` outside the Linux gnu/musl split. */
  readonly libc: LibcTag | null;
  /** The binary file name shipped by this row's platform package. */
  readonly binaryName: string;
}

/** Where a resolved binary came from. */
export type BinarySource = "platform-package" | "dev-build" | "path";

/** One candidate binary location, tagged with its provenance. */
export interface BinaryCandidate {
  /** Absolute path, or the bare binary name for the `path` source. */
  readonly path: string;
  readonly source: BinarySource;
}

/** Host overrides and lookup seams. Defaults describe the running process. */
export interface ResolveOptions {
  /** Node `process.platform` value. Defaults to the running platform. */
  readonly platform?: string;
  /** Node `process.arch` value. Defaults to the running architecture. */
  readonly arch?: string;
  /** Whether the host libc is musl. Probed on linux when omitted. */
  readonly musl?: boolean;
  /** Override the platform-package lookup for this call. */
  readonly platformPackageDir?: (packageName: string) => string | null;
}

/** Naming inputs that distinguish one binary family from another. */
export interface MatrixNaming {
  /** npm scope prefix for the platform packages, e.g. `@verter/lsp-`. */
  readonly packagePrefix: string;
  /** On-disk stem of the binary, without any `.exe` suffix. */
  readonly binaryStem: string;
}

export interface LauncherOptions {
  /** User-facing tool name, used in error messages. */
  readonly toolName: string;
  readonly matrix: readonly PlatformEntry[];
  /** Repository root, for development-build discovery. */
  readonly workspaceRoot: string;
  /** Platform-package directory lookup bound to the caller's `require`. */
  readonly resolvePackageDir: (packageName: string) => string | null;
}

export interface Launcher {
  readonly toolName: string;
  readonly PLATFORM_MATRIX: readonly PlatformEntry[];
  readonly SUPPORTED_TARGETS: string;
  isMusl(): boolean;
  resolveSuffix(platform: string, arch: string, musl: boolean): string | null;
  platformPackageName(npmSuffix: string): string | null;
  binaryCandidates(options?: ResolveOptions): readonly BinaryCandidate[];
  resolveBinary(options?: ResolveOptions): BinaryCandidate;
  binaryPath(options?: ResolveOptions): string;
}

export declare function buildPlatformMatrix(
  rustTargets: readonly string[],
  naming: MatrixNaming,
): readonly PlatformEntry[];

export declare function createLauncher(options: LauncherOptions): Launcher;

/** Whether the host's libc is musl. Always `false` off linux. */
export declare function isMusl(): boolean;

/** A platform-package directory lookup bound to a caller's module resolution. */
export declare function packageDirResolver(requireFn: {
  resolve(request: string): string;
}): (packageName: string) => string | null;
