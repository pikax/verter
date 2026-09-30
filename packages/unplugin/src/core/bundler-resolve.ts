export type ResolveHook = (
  source: string,
  importer: string,
  options: { skipSelf: true },
) => Promise<unknown> | unknown;

/**
 * Outcome of asking the bundler to resolve a dependency specifier.
 *
 * `id` is a readable file id, or `null` when the bundler has no readable
 * target (no result, an external or virtual id, or a thrown resolver).
 * `failure` carries the resolver's error when it threw. Rolldown throws for a
 * package subpath exported only under the `types` condition, so a caller
 * first tries its own fallback and rethrows `failure.error` only when that
 * fallback cannot resolve the specifier either.
 */
export interface BundlerResolution {
  readonly id: string | null;
  readonly failure: { readonly error: unknown } | null;
}

export async function resolveThroughBundler(
  resolveId: ResolveHook,
  specifier: string,
  importer: string,
): Promise<BundlerResolution> {
  let result: unknown;
  try {
    result = await resolveId(specifier, importer, { skipSelf: true });
  } catch (error) {
    return { id: null, failure: { error } };
  }
  return { id: readableId(result), failure: null };
}

function readableId(result: unknown): string | null {
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
