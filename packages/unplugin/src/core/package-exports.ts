/**
 * Declaration targets a package's `exports` map declares for a sub-path.
 *
 * `subpath` is the package-relative specifier (`"./audit"`). An exact key wins
 * over `*` patterns, and among patterns the longest prefix wins, as in Node's
 * resolver. The `types` condition is preferred at every nesting level; a
 * runtime JavaScript target contributes its sibling declaration files.
 *
 * Returns `null` when the map has no entry for the sub-path, so the caller can
 * fall back to probing the literal package layout; an empty array means the
 * sub-path is mapped but declares no target.
 */
export function exportsDeclarationTargets(exportsField: unknown, subpath: string): string[] | null {
  if (!exportsField || typeof exportsField !== "object" || Array.isArray(exportsField)) {
    return null;
  }
  const map = exportsField as Record<string, unknown>;
  if (!Object.keys(map).some((key) => key.startsWith("."))) return null;

  if (Object.prototype.hasOwnProperty.call(map, subpath)) {
    return declarationTargets(map[subpath], null);
  }

  let bestKey: string | null = null;
  let bestMatch = "";
  for (const key of Object.keys(map)) {
    const star = key.indexOf("*");
    if (star === -1 || key.indexOf("*", star + 1) !== -1) continue;
    const prefix = key.slice(0, star);
    const suffix = key.slice(star + 1);
    if (!subpath.startsWith(prefix) || !subpath.endsWith(suffix)) continue;
    if (subpath.length < prefix.length + suffix.length) continue;
    if (bestKey === null || prefix.length > bestKey.indexOf("*")) {
      bestKey = key;
      bestMatch = subpath.slice(prefix.length, subpath.length - suffix.length);
    }
  }
  return bestKey === null ? null : declarationTargets(map[bestKey], bestMatch);
}

function declarationTargets(entry: unknown, patternMatch: string | null): string[] {
  if (typeof entry === "string") {
    const target = patternMatch === null ? entry : entry.split("*").join(patternMatch);
    return declarationsForTarget(target);
  }
  if (Array.isArray(entry)) {
    return entry.flatMap((item) => declarationTargets(item, patternMatch));
  }
  if (!entry || typeof entry !== "object") return [];

  const conditions = entry as Record<string, unknown>;
  const ordered = Object.keys(conditions).sort(
    (a, b) => Number(b === "types") - Number(a === "types"),
  );
  return ordered.flatMap((condition) => declarationTargets(conditions[condition], patternMatch));
}

function declarationsForTarget(target: string): string[] {
  const runtime = target.match(/\.(m|c)?js$/);
  if (!runtime) return [target];
  const base = target.slice(0, -runtime[0].length);
  const flavor = runtime[1] ?? "";
  return [`${base}.d.${flavor}ts`];
}
