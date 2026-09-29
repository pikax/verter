export type ResolveHook = (
  source: string,
  importer: string,
  options: { skipSelf: true },
) => Promise<unknown> | unknown;

/**
 * Asks the bundler to resolve a dependency specifier to a readable file id.
 *
 * Returns `null` when the bundler has no readable runtime target: no result,
 * an external or virtual id, or a resolver that throws (rolldown throws for a
 * package subpath exported only under the `types` condition). Callers then
 * fall back to their own filesystem or package-declaration resolution.
 */
export async function resolveThroughBundler(
  resolveId: ResolveHook,
  specifier: string,
  importer: string,
): Promise<string | null> {
  let result: unknown;
  try {
    result = await resolveId(specifier, importer, { skipSelf: true });
  } catch {
    return null;
  }
  if (!result) return null;
  if (typeof result === "string") {
    return result.startsWith("\0") || result.includes("?") ? null : result;
  }
  if (typeof result !== "object") return null;

  const resolved = result as { id?: unknown; external?: unknown };
  if (resolved.external) return null;
  if (typeof resolved.id !== "string") return null;
  if (resolved.id.startsWith("\0") || resolved.id.includes("?")) return null;
  return resolved.id;
}
