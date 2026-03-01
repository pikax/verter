/**
 * Detect and dynamically load CSS preprocessor compilers from the project's node_modules.
 *
 * Supports: `sass` (for Sass indented syntax) and `stylus`.
 * Gracefully falls back when packages are not installed.
 */

export interface PreprocessorCache {
  sass?: unknown;
  stylus?: unknown;
  /** Set to true after we've attempted (and failed) to resolve a package. */
  sassAttempted?: boolean;
  stylusAttempted?: boolean;
}

/**
 * Attempt to resolve and cache a preprocessor from the workspace's node_modules.
 *
 * @returns The loaded module, or null if not installed.
 */
export function resolvePreprocessor(
  lang: "sass" | "stylus",
  workspacePath: string,
  cache: PreprocessorCache,
): unknown | null {
  const attemptedKey = `${lang}Attempted` as keyof PreprocessorCache;
  if (cache[lang] !== undefined) return cache[lang] ?? null;
  if (cache[attemptedKey]) return null;

  try {
    const modulePath = require.resolve(lang, { paths: [workspacePath] });
    const mod = require(modulePath);
    cache[lang] = mod;
    return mod;
  } catch {
    (cache as Record<string, unknown>)[attemptedKey] = true;
    return null;
  }
}
