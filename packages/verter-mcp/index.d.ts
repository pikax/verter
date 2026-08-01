import type {
  BinaryCandidate,
  BinarySource,
  Launcher,
  PlatformEntry,
  ResolveOptions,
} from "@verter/binary-launcher";

export type { ResolveOptions } from "@verter/binary-launcher";

/** Where a resolved server binary came from. */
export type ServerBinarySource = BinarySource;

/** One candidate server-binary location, tagged with its provenance. */
export type ServerBinaryCandidate = BinaryCandidate;

export declare const PLATFORM_MATRIX: readonly PlatformEntry[];
export declare const SUPPORTED_TARGETS: string;

/** The underlying launcher, for consumers that want the full surface. */
export declare const launcher: Launcher;

/** Whether the host's libc is musl. Always `false` off linux. */
export declare function isMusl(): boolean;

/** The platform package name for an npm platform suffix. */
export declare function platformPackageName(npmSuffix: string): string | null;

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
