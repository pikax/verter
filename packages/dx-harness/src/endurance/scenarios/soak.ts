/**
 * Scenario 4 — soak.
 *
 * Sustained mixed workload for a configurable duration: a typer repeatedly
 * builds a small SFC from scratch in a scratch buffer while hover /
 * completion / definition workers cycle across stable carrier files. Asserts
 * no meaningful p95 degradation (failure requires both a factor breach and a
 * configured absolute-delta floor), the provider stays alive, ZERO unanswered requests, RSS
 * under the configured ceiling, and a final full-feature sanity pass (hover +
 * completion + definition correct on known positions) after the soak.
 */
import { DEFAULT_ENDURANCE_LANE, type EnduranceLane, type EnduranceReceipt } from "../types.js";
import type { EnduranceProbe } from "../session.js";
import { sleep } from "../metrics.js";
import type { WorkspaceFiles } from "../workspace.js";
import {
  buildCarrierSet,
  carrierPath,
  childConsumerContent,
  ENDURANCE_TSCONFIG,
  heavyUpdateChildContent,
} from "../workspace.js";
import { carrierStormProbes } from "./storm.js";
import {
  buildReceipt,
  convergeProbe,
  FailureBag,
  typeFromScratch,
  type ScenarioContext,
} from "./common.js";

function soakTypedDocument(lane: EnduranceLane): string {
  if (lane.framework === "svelte") {
    return [
      lane.mode === "ts" ? '<script lang="ts">' : "<script>\n  // @ts-check",
      lane.mode === "ts"
        ? "  let { note }: { note: string } = $props();"
        : "  /** @type {{ note: string }} */\n  let { note } = $props();",
      "  let scratchLocal = $derived(`s:${note}`);",
      "</script>",
      "<div>{scratchLocal}</div>",
      "",
    ].join("\n");
  }
  return [
    lane.mode === "ts" ? '<script setup lang="ts">' : "<script setup>\n// @ts-check",
    lane.mode === "ts"
      ? "const props = defineProps<{ note: string }>();"
      : "const props = defineProps({ note: { type: String, required: true } });",
    "const scratchLocal = `s:${props.note}`;",
    "</script>",
    "",
    "<template>",
    "  <div>{{ scratchLocal }}</div>",
    "</template>",
    "",
  ].join("\n");
}

const DEFAULT_SCRATCH = carrierPath(DEFAULT_ENDURANCE_LANE, "Scratch");
export const SOAK_SCRATCH_PATH = DEFAULT_SCRATCH;
export const SOAK_TYPED_DOC = soakTypedDocument(DEFAULT_ENDURANCE_LANE);

export interface SoakWorkspace {
  readonly files: WorkspaceFiles;
  readonly carriers: readonly string[];
  readonly lane: EnduranceLane;
  readonly childPath: string;
  readonly appPath: string;
  readonly scratchPath: string;
  readonly typedDocument: string;
}

export function soakWorkspace(
  carrierCount = 4,
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): SoakWorkspace {
  const carrierSet = buildCarrierSet(carrierCount, lane);
  const childPath = carrierPath(lane, "Child");
  const appPath = carrierPath(lane, "App");
  const scratchPath = carrierPath(lane, "Scratch");
  const typedDocument = soakTypedDocument(lane);
  return {
    files: {
      "tsconfig.json": ENDURANCE_TSCONFIG,
      [childPath]: heavyUpdateChildContent(lane),
      [appPath]: childConsumerContent(lane),
      [scratchPath]: typedDocument,
      ...Object.fromEntries(
        Object.entries(carrierSet.files).filter(([key]) => key !== "tsconfig.json"),
      ),
    },
    carriers: carrierSet.carriers,
    lane,
    childPath,
    appPath,
    scratchPath,
    typedDocument,
  };
}

/** Stable, content-checked soak probes across Child/App/carriers. */
export function soakProbes(
  carriers: readonly string[],
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): EnduranceProbe[] {
  const childPath = carrierPath(lane, "Child");
  const appPath = carrierPath(lane, "App");
  const childLocal = lane.framework === "vue" ? "{{ greeting }}" : "greeting.length";
  const appLocal = lane.framework === "vue" ? "{{ heading }}" : "heading.length";
  const probes: EnduranceProbe[] = [
    {
      // Framework-local hover: the binding NAME is Verter-owned in every lane
      // (hard + non-empty + name fragment). The TYPE TEXT at a template-mapped
      // position is provider-owned and truthfully surfaces `any` on the
      // tsserver route today (documented provider type-quality gap), so no
      // type fragment is forbidden there; the Svelte probe uses a script
      // position whose typed answer DOES forbid `any`.
      kind: "hover",
      relativePath: childPath,
      needle: childLocal,
      cursorOffset: lane.framework === "vue" ? 3 : 2,
      expectIncludes: ["greeting"],
      forbidIncludes: lane.framework === "vue" ? [] : ["any"],
      requireNonEmpty: true,
      label: "child local hover",
    },
    {
      // Script member completion: documented provider type-quality gap —
      // informational (settling asserted, emptiness recorded in typeQuality).
      kind: "completion",
      relativePath: childPath,
      needle: lane.framework === "vue" ? ':title="props.' : "title={label",
      cursorOffset: lane.framework === "vue" ? ':title="props.'.length : "title={label".length,
      expectLabels: ["label", "count"],
      informational: true,
      label: "child props member completion",
    },
    {
      kind: "hover",
      relativePath: appPath,
      needle: appLocal,
      cursorOffset: lane.framework === "vue" ? 3 : 2,
      expectIncludes: ["heading"],
      forbidIncludes: lane.framework === "vue" ? [] : ["any"],
      requireNonEmpty: true,
      label: "app local hover",
    },
    // Carriers 1..N-1 (soak's typer targets only the framework-neutral Scratch file, so every carrier
    // is a stable probe target; carrierStormProbes maps index→name correctly
    // only on the FULL carriers array).
    ...carrierStormProbes(carriers, lane),
  ];
  if (lane.framework === "vue") {
    probes.push(
      {
        kind: "definition",
        relativePath: childPath,
        needle: childLocal,
        cursorOffset: 3,
        expectLineNeedle: "const greeting",
        label: "child local definition",
      },
      {
        kind: "definition",
        relativePath: appPath,
        needle: appLocal,
        cursorOffset: 3,
        expectLineNeedle: "const heading",
        label: "app local definition",
      },
    );
  }
  return probes;
}

export interface SoakParams {
  readonly probes: readonly EnduranceProbe[];
  /** Buffer the typer re-types from scratch on a loop. */
  readonly typingFile?: { relativePath: string; typedText: string };
  readonly durationMs?: number;
  /** Query worker count (default 3). */
  readonly queryWorkers?: number;
}

export async function runSoakScenario(
  context: ScenarioContext,
  params: SoakParams,
): Promise<EnduranceReceipt> {
  const failures = new FailureBag();
  const startedAtMs = Date.now();
  const { session } = context;
  const durationMs = params.durationMs ?? context.config.soakDurationMs;
  const queryWorkers = params.queryWorkers ?? 3;
  if (params.probes.length === 0) throw new Error("soak requires at least one probe");
  context.sampler?.start();
  try {
    // Calibration: every probe correct BEFORE the measured window.
    for (const probe of params.probes) {
      await convergeProbe(context, probe, failures);
    }

    const deadline = Date.now() + durationMs;
    let cursor = 0;
    const queryWorker = async (): Promise<void> => {
      while (Date.now() < deadline) {
        const probe = params.probes[cursor % params.probes.length];
        cursor += 1;
        try {
          const outcome = await session.runProbe(probe, context.config.requestTimeoutMs);
          if (outcome.classification !== "answered") {
            failures.add(`soak probe ${probe.label} settled as ${outcome.classification}`);
          } else if (outcome.mismatch) {
            failures.add(outcome.mismatch);
          }
        } catch (error) {
          failures.add(
            `soak probe ${probe.label} threw: ${error instanceof Error ? error.message : error}`,
          );
        }
      }
    };

    const typer = async (): Promise<void> => {
      if (!params.typingFile) return;
      const { relativePath, typedText } = params.typingFile;
      while (Date.now() < deadline) {
        await typeFromScratch(context, relativePath, typedText, [], failures);
        if (Date.now() >= deadline) break;
        await sleep(200);
        session.changeFile(relativePath, "");
        await sleep(100);
      }
      // Leave the buffer fully typed so post-soak probes see a stable doc.
      // Skip the restore when the last typing pass already completed — a
      // didChange with identical content would be redundant edit traffic.
      if (session.textOf(relativePath) !== typedText) {
        session.changeFile(relativePath, typedText);
      }
    };

    await Promise.all([...Array.from({ length: queryWorkers }, () => queryWorker()), typer()]);

    // Final full-feature sanity pass after the soak.
    let finalSanityPass = true;
    for (const probe of params.probes) {
      const ok = await convergeProbe(context, probe, failures);
      finalSanityPass = finalSanityPass && ok;
    }

    return buildReceipt(context, startedAtMs, {
      finalSanityPass,
      failures: failures.list,
    });
  } finally {
    context.sampler?.stop();
  }
}
