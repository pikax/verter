import type { PlatformEntry } from "./platforms.js";

/** Where a resolved server binary came from. */
export type ServerBinarySource = "platform-package" | "dev-build" | "path";

/** One candidate server-binary location, tagged with its provenance. */
export interface ServerBinaryCandidate {
  /** Absolute path, or the bare binary name for the `path` source. */
  readonly path: string;
  readonly source: ServerBinarySource;
}

/** Host overrides and lookup seams. Defaults describe the running process. */
export interface ResolveOptions {
  /** Node `process.platform` value. Defaults to the running platform. */
  readonly platform?: string;
  /** Node `process.arch` value. Defaults to the running architecture. */
  readonly arch?: string;
  /** Whether the host libc is musl. Probed on linux when omitted. */
  readonly musl?: boolean;
  /**
   * Resolve a platform package's directory, or `null` when it is not
   * installed. Defaults to a `require.resolve` lookup.
   */
  readonly platformPackageDir?: (packageName: string) => string | null;
}

export declare const PLATFORM_MATRIX: readonly PlatformEntry[];
export declare const SUPPORTED_TARGETS: string;

/** Whether the host's libc is musl. Always `false` off linux. */
export declare function isMusl(): boolean;

/** The platform package name for an npm platform suffix. */
export declare function platformPackageName(npmSuffix: string): string;

/**
 * The npm platform suffix serving a host, or `null` when no platform package
 * covers it.
 */
export declare function resolveSuffix(platform: string, arch: string, musl: boolean): string | null;

/** The ordered candidate locations of the server binary for a host. */
export declare function serverBinaryCandidates(
  options?: ResolveOptions,
): readonly ServerBinaryCandidate[];

/**
 * The server binary for a host: the first candidate present on disk, falling
 * back to the bare name for `PATH` lookup. Throws for an unsupported host.
 */
export declare function resolveServerBinary(options?: ResolveOptions): ServerBinaryCandidate;

/** The path (or bare `PATH` name) of the server binary for a host. */
export declare function serverBinaryPath(options?: ResolveOptions): string;
