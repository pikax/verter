import type { PlatformEntry } from "@verter/binary-launcher";

export type { LibcTag, PlatformEntry } from "@verter/binary-launcher";

export declare const BINARY_STEM: string;
export declare const PLATFORM_MATRIX: readonly PlatformEntry[];
export declare const PLATFORM_PACKAGE_PREFIX: string;
export declare const SUPPORTED_RUST_TARGETS: readonly string[];
export declare const SUPPORTED_TARGETS: string;

/** Build a matrix for this family from an arbitrary rust-target list. */
export declare function buildPlatformMatrix(
  rustTargets: readonly string[],
): readonly PlatformEntry[];
