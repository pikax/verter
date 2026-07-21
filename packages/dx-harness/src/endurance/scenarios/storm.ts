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
import { DEFAULT_ENDURANCE_LANE, type EnduranceLane, type EnduranceReceipt } from "../types.js";
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
export function carrierStormProbes(
  carriers: readonly string[],
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): EnduranceProbe[] {
  const probes: EnduranceProbe[] = [];
  for (let index = 1; index < carriers.length; index += 1) {
    const relativePath = carriers[index];
    const prop = `carrierProp${index}`;
    const local = `carrierLocal${index}`;
    const handler = `onCarrier${index}`;
    const childTag = `Carrier${index - 1}`;
    const childUnusedProp = `carrierUnusedOnly${String(index - 1).padStart(6, "0")}`;
    probes.push(
      {
        // Provider member hover: documented type-quality gap — informational.
        kind: "hover",
        relativePath,
        needle: lane.framework === "vue" ? `:title="props.${prop}"` : `${prop}.length`,
        cursorOffset: lane.framework === "vue" ? `:title="props.`.length : 2,
        expectIncludes: lane.framework === "vue" ? [] : [prop],
        ...(lane.framework === "vue"
          ? { informational: true as const }
          : { forbidIncludes: ["any"], requireNonEmpty: true }),
        label: `${relativePath} prop hover`,
      },
      {
        // Strong framework-local hover: the binding NAME is Verter-owned in
        // every lane (hard + non-empty + name fragment). The TYPE TEXT at a
        // template-mapped position is provider-owned and truthfully surfaces
        // `any` on the tsserver route today (documented provider type-quality
        // gap), so no type fragment is forbidden there; the Svelte probe uses
        // a script position whose typed answer DOES forbid `any`.
        kind: "hover",
        relativePath,
        needle: lane.framework === "vue" ? `{{ ${local} }}` : `${local}.length`,
        cursorOffset: lane.framework === "vue" ? 3 : 2,
        expectIncludes: [local],
        forbidIncludes: lane.framework === "vue" ? [] : ["any"],
        requireNonEmpty: true,
        label: `${relativePath} local hover`,
      },
      {
        // Strong definition navigation: Vue event handler, Svelte imported component.
        kind: "definition",
        relativePath,
        needle: lane.framework === "vue" ? `@click="${handler}"` : `<${childTag}`,
        cursorOffset: lane.framework === "vue" ? 8 : 2,
        ...(lane.framework === "vue"
          ? { expectLineNeedle: `function ${handler}` }
          : { expectUriSuffix: `/${carriers[index - 1]}` }),
        label: `${relativePath} ${lane.framework === "vue" ? "handler" : "component"} definition`,
      },
      {
        // D1 STABILITY: component attr-name completion on the bare child tag
        // must offer the child's typed prop names (in template-idiomatic
        // kebab-case form) — under full storm load.
        kind: "completion",
        relativePath,
        needle: `<${childTag} />`,
        cursorOffset: `<${childTag} `.length,
        expectLabels: [lane.framework === "vue" ? camelToKebab(childUnusedProp) : childUnusedProp],
        label: `${relativePath} child attr completion (D1)`,
      },
    );
    if (lane.framework === "svelte") {
      probes.push(
        {
          kind: "definition",
          relativePath,
          needle: `onclick={${handler}}`,
          cursorOffset: "onclick={".length + 1,
          expectLineNeedle: `function ${handler}`,
          label: `${relativePath} markup event-site definition`,
        },
        {
          kind: "definition",
          relativePath,
          needle: "onfire?.(",
          cursorOffset: 2,
          expectLineNeedle: `let { ${prop}, onfire, content }`,
          label: `${relativePath} callback-event definition`,
        },
        {
          kind: "hover",
          relativePath,
          needle: "Snippet<[string]>",
          cursorOffset: 2,
          expectIncludes: ["Snippet"],
          ...(lane.mode === "js"
            ? { informational: true as const }
            : { forbidIncludes: ["any"], requireNonEmpty: true }),
          label: `${relativePath} snippet hover`,
        },
        {
          kind: "definition",
          relativePath,
          needle: "@render carrierSnippet",
          cursorOffset: "@render ".length + 1,
          expectLineNeedle: "{#snippet carrierSnippet",
          label: `${relativePath} snippet definition`,
        },
      );
    } else {
      probes.push({
        kind: "definition",
        relativePath,
        needle: `{{ ${local} }}`,
        cursorOffset: 3,
        expectLineNeedle: `const ${local}`,
        label: `${relativePath} local definition`,
      });
    }
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
export function stormWorkspace(
  carrierCount?: number,
  lane?: EnduranceLane,
): { files: WorkspaceFiles; carriers: readonly string[]; lane: EnduranceLane };
export function stormWorkspace(
  carrierCount = STORM_CARRIER_COUNT,
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): { files: WorkspaceFiles; carriers: readonly string[]; lane: EnduranceLane } {
  return buildCarrierSet(carrierCount, lane);
}
