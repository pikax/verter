/**
 * Scenario 4 — soak.
 *
 * Sustained mixed workload for a configurable duration: a typer repeatedly
 * builds a small SFC from scratch in a scratch buffer while hover /
 * completion / definition workers cycle across stable carrier files. Asserts
 * no p95 degradation trend (late window <= early window * factor AND <= an
 * absolute bound), the provider stays alive, ZERO unanswered requests, RSS
 * under the configured ceiling, and a final full-feature sanity pass (hover +
 * completion + definition correct on known positions) after the soak.
 */
import type { EnduranceReceipt } from "../types.js";
import type { EnduranceProbe } from "../session.js";
import { sleep } from "../metrics.js";
import type { WorkspaceFiles } from "../workspace.js";
import {
  buildCarrierSet,
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

const CHILD = "src/Child.vue";
const APP = "src/App.vue";
const SCRATCH = "src/Scratch.vue";

/** The scratch buffer + document the soak typer rebuilds on a loop. */
export const SOAK_SCRATCH_PATH = SCRATCH;

const TINY_DOC = [
  '<script setup lang="ts">',
  "const props = defineProps<{ note: string }>();",
  "const scratchLocal = `s:${props.note}`;",
  "</script>",
  "",
  "<template>",
  "  <div>{{ scratchLocal }}</div>",
  "</template>",
  "",
].join("\n");

/** The small SFC the soak typer types from scratch, repeatedly. */
export const SOAK_TYPED_DOC = TINY_DOC;

export interface SoakWorkspace {
  readonly files: WorkspaceFiles;
  readonly carriers: readonly string[];
}

export function soakWorkspace(carrierCount = 4): SoakWorkspace {
  const carrierSet = buildCarrierSet(carrierCount);
  return {
    files: {
      "tsconfig.json": ENDURANCE_TSCONFIG,
      [CHILD]: heavyUpdateChildContent(),
      [APP]: childConsumerContent(),
      [SCRATCH]: TINY_DOC,
      ...Object.fromEntries(
        Object.entries(carrierSet.files).filter(([key]) => key !== "tsconfig.json"),
      ),
    },
    carriers: carrierSet.carriers,
  };
}

/** Stable, content-checked soak probes across Child/App/carriers. */
export function soakProbes(carriers: readonly string[]): EnduranceProbe[] {
  const probes: EnduranceProbe[] = [
    {
      // Vue binding hover on a local: Verter owns the answer — strong.
      kind: "hover",
      relativePath: CHILD,
      needle: "{{ greeting }}",
      cursorOffset: 3,
      expectIncludes: ["greeting"],
      requireNonEmpty: true,
      label: "child local hover",
    },
    {
      // Script member completion: documented provider type-quality gap —
      // informational (settling asserted, emptiness recorded in typeQuality).
      kind: "completion",
      relativePath: CHILD,
      needle: ':title="props.',
      cursorOffset: ':title="props.'.length,
      expectLabels: ["label", "count"],
      informational: true,
      label: "child props member completion",
    },
    {
      kind: "definition",
      relativePath: CHILD,
      needle: "{{ greeting }}",
      cursorOffset: 3,
      expectLineNeedle: "const greeting",
      label: "child local definition",
    },
    {
      kind: "hover",
      relativePath: APP,
      needle: "{{ heading }}",
      cursorOffset: 3,
      expectIncludes: ["heading"],
      requireNonEmpty: true,
      label: "app local hover",
    },
    {
      kind: "definition",
      relativePath: APP,
      needle: "{{ heading }}",
      cursorOffset: 3,
      expectLineNeedle: "const heading",
      label: "app local definition",
    },
    // Carriers 1..N-1 (soak's typer targets only Scratch.vue, so every carrier
    // is a stable probe target; carrierStormProbes maps index→name correctly
    // only on the FULL carriers array).
    ...carrierStormProbes(carriers),
  ];
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
      session.changeFile(relativePath, typedText);
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
