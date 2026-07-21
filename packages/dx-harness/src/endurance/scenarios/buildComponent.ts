/** Build a component from scratch in every framework × language-mode lane. */
import type { EnduranceProbe } from "../session.js";
import { DEFAULT_ENDURANCE_LANE, type EnduranceLane, type EnduranceReceipt } from "../types.js";
import { carrierPath, ENDURANCE_TSCONFIG, type WorkspaceFiles } from "../workspace.js";
import {
  buildReceipt,
  convergeProbe,
  FailureBag,
  replaceOnce,
  typeFromScratch,
  typeInsertion,
  type ScenarioContext,
  type TypingCheckpoint,
} from "./common.js";

export interface BuildComponentFixture {
  readonly lane: EnduranceLane;
  readonly childPath: string;
  readonly parentPath: string;
  readonly childFinal: string;
  readonly parentFinal: string;
  readonly files: WorkspaceFiles;
  readonly childCheckpoints: readonly TypingCheckpoint[];
  readonly parentCheckpoints: readonly TypingCheckpoint[];
  readonly childMemberInsertion: string;
  readonly parentMemberInsertion: string;
  readonly parentMemberCheckpoints: readonly TypingCheckpoint[];
}

function afterMarker(text: string, marker: string): number {
  const index = text.indexOf(marker);
  if (index === -1) throw new Error(`marker not found in fixture: ${JSON.stringify(marker)}`);
  return index + marker.length;
}

function completionAtEnd(
  relativePath: string,
  typed: string,
  labels: readonly string[],
  label: string,
  informational = false,
): EnduranceProbe {
  return {
    kind: "completion",
    relativePath,
    needle: typed,
    cursorOffset: typed.length,
    expectLabels: labels,
    informational,
    label,
  };
}

function completionAtNeedle(
  relativePath: string,
  needle: string,
  labels: readonly string[],
  label: string,
  informational = false,
): EnduranceProbe {
  return {
    kind: "completion",
    relativePath,
    needle,
    cursorOffset: needle.length,
    expectLabels: labels,
    informational,
    label,
  };
}

function vueFixture(lane: EnduranceLane): BuildComponentFixture {
  const childPath = carrierPath(lane, "DraftCard");
  const parentPath = carrierPath(lane, "App");
  const scriptOpen =
    lane.mode === "ts" ? '<script setup lang="ts">' : "<script setup>\n// @ts-check";
  const props =
    lane.mode === "ts"
      ? [
          "interface DraftProps {",
          "  title: string;",
          "  level?: number;",
          "  unusedOnly?: boolean;",
          "}",
          "const props = defineProps<DraftProps>();",
          'const emit = defineEmits<{ (e: "save", title: string): void }>();',
          "defineSlots<{ active(props: { active: boolean }): any }>();",
        ]
      : [
          "const props = defineProps({",
          "  title: { type: String, required: true },",
          "  level: Number,",
          "  unusedOnly: Boolean,",
          "});",
          'const emit = defineEmits(["save"]);',
          "defineSlots();",
        ];
  const childFinal = [
    scriptOpen,
    ...props,
    "const draftLabel = `draft:${props.title}`;",
    "const draftLength = draftLabel.length;",
    "function saveDraft() {",
    '  emit("save", props.title);',
    "}",
    "</script>",
    "",
    "<template>",
    '  <section class="draft">',
    '    <h2 :data-level="props.level">{{ draftLabel }}</h2>',
    '    <button @click="saveDraft">Save</button>',
    '    <slot name="active" :active="true" :unused-slot-only="false" />',
    "  </section>",
    "</template>",
    "",
  ].join("\n");
  const parentFinal = [
    scriptOpen,
    `import DraftCard from "./DraftCard.${lane.framework}";`,
    'const heading = "drafts";',
    "const headingLength = heading.length;",
    lane.mode === "ts" ? "function onSave(title: string) {" : "function onSave(title) {",
    "  console.log(title.length);",
    "}",
    "</script>",
    "",
    "<template>",
    "  <main>",
    "    <h1>{{ heading }}</h1>",
    '    <DraftCard :title="heading" :level="1" @save="onSave">',
    '      <template #active="{ active }">',
    '        <span v-if="active">ready</span>',
    "      </template>",
    "    </DraftCard>",
    "  </main>",
    "</template>",
    "<!-- checkpoint-ready -->",
    "<!-- typing-tail -->",
    "<!-- post-checkpoint-tail -->",
    "",
  ].join("\n");
  const childMemberInsertion = "\nconst childMemberCheckpoint = draftLabel.length;";
  const parentMemberInsertion = "\nconst parentMemberCheckpoint = heading.length;";
  const childCheckpoints: TypingCheckpoint[] = [
    {
      atLength: afterMarker(childMemberInsertion, "draftLabel."),
      makeProbe: () =>
        completionAtNeedle(
          childPath,
          "childMemberCheckpoint = draftLabel.",
          ["length"],
          "vue local member completion during typing",
          true,
        ),
    },
    {
      atLength: afterMarker(childMemberInsertion, "draftLabel."),
      makeProbe: () => ({
        kind: "definition",
        relativePath: childPath,
        needle: "childMemberCheckpoint = draftLabel.",
        cursorOffset: "childMemberCheckpoint = ".length + 1,
        expectLineNeedle: "const draftLabel",
        label: "vue local definition during typing",
      }),
    },
  ];
  const parentCheckpoints: TypingCheckpoint[] = [
    {
      atLength: afterMarker(parentFinal, "<DraftCard "),
      makeProbe: (typed) =>
        completionAtEnd(
          parentPath,
          typed,
          ["unused-only"],
          "vue child props completion during typing",
        ),
    },
  ];
  const parentMemberCheckpoints: TypingCheckpoint[] = [
    {
      atLength: afterMarker(parentMemberInsertion, "heading."),
      makeProbe: () =>
        completionAtNeedle(
          parentPath,
          "parentMemberCheckpoint = heading.",
          ["length"],
          "vue script completion during typing",
          true,
        ),
    },
    {
      atLength: afterMarker(parentMemberInsertion, "heading."),
      makeProbe: () => ({
        kind: "definition",
        relativePath: parentPath,
        needle: "parentMemberCheckpoint = heading.",
        cursorOffset: "parentMemberCheckpoint = ".length + 1,
        expectLineNeedle: "const heading",
        label: "vue parent local definition during typing",
      }),
    },
  ];
  return {
    lane,
    childPath,
    parentPath,
    childFinal,
    parentFinal,
    files: {
      "tsconfig.json": ENDURANCE_TSCONFIG,
      [childPath]: childFinal,
      [parentPath]: parentFinal,
    },
    childCheckpoints,
    parentCheckpoints,
    childMemberInsertion,
    parentMemberInsertion,
    parentMemberCheckpoints,
  };
}

function svelteFixture(lane: EnduranceLane): BuildComponentFixture {
  const childPath = carrierPath(lane, "DraftCard");
  const parentPath = carrierPath(lane, "App");
  const scriptOpen = lane.mode === "ts" ? '<script lang="ts">' : "<script>\n  // @ts-check";
  const props =
    lane.mode === "ts"
      ? [
          '  import type { Snippet } from "svelte";',
          "  interface DraftProps {",
          "    title: string;",
          "    level?: number;",
          "    unusedOnly?: boolean;",
          "    onclick?: () => void;",
          "    children?: Snippet<[boolean]>;",
          "  }",
          "  let { title, level, onclick, children }: DraftProps = $props();",
        ]
      : [
          '  /** @typedef {import("svelte").Snippet<[boolean]>} DraftChildren */',
          "  /** @type {{ title: string, level?: number, unusedOnly?: boolean, onclick?: () => void, children?: DraftChildren }} */",
          "  let { title, level, onclick, children } = $props();",
        ];
  const childFinal = [
    scriptOpen,
    ...props,
    "  let draftLabel = $derived(`draft:${title}`);",
    "  const titleLength = title.length;",
    "  const levelValue = level;",
    "  const draftLength = draftLabel.length;",
    "  const childContent = children;",
    "  function saveDraft() {",
    "    onclick?.();",
    "  }",
    "  const saveDraftHandler = saveDraft;",
    "</script>",
    "",
    "{#snippet draftSnippet(active)}",
    "  <span>{active ? 'ready' : 'waiting'}</span>",
    "{/snippet}",
    '<section class="draft">',
    "  <h2 data-level={level}>{draftLabel}</h2>",
    "  <button onclick={saveDraft}>Save</button>",
    "  {@render draftSnippet(true)}",
    "  {@render children?.(true)}",
    "</section>",
    "",
  ].join("\n");
  const parentFinal = [
    scriptOpen,
    `  import DraftCard from "./DraftCard.${lane.framework}";`,
    '  let heading = $state("drafts");',
    lane.mode === "ts" ? "  function onSave(): void {" : "  function onSave() {",
    "    console.log(heading.length);",
    "  }",
    "  const onSaveHandler = onSave;",
    "</script>",
    "",
    "<main>",
    "  <h1>{heading}</h1>",
    "  <DraftCard title={heading} level={1} onclick={onSave}>",
    lane.mode === "ts"
      ? "    {#snippet children(active: boolean)}"
      : "    {#snippet children(active)}",
    "      <span>{active ? 'ready' : 'waiting'}</span>",
    "    {/snippet}",
    "  </DraftCard>",
    "</main>",
    "<!-- checkpoint-ready -->",
    "<!-- typing-tail -->",
    "<!-- post-checkpoint-tail -->",
    "",
  ].join("\n");
  const childMemberInsertion = "\n  const childMemberCheckpoint = draftLabel.length;";
  const parentMemberInsertion = "\n  const parentMemberCheckpoint = heading.length;";
  const childCheckpoints: TypingCheckpoint[] = [
    {
      atLength: afterMarker(childMemberInsertion, "draftLabel."),
      makeProbe: () =>
        completionAtNeedle(
          childPath,
          "childMemberCheckpoint = draftLabel.",
          ["length"],
          "svelte child completion during typing",
          true,
        ),
    },
    {
      atLength: afterMarker(childMemberInsertion, "draftLabel."),
      makeProbe: () => ({
        kind: "definition",
        relativePath: childPath,
        needle: "childMemberCheckpoint = draftLabel.",
        cursorOffset: "childMemberCheckpoint = ".length + 1,
        expectLineNeedle: "let draftLabel",
        label: "svelte local definition during typing",
      }),
    },
  ];
  const parentCheckpoints: TypingCheckpoint[] = [
    {
      atLength: afterMarker(parentFinal, "<DraftCard "),
      makeProbe: (typed) =>
        completionAtEnd(
          parentPath,
          typed,
          ["unusedOnly"],
          "svelte child props completion during typing",
        ),
    },
  ];
  const parentMemberCheckpoints: TypingCheckpoint[] = [
    {
      atLength: afterMarker(parentMemberInsertion, "heading."),
      makeProbe: () =>
        completionAtNeedle(
          parentPath,
          "parentMemberCheckpoint = heading.",
          ["length"],
          "svelte parent completion during typing",
          true,
        ),
    },
    {
      atLength: afterMarker(parentMemberInsertion, "heading."),
      makeProbe: () => ({
        kind: "definition",
        relativePath: parentPath,
        needle: "parentMemberCheckpoint = heading.",
        cursorOffset: "parentMemberCheckpoint = ".length + 1,
        expectLineNeedle: "let heading",
        label: "svelte parent local definition during typing",
      }),
    },
  ];
  return {
    lane,
    childPath,
    parentPath,
    childFinal,
    parentFinal,
    files: {
      "tsconfig.json": ENDURANCE_TSCONFIG,
      [childPath]: childFinal,
      [parentPath]: parentFinal,
    },
    childCheckpoints,
    parentCheckpoints,
    childMemberInsertion,
    parentMemberInsertion,
    parentMemberCheckpoints,
  };
}

export function buildComponentFixture(
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): BuildComponentFixture {
  return lane.framework === "vue" ? vueFixture(lane) : svelteFixture(lane);
}

/** Hard probes proving that the parent consumes the child's typed slot/snippet contract. */
export function buildComponentIntegrationProbes(fixture: BuildComponentFixture): EnduranceProbe[] {
  const isVue = fixture.lane.framework === "vue";
  const activeUse = isVue ? 'v-if="active"' : "active ? 'ready'";
  const activeDeclaration = isVue
    ? '<template #active="{ active }">'
    : fixture.lane.mode === "ts"
      ? "{#snippet children(active: boolean)}"
      : "{#snippet children(active)}";
  const probes: EnduranceProbe[] = isVue
    ? [
        {
          kind: "definition",
          relativePath: fixture.parentPath,
          needle: "#active",
          cursorOffset: 2,
          expectUriSuffix: `/${fixture.childPath}`,
          expectLineNeedle:
            fixture.lane.mode === "ts" ? "defineSlots<{ active" : '<slot name="active"',
          label: `${fixture.lane.id} parent active slot-name mapped definition`,
        },
      ]
    : [
        {
          kind: "definition",
          relativePath: fixture.parentPath,
          needle: activeUse,
          cursorOffset: 2,
          expectLineNeedle: activeDeclaration,
          label: `${fixture.lane.id} parent children snippet active definition`,
        },
      ];
  if (fixture.lane.mode === "ts" && isVue) {
    probes.push(
      {
        kind: "definition",
        relativePath: fixture.parentPath,
        needle: '#active="{ active }"',
        cursorOffset: '#active="{ '.length + 1,
        expectUriSuffix: `/${fixture.childPath}`,
        expectLineNeedle: "active(props: { active: boolean })",
        label: `${fixture.lane.id} parent scoped-slot active mapped definition`,
      },
      {
        kind: "hover",
        relativePath: fixture.childPath,
        needle: "active: boolean",
        cursorOffset: 2,
        expectIncludes: ["active", "boolean"],
        forbidIncludes: ["any"],
        requireNonEmpty: true,
        label: `${fixture.lane.id} child scoped-slot typed active hover`,
      },
    );
  }
  if (fixture.lane.mode === "ts") {
    probes.push({
      kind: "hover",
      relativePath: fixture.parentPath,
      needle: activeUse,
      cursorOffset: isVue ? 'v-if="'.length + 1 : 2,
      expectIncludes: isVue ? [] : ["active", "boolean"],
      ...(isVue
        ? { informational: true as const }
        : { forbidIncludes: ["any"], requireNonEmpty: true }),
      label: `${fixture.lane.id} parent ${isVue ? "scoped-slot" : "children snippet"} typed active hover`,
    });
  }
  if (!isVue) {
    probes.push({
      kind: "definition",
      relativePath: fixture.childPath,
      needle: "@render children?.(true)",
      cursorOffset: "@render ".length + 1,
      expectLineNeedle: "let { title, level, onclick, children }",
      label: `${fixture.lane.id} child incoming children render definition`,
    });
  }
  return probes;
}

/** Hard navigation from authored markup event sites to their script handlers. */
export function buildComponentEventSiteProbes(
  fixture: BuildComponentFixture,
): [EnduranceProbe, EnduranceProbe] {
  if (fixture.lane.framework === "svelte") {
    return [
      {
        kind: "definition",
        relativePath: fixture.childPath,
        needle: "onclick={saveDraft}",
        cursorOffset: "onclick={".length + 1,
        expectLineNeedle: "function saveDraft",
        label: `${fixture.lane.id} child markup event-site definition`,
      },
      {
        kind: "definition",
        relativePath: fixture.parentPath,
        needle: "onclick={onSave}",
        cursorOffset: "onclick={".length + 1,
        expectLineNeedle: "function onSave",
        label: `${fixture.lane.id} component markup event-site definition`,
      },
    ];
  }
  return [
    {
      kind: "definition",
      relativePath: fixture.childPath,
      needle: '@click="saveDraft"',
      cursorOffset: '@click="'.length + 1,
      expectLineNeedle: "function saveDraft",
      label: `${fixture.lane.id} child markup event-site definition`,
    },
    {
      kind: "definition",
      relativePath: fixture.parentPath,
      needle: '@save="onSave"',
      cursorOffset: '@save="'.length + 1,
      expectLineNeedle: "function onSave",
      label: `${fixture.lane.id} component markup event-site definition`,
    },
  ];
}

export const BUILD_COMPONENT_FILES: WorkspaceFiles = buildComponentFixture().files;

export async function runBuildComponentScenario(
  context: ScenarioContext,
  fixture: BuildComponentFixture = buildComponentFixture(context.lane),
): Promise<EnduranceReceipt> {
  const failures = new FailureBag();
  const startedAtMs = Date.now();
  const { session } = context;
  context.sampler?.start();
  try {
    session.openFile(fixture.childPath, "");
    await typeFromScratch(context, fixture.childPath, fixture.childFinal, [], failures);
    await typeInsertion(
      context,
      fixture.childPath,
      "</script>",
      fixture.childMemberInsertion,
      fixture.childCheckpoints,
      failures,
    );

    const localNeedle = fixture.lane.framework === "vue" ? "{{ draftLabel }}" : "draftLabel.length";
    await convergeProbe(
      context,
      {
        // Framework-local hover: the binding NAME is Verter-owned in every lane
        // (hard + non-empty + name fragment). The TYPE TEXT at a template-mapped
        // position is provider-owned and truthfully surfaces `any` on the
        // tsserver route today (the documented provider type-quality gap), so no
        // type fragment is forbidden there; the Svelte probe uses a script
        // position whose typed answer DOES forbid `any`.
        kind: "hover",
        relativePath: fixture.childPath,
        needle: localNeedle,
        cursorOffset: fixture.lane.framework === "vue" ? 4 : 2,
        expectIncludes: ["draftLabel"],
        forbidIncludes: fixture.lane.framework === "vue" ? [] : ["any"],
        requireNonEmpty: true,
        label: `${fixture.lane.id} child local hover`,
      },
      failures,
    );
    await convergeProbe(
      context,
      {
        kind: "definition",
        relativePath: fixture.childPath,
        needle: localNeedle,
        cursorOffset: fixture.lane.framework === "vue" ? 4 : 2,
        expectLineNeedle: fixture.lane.framework === "vue" ? "const draftLabel" : "let draftLabel",
        label: `${fixture.lane.id} child local definition`,
      },
      failures,
    );

    if (fixture.lane.framework === "svelte") {
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: fixture.childPath,
          needle: "title.length",
          cursorOffset: 2,
          expectIncludes: ["title"],
          ...(fixture.lane.mode === "js"
            ? { informational: true as const }
            : { forbidIncludes: ["any"], requireNonEmpty: true }),
          label: `${fixture.lane.id} typed data prop hover`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: fixture.childPath,
          needle: "title.length",
          cursorOffset: 2,
          expectLineNeedle: "let { title, level, onclick, children }",
          label: `${fixture.lane.id} typed data prop definition`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "completion",
          relativePath: fixture.childPath,
          needle: "title.length",
          cursorOffset: "title.".length,
          expectLabels: ["length"],
          label: `${fixture.lane.id} typed data prop completion`,
        },
        failures,
      );
    }

    const eventSiteProbes = buildComponentEventSiteProbes(fixture);
    await convergeProbe(context, eventSiteProbes[0], failures);

    if (fixture.lane.framework === "svelte") {
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: fixture.childPath,
          needle: "onclick?.()",
          cursorOffset: 1,
          expectLineNeedle: "let { title, level, onclick, children }",
          label: `${fixture.lane.id} callback-event prop definition`,
        },
        failures,
      );
      if (fixture.lane.mode === "ts") {
        await convergeProbe(
          context,
          {
            kind: "hover",
            relativePath: fixture.childPath,
            needle: "Snippet<[boolean]>",
            cursorOffset: 2,
            expectIncludes: ["Snippet"],
            forbidIncludes: ["any"],
            requireNonEmpty: true,
            label: `${fixture.lane.id} typed children Snippet type hover`,
          },
          failures,
        );
      }
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: fixture.childPath,
          needle: "const childContent = children",
          cursorOffset: "const childContent = ".length + 1,
          expectLineNeedle: "let { title, level, onclick, children }",
          label: `${fixture.lane.id} typed children Snippet definition`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: fixture.childPath,
          needle: "@render draftSnippet",
          cursorOffset: "@render ".length + 1,
          expectLineNeedle: "{#snippet draftSnippet",
          label: `${fixture.lane.id} snippet render definition`,
        },
        failures,
      );
    }

    const insertion =
      fixture.lane.framework === "vue"
        ? '\n    <p class="note">{{ props.title }}</p>'
        : '\n  <p class="note">{title}</p>';
    await typeInsertion(context, fixture.childPath, "</section>", insertion, [], failures);

    session.openFile(fixture.parentPath, "");
    await typeFromScratch(
      context,
      fixture.parentPath,
      fixture.parentFinal,
      fixture.parentCheckpoints,
      failures,
    );
    await typeInsertion(
      context,
      fixture.parentPath,
      "</script>",
      fixture.parentMemberInsertion,
      fixture.parentMemberCheckpoints,
      failures,
    );
    await convergeProbe(
      context,
      {
        kind: "definition",
        relativePath: fixture.parentPath,
        needle: "<DraftCard",
        cursorOffset: 2,
        expectUriSuffix: `/DraftCard.${fixture.lane.framework}`,
        label: `${fixture.lane.id} component definition`,
      },
      failures,
    );

    await convergeProbe(context, eventSiteProbes[1], failures);

    if (fixture.lane.framework === "vue") {
      // Parent interpolation local hover: the binding NAME is Verter-owned
      // (hard + non-empty); the TYPE TEXT at the template-mapped position is
      // provider-owned and truthfully surfaces `any` on the tsserver route
      // today (documented provider type-quality gap), so none is forbidden.
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: fixture.parentPath,
          needle: "{{ heading }}",
          cursorOffset: 4,
          expectIncludes: ["heading"],
          forbidIncludes: [],
          requireNonEmpty: true,
          label: `${fixture.lane.id} parent interpolation local hover`,
        },
        failures,
      );
      // Component-tag hover: Verter owns a native answer here — it must be
      // non-empty (the type text itself is the provider's business, so no
      // fragment is forbidden).
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: fixture.parentPath,
          needle: "<DraftCard",
          cursorOffset: 2,
          expectIncludes: [],
          forbidIncludes: [],
          requireNonEmpty: true,
          label: `${fixture.lane.id} component tag hover answers`,
        },
        failures,
      );
      // Attr expression → parent script local navigation (Verter-owned mapping).
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: fixture.parentPath,
          needle: ':title="heading"',
          cursorOffset: 9,
          expectLineNeedle: "const heading",
          label: `${fixture.lane.id} attr expression to parent script local`,
        },
        failures,
      );
    }

    if (fixture.lane.framework === "vue") {
      const full = '    <DraftCard :title="heading" :level="1" @save="onSave">';
      const partial = "    <DraftCard :>";
      session.changeFile(
        fixture.parentPath,
        replaceOnce(session.textOf(fixture.parentPath), full, partial),
      );
      await convergeProbe(
        context,
        {
          kind: "completion",
          relativePath: fixture.parentPath,
          needle: "<DraftCard :>",
          cursorOffset: "<DraftCard :".length,
          expectLabels: ["unused-only"],
          label: `${fixture.lane.id} bind prop completion`,
        },
        failures,
      );
      session.changeFile(
        fixture.parentPath,
        replaceOnce(session.textOf(fixture.parentPath), partial, full),
      );
    }

    for (const probe of buildComponentIntegrationProbes(fixture)) {
      await convergeProbe(context, probe, failures);
    }

    return buildReceipt(context, startedAtMs, { finalSanityPass: null, failures: failures.list });
  } finally {
    context.sampler?.stop();
  }
}
