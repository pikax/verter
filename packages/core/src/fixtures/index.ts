/**
 * Fixture Testing Utilities
 *
 * This module provides utilities for fixture-based type testing:
 *
 * - `types.ts` - Type definitions for fixtures
 * - `ts-program.ts` - TypeScript program utilities for type checking
 *
 * @example Basic usage
 * ```typescript
 * import type { Fixture, TypeTest } from '@/fixtures';
 * import { runTypeTest, getTypeString } from '@/fixtures';
 * ```
 */

// Re-export types
export * from "./types";

// Re-export TypeScript utilities
export * from "./ts-program";
