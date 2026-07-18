/**
 * Scenario 2 — heavy-update loops.
 *
 * N hundred edit→query cycles against one component: rename a member, add and
 * remove a prop, break and fix syntax. After every edit the response MUST
 * reflect the current content (renamed member resolves under its new name, a
 * removed prop disappears from completion, broken syntax degrades gracefully
 * — answered, not errored/dropped — and recovers after the fix). The provider
 * must stay alive and answer every request throughout.
 */
import type { EnduranceReceipt } from "../types.js";
import type { WorkspaceFiles } from "../workspace.js";
import { childConsumerContent, ENDURANCE_TSCONFIG, heavyUpdateChildContent } from "../workspace.js";
import {
  buildReceipt,
  convergeProbe,
  FailureBag,
  replaceOnce,
  type ScenarioContext,
} from "./common.js";

const CHILD = "src/Child.vue";
const APP = "src/App.vue";

export const HEAVY_UPDATE_FILES: WorkspaceFiles = {
  "tsconfig.json": ENDURANCE_TSCONFIG,
  [CHILD]: heavyUpdateChildContent(),
  [APP]: childConsumerContent(),
};

const INTERFACE_MEMBER = "  count?: number;";
const INTERFACE_MEMBER_BADGE = "  count?: number;\n  badge?: string;";
const ATTR_SITE = ':title="props.label"';
const ATTR_SITE_BADGE = ':title="props.label" :data-badge="props.badge"';
const PICK_INTACT = '  emit("select", props.label);\n}';
const PICK_BROKEN = '  emit("select", props.label);';

/** Shared rename loop used by the scale lane (word-boundary safe, overlay-only). */
export async function runRenameCycles(
  context: ScenarioContext,
  relativePath: string,
  ident: string,
  cycles: number,
  failures: FailureBag,
): Promise<void> {
  const boundary = new RegExp(`\\b${ident.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "g");
  for (let cycle = 0; cycle < cycles; cycle += 1) {
    const renamed = `${ident}__renamed${cycle}`;
    const current = context.session.textOf(relativePath);
    context.session.changeFile(relativePath, current.replace(boundary, renamed));
    await convergeProbe(
      context,
      {
        kind: "hover",
        relativePath,
        needle: renamed,
        occurrence: 1,
        cursorOffset: 2,
        expectIncludes: [renamed],
        label: `rename-cycle ${cycle} usage hover`,
      },
      failures,
    );
    const restored = context.session.textOf(relativePath);
    context.session.changeFile(
      relativePath,
      restored.replace(new RegExp(`\\b${renamed}\\b`, "g"), ident),
    );
  }
}

export async function runHeavyUpdateScenario(
  context: ScenarioContext,
  options: { cycles?: number } = {},
): Promise<EnduranceReceipt> {
  const failures = new FailureBag();
  const startedAtMs = Date.now();
  const { session } = context;
  const cycles = options.cycles ?? context.config.heavyUpdateCycles;
  context.sampler?.start();
  try {
    session.openFile(CHILD);
    session.openFile(APP);
    let buffer = heavyUpdateChildContent();
    const apply = (next: string): void => {
      session.changeFile(CHILD, next);
      buffer = next;
    };

    for (let cycle = 0; cycle < cycles; cycle += 1) {
      const from = cycle % 2 === 0 ? "greeting" : "salutation";
      const to = cycle % 2 === 0 ? "salutation" : "greeting";
      const usageNeedle = `{{ ${to} }}`;

      // 1. Rename the member; hover + definition must resolve the NEW name.
      apply(buffer.replaceAll(from, to));
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: CHILD,
          needle: usageNeedle,
          cursorOffset: 3,
          expectIncludes: [to],
          forbidIncludes: [from],
          label: `cycle ${cycle}: renamed member hover`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: CHILD,
          needle: usageNeedle,
          cursorOffset: 3,
          expectLineNeedle: `const ${to}`,
          label: `cycle ${cycle}: renamed member definition`,
        },
        failures,
      );

      // 2. Add a prop (+ usage); the NEW prop must appear in the PARENT's
      //    component attr-name completion (D1 stability contract — the
      //    Verter-native component-prop completion across files).
      apply(
        replaceOnce(
          replaceOnce(buffer, INTERFACE_MEMBER, INTERFACE_MEMBER_BADGE),
          ATTR_SITE,
          ATTR_SITE_BADGE,
        ),
      );
      await convergeProbe(
        context,
        {
          kind: "completion",
          relativePath: APP,
          needle: "<Child ",
          cursorOffset: "<Child ".length,
          expectLabels: ["label", "count", "badge"],
          label: `cycle ${cycle}: added prop appears in parent attr completion (D1)`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          // The member-access hover on the usage: provider member typing is a
          // documented gap — informational (settling asserted).
          kind: "hover",
          relativePath: CHILD,
          needle: "props.badge",
          cursorOffset: 6,
          expectIncludes: [],
          informational: true,
          label: `cycle ${cycle}: added prop hover`,
        },
        failures,
      );

      // 3. Remove it again; it must DISAPPEAR from the parent's component
      //    attr-name completion (D1), while the remaining props stay.
      apply(
        replaceOnce(
          replaceOnce(buffer, INTERFACE_MEMBER_BADGE, INTERFACE_MEMBER),
          ATTR_SITE_BADGE,
          ATTR_SITE,
        ),
      );
      await convergeProbe(
        context,
        {
          kind: "completion",
          relativePath: APP,
          needle: "<Child ",
          cursorOffset: "<Child ".length,
          expectLabels: ["label", "count"],
          forbidLabels: ["badge"],
          label: `cycle ${cycle}: removed prop disappears from parent attr completion (D1)`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          // Script member completion at the stable `props.` site: provider
          // member completion is a documented gap — informational.
          kind: "completion",
          relativePath: CHILD,
          needle: ':title="props.',
          cursorOffset: ':title="props.'.length,
          expectLabels: ["label", "count"],
          forbidLabels: ["badge"],
          informational: true,
          label: `cycle ${cycle}: removed prop disappears from member completion`,
        },
        failures,
      );

      // 4. Break syntax (drop pick()'s closing brace): graceful degradation —
      //    the hover must still SETTLE as answered (content unconstrained).
      apply(replaceOnce(buffer, PICK_INTACT, PICK_BROKEN));
      const broken = await session.runProbe({
        kind: "hover",
        relativePath: CHILD,
        needle: usageNeedle,
        cursorOffset: 3,
        expectIncludes: [],
        label: `cycle ${cycle}: broken-syntax hover settles`,
      });
      if (broken.classification !== "answered") {
        failures.add(
          `cycle ${cycle}: broken-syntax hover settled as ${broken.classification}, expected answered`,
        );
      }

      // 5. Restore; hover must reflect the current content again.
      apply(replaceOnce(buffer, PICK_BROKEN, PICK_INTACT));
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: CHILD,
          needle: usageNeedle,
          cursorOffset: 3,
          expectIncludes: [to],
          forbidIncludes: [from],
          label: `cycle ${cycle}: recovered hover`,
        },
        failures,
      );
    }

    return buildReceipt(context, startedAtMs, {
      finalSanityPass: null,
      failures: failures.list,
    });
  } finally {
    context.sampler?.stop();
  }
}
