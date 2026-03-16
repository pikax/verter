import type { HostModuleReference, VerterHost } from "@verter/native";

export function collectResolvableModuleReferenceSpecifiers(
  host: Pick<VerterHost, "collectResolvableModuleReferenceSpecifiers">,
  moduleReferences: readonly HostModuleReference[],
): string[] {
  return host.collectResolvableModuleReferenceSpecifiers([...moduleReferences]);
}
