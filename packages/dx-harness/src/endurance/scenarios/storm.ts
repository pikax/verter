/**
 * Scenario 3 — hover/definition storms (the D2 reproduction) + mixed storms
 * during typing.
 *
 * Several workers keep bounded in-flight hover/definition traffic against
 * template prop/event/local tokens across MULTIPLE carrier files while a
 * typer concurrently churns the import-chain root file (forcing downstream
 * invalidation under load). Assertions are non-negotiable: EVERY request
 * settles (answered or properly cancelled — a timeout rejection is a silent
 * drop and fails), the provider is alive after, p95 latency is bounded, and
 * hover/definition STILL answer with correct typed content after the storm.
 */
import type { EnduranceReceipt } from "../types.js";
import type { EnduranceProbe } from "../session.js";
import { camelToKebab } from "../session.js";
import { sleep } from "../metrics.js";
import type { WorkspaceFiles } from "../workspace.js";
import { buildCarrierSet } from "../workspace.js";
import {
  buildReceipt,
  convergeProbe,
  FailureBag,
  replaceOnce,
  type ScenarioContext,
} from "./common.js";

export const STORM_CARRIER_COUNT = 6;

export interface StormParams {
  /** Content-checked probes; MUST NOT target the churn file. */
  readonly probes: readonly EnduranceProbe[];
  /** File the typer churns during the storm (downstream-invalidation source). */
  readonly churn?: { relativePath: string; baseText: string };
  readonly durationMs?: number;
  readonly workers?: number;
  /** Override churn pacing (default: 1000/typingCps — human cadence). */
  readonly churnIntervalMs?: number;
}

/** Build the carrier storm probes (carriers 1..N — carrier 0 is the churn target). */
export function carrierStormProbes(carriers: readonly string[]): EnduranceProbe[] {
  const probes: EnduranceProbe[] = [];
  for (let index = 1; index < carriers.length; index += 1) {
    const relativePath = carriers[index];
    const prop = `carrierProp${index}`;
    const local = `carrierLocal${index}`;
    const handler = `onCarrier${index}`;
    const childTag = `Carrier${index - 1}`;
    const childProp = `carrierProp${index - 1}`;
    probes.push(
      {
        // Provider member hover: documented type-quality gap — informational.
        kind: "hover",
        relativePath,
        needle: `:title="props.${prop}"`,
        cursorOffset: `:title="props.`.length,
        expectIncludes: [],
        informational: true,
        label: `${relativePath} prop hover`,
      },
      {
        // Vue binding hover on a local: Verter owns the answer — strong.
        kind: "hover",
        relativePath,
        needle: `{{ ${local} }}`,
        cursorOffset: 3,
        expectIncludes: [local],
        requireNonEmpty: true,
        label: `${relativePath} local hover`,
      },
      {
        // Template event → script handler navigation: Verter-owned mapping.
        kind: "definition",
        relativePath,
        needle: `@click="${handler}"`,
        cursorOffset: 8,
        expectLineNeedle: `function ${handler}`,
        label: `${relativePath} handler definition`,
      },
      {
        // D1 STABILITY: component attr-name completion on the bare child tag
        // must offer the child's typed prop names (in template-idiomatic
        // kebab-case form) — under full storm load.
        kind: "completion",
        relativePath,
        needle: `<${childTag} />`,
        cursorOffset: `<${childTag} `.length,
        expectLabels: [camelToKebab(childProp)],
        label: `${relativePath} child attr completion (D1)`,
      },
    );
  }
  return probes;
}

export async function runStormScenario(
  context: ScenarioContext,
  params: StormParams,
): Promise<EnduranceReceipt> {
  const failures = new FailureBag();
  const startedAtMs = Date.now();
  const { session } = context;
  const durationMs = params.durationMs ?? context.config.stormDurationMs;
  const workers = params.workers ?? context.config.stormWorkers;
  if (params.probes.length === 0) throw new Error("storm requires at least one probe");
  context.sampler?.start();
  try {
    // Calibration: every probe must answer correctly BEFORE the storm, so a
    // post-storm mismatch is attributable to the storm, not the fixture.
    for (const probe of params.probes) {
      await convergeProbe(context, probe, failures);
    }

    const deadline = Date.now() + durationMs;
    let cursor = 0;
    const worker = async (): Promise<void> => {
      while (Date.now() < deadline) {
        const probe = params.probes[cursor % params.probes.length];
        cursor += 1;
        try {
          const outcome = await session.runProbe(probe, context.config.requestTimeoutMs);
          if (outcome.classification !== "answered") {
            failures.add(`storm probe ${probe.label} settled as ${outcome.classification}`);
          } else if (outcome.mismatch) {
            failures.add(outcome.mismatch);
          }
        } catch (error) {
          failures.add(
            `storm probe ${probe.label} threw: ${error instanceof Error ? error.message : error}`,
          );
        }
      }
    };

    const typer = async (): Promise<void> => {
      if (!params.churn) return;
      const { relativePath, baseText } = params.churn;
      // Churn edits pace at the human typing cadence (the hover/definition
      // workers above are the aggressive D2 storm; the typing is not).
      const intervalMs = params.churnIntervalMs ?? 1000 / context.config.typingCps;
      let tick = 0;
      while (Date.now() < deadline) {
        tick += 1;
        session.changeFile(
          relativePath,
          replaceOnce(baseText, "</script>", `// endurance-churn-${tick}\n</script>`),
        );
        await sleep(intervalMs);
        if (Date.now() >= deadline) break;
        session.changeFile(relativePath, baseText);
        await sleep(intervalMs);
      }
      // Always leave the churned file back at its base content.
      session.changeFile(relativePath, baseText);
    };

    await Promise.all([...Array.from({ length: workers }, () => worker()), typer()]);

    // Post-storm: every probe must STILL answer with correct typed content.
    let postStormOk = true;
    for (const probe of params.probes) {
      const ok = await convergeProbe(context, probe, failures);
      postStormOk = postStormOk && ok;
    }

    return buildReceipt(context, startedAtMs, {
      finalSanityPass: postStormOk,
      failures: failures.list,
    });
  } finally {
    context.sampler?.stop();
  }
}

/** Synthetic carrier workspace for the storm spec (chain: 1→0, 2→1, …). */
export function stormWorkspace(carrierCount = STORM_CARRIER_COUNT): {
  files: WorkspaceFiles;
  carriers: readonly string[];
} {
  return buildCarrierSet(carrierCount);
}
