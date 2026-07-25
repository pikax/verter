export const FRAMEWORK_CONTRACT_CAPABILITIES = [
  "ts.clean-diagnostics",
  "js.clean-diagnostics",
  "ts.definition.markup-to-script",
  "js.definition.markup-to-script",
  "ts.definition.markup-to-script.exact-stable-warm",
  "js.definition.markup-to-script.exact-stable-warm",
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
  "import.direct.public-prop-definition.exact-stable-warm",
  "import.direct.sfc-tag-hover.typed",
  "import.direct.plain-ts-hover.typed",
  "import.deep-barrel.sfc-tag-to-child",
  "import.deep-barrel.plain-ts-to-child",
  "import.deep-barrel.public-prop-definition.exact-stable-warm",
  "import.deep-barrel.sfc-tag-hover.typed",
  "import.deep-barrel.plain-ts-hover.typed",
  "import.deep-barrel.public-type.clean-diagnostics",
  "ctrl-click.markup-to-script",
  // Event-handler expression values are a distinct region of the generated TS surface from
  // interpolations; every provider-backed feature must behave the same in both.
  "ts.definition.event-handler-to-script",
  "ts.references.event-handler",
  "ts.rename.from-event-handler",
  "ts.hover.event-handler",
  "ctrl-click.event-handler-to-script",
  // Completion is what a user hits on every keystroke, so a gap is felt constantly.
  // `answers-every-gesture` is the survey control: it fails the moment ANY probed
  // gesture stops producing completions, which per-label assertions cannot see —
  // a source returning nothing has no labels to be wrong about.
  "completion.answers-every-gesture",
  "completion.probe-file.clean-diagnostics",
] as const;

export type FrameworkContractCapability = (typeof FRAMEWORK_CONTRACT_CAPABILITIES)[number];
export type ContractFramework = "vue" | "svelte";

/**
 * The completion gestures whose CONTENT is asserted, per framework.
 *
 * Per-framework rather than shared because the gestures genuinely differ — Vue's
 * `v-slot` scope and `<component :is>` have no Svelte counterpart, and Svelte's `{#each}`
 * has no Vue one. Forcing a shared list would mean inventing an equivalence that does not
 * exist, and the surveyed-probe list already covers both frameworks uniformly.
 *
 * A gesture enters this list only once its content is correct; the surveyed-but-not-yet
 * asserted gestures are recorded as defects in the plan document. The registration
 * asserts this list and the descriptor's `assertedCompletions` keys match exactly, so a
 * gesture cannot be silently dropped from the required set.
 */
export const FRAMEWORK_ASSERTED_COMPLETIONS: Readonly<
  Record<ContractFramework, readonly string[]>
> = {
  vue: ["component-event", "member-in-directive", "slot-scope", "dynamic-is", "script-member"],
  svelte: ["member-in-markup", "script-member"],
};

export function completionContractId(framework: ContractFramework, probeId: string): string {
  return `${framework}.completion.${probeId}`;
}

export function frameworkContractId(
  framework: ContractFramework,
  capability: FrameworkContractCapability,
): string {
  return `${framework}.${capability}`;
}

export function requiredFrameworkContractIds(framework: ContractFramework): string[] {
  return [
    ...FRAMEWORK_CONTRACT_CAPABILITIES.map((capability) =>
      frameworkContractId(framework, capability),
    ),
    ...FRAMEWORK_ASSERTED_COMPLETIONS[framework].map((probeId) =>
      completionContractId(framework, probeId),
    ),
  ];
}
