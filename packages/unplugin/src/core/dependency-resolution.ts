import type { HostModuleReference } from "@verter/native";

export function collectResolvableModuleReferenceSpecifiers(
  moduleReferences: readonly HostModuleReference[],
): string[] {
  const seen = new Set<string>();
  const specifiers: string[] = [];

  for (const reference of moduleReferences) {
    const candidates =
      reference.analyzability === "exact"
        ? reference.literalSpecifier
          ? [reference.literalSpecifier]
          : []
        : reference.analyzability === "finiteSet"
          ? reference.finiteSpecifiers
          : [];

    for (const specifier of candidates) {
      if (!specifier || seen.has(specifier)) continue;
      seen.add(specifier);
      specifiers.push(specifier);
    }
  }

  return specifiers;
}
