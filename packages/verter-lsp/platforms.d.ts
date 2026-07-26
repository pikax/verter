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

export declare const BINARY_STEM: string;
export declare const PLATFORM_MATRIX: readonly PlatformEntry[];
export declare const PLATFORM_PACKAGE_PREFIX: string;
export declare const SUPPORTED_RUST_TARGETS: readonly string[];
export declare const SUPPORTED_TARGETS: string;
export declare function buildPlatformMatrix(
  rustTargets: readonly string[],
): readonly PlatformEntry[];
