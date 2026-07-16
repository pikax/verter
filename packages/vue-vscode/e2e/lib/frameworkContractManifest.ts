export const FRAMEWORK_CONTRACT_CAPABILITIES = [
  "ts.clean-diagnostics",
  "js.clean-diagnostics",
  "ts.definition.markup-to-script",
  "js.definition.markup-to-script",
  "ts.references.script-and-markup",
  "js.references.script-and-markup",
  "ts.rename.from-script",
  "ts.rename.from-markup",
  "js.rename.from-script",
  "js.rename.from-markup",
  "ts.hover.typed-markup",
  "js.hover.typed-markup",
  "import.direct.sfc-tag-to-child",
  "import.direct.plain-ts-to-child",
  "import.direct.sfc-tag-hover.typed",
  "import.direct.plain-ts-hover.typed",
  "import.deep-barrel.sfc-tag-to-child",
  "import.deep-barrel.plain-ts-to-child",
  "import.deep-barrel.sfc-tag-hover.typed",
  "import.deep-barrel.plain-ts-hover.typed",
  "import.deep-barrel.public-type.clean-diagnostics",
  "ctrl-click.markup-to-script",
] as const;

export type FrameworkContractCapability = (typeof FRAMEWORK_CONTRACT_CAPABILITIES)[number];
export type ContractFramework = "vue" | "svelte";

export function frameworkContractId(
  framework: ContractFramework,
  capability: FrameworkContractCapability,
): string {
  return `${framework}.${capability}`;
}

export function requiredFrameworkContractIds(framework: ContractFramework): string[] {
  return FRAMEWORK_CONTRACT_CAPABILITIES.map((capability) =>
    frameworkContractId(framework, capability),
  );
}
