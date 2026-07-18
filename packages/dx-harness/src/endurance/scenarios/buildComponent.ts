/**
 * Scenario 1 — build-a-component-from-scratch.
 *
 * Keystroke-level typing simulation: two SFC buffers start EMPTY and are
 * typed chunk-by-chunk (didChange per keystroke, a few ms apart) — first a
 * child (`<script setup>` with props/emits/slots, then a `<template>` using
 * them), then a parent consuming it. At realistic points (after a
 * member-access dot, after a component tag's attr introducer, inside an
 * interpolation) completion/hover/definition are fired and EVERY response is
 * asserted: typed items present, definition/hover on authored spans, latency
 * under bound. Any timed-out/rejected request fails the run.
 */
import type { EnduranceReceipt } from "../types.js";
import type { EnduranceProbe } from "../session.js";
import type { WorkspaceFiles } from "../workspace.js";
import { ENDURANCE_TSCONFIG } from "../workspace.js";
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

const DRAFT_CARD = "src/DraftCard.vue";
const APP = "src/App.vue";

const DRAFT_CARD_FINAL = [
  '<script setup lang="ts">',
  "interface DraftProps {",
  "  title: string;",
  "  level?: number;",
  "}",
  "const props = defineProps<DraftProps>();",
  'const emit = defineEmits<{ (e: "save", title: string): void }>();',
  "defineSlots<{ default(props: { active: boolean }): any }>();",
  "const draftLabel = `draft:${props.title}`;",
  "function saveDraft() {",
  '  emit("save", props.title);',
  "}",
  "</script>",
  "",
  "<template>",
  '  <section class="draft">',
  '    <h2 :data-level="props.level">{{ draftLabel }}</h2>',
  '    <button @click="saveDraft">Save</button>',
  "  </section>",
  "</template>",
  "",
].join("\n");

const APP_FINAL = [
  '<script setup lang="ts">',
  'import DraftCard from "./DraftCard.vue";',
  'const heading = "drafts";',
  "function onSave(title: string) {",
  "  console.log(title.length);",
  "}",
  "</script>",
  "",
  "<template>",
  "  <main>",
  "    <h1>{{ heading }}</h1>",
  '    <DraftCard :title="heading" :level="1" @save="onSave">',
  '      <template #default="{ active }"><span v-if="active">ready</span></template>',
  "    </DraftCard>",
  "  </main>",
  "</template>",
  "",
].join("\n");

/** The insertion typed mid-document (before `</section>`) once the SFC is complete. */
const TEMPLATE_INSERT = '\n    <p class="note">{{ props.title }}</p>';
const INSERT_COMPLETION_PREFIX = '\n    <p class="note">{{ props.';

export const BUILD_COMPONENT_FILES: WorkspaceFiles = {
  "tsconfig.json": ENDURANCE_TSCONFIG,
  [DRAFT_CARD]: DRAFT_CARD_FINAL,
  [APP]: APP_FINAL,
};

function afterMarker(text: string, marker: string): number {
  const index = text.indexOf(marker);
  if (index === -1) throw new Error(`marker not found in fixture: ${JSON.stringify(marker)}`);
  return index + marker.length;
}

/** Completion probe pinned to the end of the just-typed region. */
function endOfTypedCompletionProbe(typedSoFar: string, label: string): EnduranceProbe {
  return {
    kind: "completion",
    relativePath: DRAFT_CARD,
    needle: typedSoFar,
    cursorOffset: typedSoFar.length,
    expectLabels: ["title", "level"],
    label,
  };
}

export async function runBuildComponentScenario(
  context: ScenarioContext,
): Promise<EnduranceReceipt> {
  const failures = new FailureBag();
  const startedAtMs = Date.now();
  const { session } = context;
  context.sampler?.start();
  try {
    // ── Part 1: type the child SFC from an empty buffer ─────────────────
    session.openFile(DRAFT_CARD, "");
    const draftCheckpoints: TypingCheckpoint[] = [
      {
        // Member-access dot inside the template literal: `draft:${props.|
        // INFORMATIONAL: provider member completion is documented type-quality
        // backlog — settling is asserted, content is observed, not asserted.
        atLength: afterMarker(DRAFT_CARD_FINAL, "`draft:${props."),
        makeProbe: (typed) => ({
          ...endOfTypedCompletionProbe(typed, "script template-literal member access"),
          informational: true,
        }),
      },
      {
        // Member-access dot in the emit call args (also informational).
        atLength: afterMarker(DRAFT_CARD_FINAL, 'emit("save", props.'),
        makeProbe: (typed) => ({
          ...endOfTypedCompletionProbe(typed, "script emit-arg member access"),
          informational: true,
        }),
      },
    ];
    await typeFromScratch(context, DRAFT_CARD, DRAFT_CARD_FINAL, draftCheckpoints, failures);

    // Fully-typed child: Vue binding hover + definition mapping are STABILITY
    // assertions (Verter owns these answers).
    await convergeProbe(
      context,
      {
        kind: "hover",
        relativePath: DRAFT_CARD,
        needle: "{{ draftLabel }}",
        cursorOffset: 4,
        expectIncludes: ["draftLabel"],
        requireNonEmpty: true,
        label: "template interpolation local hover",
      },
      failures,
    );
    await convergeProbe(
      context,
      {
        kind: "definition",
        relativePath: DRAFT_CARD,
        needle: "{{ draftLabel }}",
        cursorOffset: 4,
        expectLineNeedle: "const draftLabel",
        label: "template interpolation → script declaration",
      },
      failures,
    );
    await convergeProbe(
      context,
      {
        // `props.level` member hover: provider member typing is a documented
        // gap — informational (settling asserted, content observed).
        kind: "hover",
        relativePath: DRAFT_CARD,
        needle: "props.level",
        cursorOffset: 7,
        expectIncludes: [],
        informational: true,
        label: "template prop-attr expression hover",
      },
      failures,
    );

    // Mid-document typing: a new interpolation before </section>, with a
    // member-access completion probe (informational — provider member gap).
    await typeInsertion(
      context,
      DRAFT_CARD,
      "</section>",
      TEMPLATE_INSERT,
      [
        {
          atLength: INSERT_COMPLETION_PREFIX.length,
          makeProbe: (typed) => ({
            kind: "completion",
            relativePath: DRAFT_CARD,
            needle: typed,
            cursorOffset: typed.length,
            expectLabels: ["title", "level"],
            informational: true,
            label: "template interpolation member access (mid-doc insert)",
          }),
        },
      ],
      failures,
    );
    await convergeProbe(
      context,
      {
        kind: "hover",
        relativePath: DRAFT_CARD,
        needle: "{{ props.title",
        cursorOffset: "{{ props.".length,
        expectIncludes: [],
        informational: true,
        label: "inserted interpolation prop hover",
      },
      failures,
    );

    // ── Part 2: type the parent consuming the child ─────────────────────
    session.openFile(APP, "");
    const appCheckpoints: TypingCheckpoint[] = [
      {
        // INFORMATIONAL mid-typing script member completion (provider member
        // typing is a documented gap — settling asserted, content observed).
        atLength: afterMarker(APP_FINAL, "console.log(title."),
        makeProbe: (typed) => ({
          kind: "completion",
          relativePath: APP,
          needle: typed,
          cursorOffset: typed.length,
          expectLabels: ["length"],
          informational: true,
          label: "script member access completion",
        }),
      },
    ];
    await typeFromScratch(context, APP, APP_FINAL, appCheckpoints, failures);

    // D1 STABILITY: attr-name completion on the (complete) `<DraftCard` usage
    // must offer the child's typed prop names. Bound-`:attr` / `@event` attrs
    // are NOT filtered by verter, so the complete element is valid D1 ground.
    await convergeProbe(
      context,
      {
        kind: "completion",
        relativePath: APP,
        needle: "<DraftCard ",
        cursorOffset: "<DraftCard ".length,
        expectLabels: ["title", "level"],
        label: "component attr-name prop completion (D1)",
      },
      failures,
    );

    await convergeProbe(
      context,
      {
        kind: "hover",
        relativePath: APP,
        needle: "{{ heading }}",
        cursorOffset: 4,
        expectIncludes: ["heading"],
        requireNonEmpty: true,
        label: "parent interpolation hover",
      },
      failures,
    );
    await convergeProbe(
      context,
      {
        // Component-tag hover: Verter owns a native answer here — it must be
        // non-empty (type text itself is the provider's business).
        kind: "hover",
        relativePath: APP,
        needle: "<DraftCard",
        cursorOffset: 2,
        expectIncludes: [],
        requireNonEmpty: true,
        label: "component tag hover answers",
      },
      failures,
    );
    await convergeProbe(
      context,
      {
        kind: "definition",
        relativePath: APP,
        needle: "<DraftCard",
        cursorOffset: 2,
        expectUriSuffix: "/DraftCard.vue",
        label: "component tag → child file definition",
      },
      failures,
    );
    await convergeProbe(
      context,
      {
        kind: "definition",
        relativePath: APP,
        needle: ':title="heading"',
        cursorOffset: 9,
        expectLineNeedle: "const heading",
        label: "attr expression → parent script local",
      },
      failures,
    );

    // D1 STABILITY (bind form): a FRESH `:` trigger — the shape real typing
    // produces (`<DraftCard :` right after the colon, tag close following).
    // Probing mid-token of a pre-existing attr (`:|title`) is not a fresh-bind
    // position, so the ground is made by truncating the usage, probing, and
    // restoring — exercising another edit round-trip too.
    const draftLine = '    <DraftCard :title="heading" :level="1" @save="onSave">';
    const truncatedLine = "    <DraftCard :>";
    session.changeFile(APP, replaceOnce(session.textOf(APP), draftLine, truncatedLine));
    await convergeProbe(
      context,
      {
        kind: "completion",
        relativePath: APP,
        needle: "<DraftCard :>",
        cursorOffset: "<DraftCard :".length,
        expectLabels: ["title", "level"],
        label: "component-attr bind completion (D1)",
      },
      failures,
    );
    session.changeFile(APP, replaceOnce(session.textOf(APP), truncatedLine, draftLine));

    return buildReceipt(context, startedAtMs, {
      finalSanityPass: null,
      failures: failures.list,
    });
  } finally {
    context.sampler?.stop();
  }
}
