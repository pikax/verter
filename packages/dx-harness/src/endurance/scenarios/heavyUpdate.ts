/** Heavy edit → query loops across Vue/Svelte and TypeScript/JavaScript. */
import { DEFAULT_ENDURANCE_LANE, type EnduranceLane, type EnduranceReceipt } from "../types.js";
import {
  carrierPath,
  childConsumerContent,
  ENDURANCE_TSCONFIG,
  heavyUpdateChildContent,
  type WorkspaceFiles,
} from "../workspace.js";
import {
  buildReceipt,
  convergeProbe,
  FailureBag,
  replaceOnce,
  type ScenarioContext,
} from "./common.js";

export interface HeavyUpdateFixture {
  readonly lane: EnduranceLane;
  readonly childPath: string;
  readonly parentPath: string;
  readonly childContent: string;
  readonly files: WorkspaceFiles;
  readonly member: string;
  readonly memberWithBadge: string;
  readonly destructure: string | null;
  readonly destructureWithBadge: string | null;
  readonly attrSite: string;
  readonly attrSiteWithBadge: string;
  readonly pickIntact: string;
  readonly pickBroken: string;
}

export function heavyUpdateFixture(
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): HeavyUpdateFixture {
  const childPath = carrierPath(lane, "Child");
  const parentPath = carrierPath(lane, "App");
  const childContent = heavyUpdateChildContent(lane);
  let member: string;
  let memberWithBadge: string;
  if (lane.framework === "vue" && lane.mode === "js") {
    member = "  count: Number,";
    memberWithBadge = "  count: Number,\n  badge: String,";
  } else if (lane.framework === "svelte" && lane.mode === "js") {
    member = "label: string, count?: number,";
    memberWithBadge = "label: string, count?: number, badge?: string,";
  } else {
    member = lane.framework === "vue" ? "  count?: number;" : "    count?: number;";
    memberWithBadge = `${member}\n${lane.framework === "vue" ? "  badge?: string;" : "    badge?: string;"}`;
  }
  const attrSite = lane.framework === "vue" ? ':title="props.label"' : "title={label}";
  const attrSiteWithBadge =
    lane.framework === "vue"
      ? ':title="props.label" :data-badge="props.badge"'
      : "title={label} data-badge={badge}";
  const pickIntact =
    lane.framework === "vue" ? '  emit("select", props.label);\n}' : "    onselect?.(label);\n  }";
  const pickBroken =
    lane.framework === "vue" ? '  emit("select", props.label);' : "    onselect?.(label);";
  const destructure =
    lane.framework === "svelte"
      ? lane.mode === "ts"
        ? "  let { label, count, onselect }: ChildProps = $props();"
        : "  let { label, count, onselect } = $props();"
      : null;
  const destructureWithBadge =
    lane.framework === "svelte"
      ? lane.mode === "ts"
        ? "  let { label, count, badge, onselect }: ChildProps = $props();"
        : "  let { label, count, badge, onselect } = $props();"
      : null;
  return {
    lane,
    childPath,
    parentPath,
    childContent,
    files: {
      "tsconfig.json": ENDURANCE_TSCONFIG,
      [childPath]: childContent,
      [parentPath]: childConsumerContent(lane),
    },
    member,
    memberWithBadge,
    destructure,
    destructureWithBadge,
    attrSite,
    attrSiteWithBadge,
    pickIntact,
    pickBroken,
  };
}

export const HEAVY_UPDATE_FILES: WorkspaceFiles = heavyUpdateFixture().files;

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function jsIdentifierPattern(ident: string): string {
  return `(?<![A-Za-z0-9_$])${escapeRegExp(ident)}(?![A-Za-z0-9_$])`;
}

function identifierUsageOccurrence(text: string, ident: string): number {
  const escaped = escapeRegExp(ident);
  const declaration = new RegExp(`\\b(?:const|let)\\s+${escaped}\\s*=`).exec(text);
  if (!declaration) throw new Error(`rename target ${ident} has no const/let declaration`);
  const declarationIdentOffset = declaration.index + declaration[0].indexOf(ident);
  let from = 0;
  let occurrence = 0;
  for (;;) {
    const offset = text.indexOf(ident, from);
    if (offset === -1) break;
    const before = text[offset - 1];
    const after = text[offset + ident.length];
    const isWholeIdentifier =
      (before === undefined || !/[A-Za-z0-9_$]/.test(before)) &&
      (after === undefined || !/[A-Za-z0-9_$]/.test(after));
    if (offset > declarationIdentOffset && isWholeIdentifier) return occurrence;
    occurrence += 1;
    from = offset + 1;
  }
  throw new Error(`rename target ${ident} has no whole-identifier usage after its declaration`);
}

export async function runRenameCycles(
  context: ScenarioContext,
  relativePath: string,
  ident: string,
  cycles: number,
  failures: FailureBag,
): Promise<boolean> {
  const boundary = new RegExp(jsIdentifierPattern(ident), "g");
  const original = context.session.textOf(relativePath);
  const escapedIdent = escapeRegExp(ident);
  const declaration = new RegExp(`\\b(const|let)\\s+${escapedIdent}\\s*=`).exec(original);
  if (!declaration) {
    throw new Error(`rename target ${ident} has no const/let declaration in ${relativePath}`);
  }
  const declarationKind = declaration[1];
  let finalSanityPass = false;
  for (let cycle = 0; cycle < cycles; cycle += 1) {
    const renamed = `${ident}__renamed${cycle}`;
    const current = context.session.textOf(relativePath);
    const renamedText = current.replace(boundary, renamed);
    context.session.changeFile(relativePath, renamedText);
    const renamedUsageOccurrence = identifierUsageOccurrence(renamedText, renamed);
    await convergeProbe(
      context,
      {
        kind: "hover",
        relativePath,
        needle: renamed,
        occurrence: renamedUsageOccurrence,
        cursorOffset: 2,
        expectIncludes: [renamed],
        forbidIncludes: ["any"],
        label: `${context.lane.id} rename-cycle ${cycle} usage hover`,
      },
      failures,
    );
    await convergeProbe(
      context,
      {
        kind: "definition",
        relativePath,
        needle: renamed,
        occurrence: renamedUsageOccurrence,
        cursorOffset: 2,
        expectUriSuffix: `/${relativePath}`,
        expectLineNeedle: `${declarationKind} ${renamed}`,
        label: `${context.lane.id} rename-cycle ${cycle} usage definition`,
      },
      failures,
    );
    const restored = context.session.textOf(relativePath);
    const restoredText = restored.replace(new RegExp(jsIdentifierPattern(renamed), "g"), ident);
    context.session.changeFile(relativePath, restoredText);
    const restoredUsageOccurrence = identifierUsageOccurrence(restoredText, ident);
    const restoredHoverOk = await convergeProbe(
      context,
      {
        kind: "hover",
        relativePath,
        needle: ident,
        occurrence: restoredUsageOccurrence,
        cursorOffset: 2,
        expectIncludes: [ident],
        forbidIncludes: [renamed, "any"],
        label: `${context.lane.id} rename-cycle ${cycle} restored usage hover`,
      },
      failures,
    );
    const restoredDefinitionOk = await convergeProbe(
      context,
      {
        kind: "definition",
        relativePath,
        needle: ident,
        occurrence: restoredUsageOccurrence,
        cursorOffset: 2,
        expectUriSuffix: `/${relativePath}`,
        expectLineNeedle: `${declarationKind} ${ident}`,
        label: `${context.lane.id} rename-cycle ${cycle} restored usage definition`,
      },
      failures,
    );
    finalSanityPass = restoredHoverOk && restoredDefinitionOk;
  }
  return finalSanityPass;
}

export async function runHeavyUpdateScenario(
  context: ScenarioContext,
  options: { cycles?: number; fixture?: HeavyUpdateFixture } = {},
): Promise<EnduranceReceipt> {
  const fixture = options.fixture ?? heavyUpdateFixture(context.lane);
  const failures = new FailureBag();
  const startedAtMs = Date.now();
  const { session } = context;
  const cycles = options.cycles ?? context.config.heavyUpdateCycles;
  context.sampler?.start();
  try {
    session.openFile(fixture.childPath);
    session.openFile(fixture.parentPath);
    let buffer = fixture.childContent;
    const apply = (next: string): void => {
      session.changeFile(fixture.childPath, next);
      buffer = next;
    };

    for (let cycle = 0; cycle < cycles; cycle += 1) {
      const from = cycle % 2 === 0 ? "greeting" : "salutation";
      const to = cycle % 2 === 0 ? "salutation" : "greeting";
      const usageNeedle = fixture.lane.framework === "vue" ? `{{ ${to} }}` : `${to}.length`;
      const cursorOffset = fixture.lane.framework === "vue" ? 3 : 2;

      apply(buffer.replaceAll(from, to));
      // The renamed NAME must surface and the stale name must be gone in every
      // lane (hard). The TYPE TEXT at a template-mapped position is
      // provider-owned and truthfully surfaces `any` on the tsserver route
      // today (documented provider type-quality gap); the Svelte script
      // position answers typed and additionally forbids `any`.
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: fixture.childPath,
          needle: usageNeedle,
          cursorOffset,
          expectIncludes: [to],
          forbidIncludes: fixture.lane.framework === "vue" ? [from] : [from, "any"],
          label: `${fixture.lane.id} cycle ${cycle}: renamed member hover`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: fixture.childPath,
          needle: usageNeedle,
          cursorOffset,
          expectLineNeedle: fixture.lane.framework === "vue" ? `const ${to}` : `let ${to}`,
          label: `${fixture.lane.id} cycle ${cycle}: renamed member definition`,
        },
        failures,
      );

      let added = replaceOnce(buffer, fixture.member, fixture.memberWithBadge);
      if (fixture.destructure && fixture.destructureWithBadge) {
        added = replaceOnce(added, fixture.destructure, fixture.destructureWithBadge);
      }
      apply(replaceOnce(added, fixture.attrSite, fixture.attrSiteWithBadge));
      await convergeProbe(
        context,
        {
          kind: "completion",
          relativePath: fixture.parentPath,
          needle: "<Child ",
          cursorOffset: "<Child ".length,
          expectLabels: ["label", "count", "badge"],
          label: `${fixture.lane.id} cycle ${cycle}: added prop completion`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: fixture.childPath,
          needle: fixture.lane.framework === "vue" ? "props.badge" : "badge}",
          cursorOffset: fixture.lane.framework === "vue" ? 6 : 2,
          expectIncludes: [],
          informational: true,
          label: `${fixture.lane.id} cycle ${cycle}: added prop hover`,
        },
        failures,
      );

      let removed = replaceOnce(buffer, fixture.memberWithBadge, fixture.member);
      if (fixture.destructure && fixture.destructureWithBadge) {
        removed = replaceOnce(removed, fixture.destructureWithBadge, fixture.destructure);
      }
      apply(replaceOnce(removed, fixture.attrSiteWithBadge, fixture.attrSite));
      await convergeProbe(
        context,
        {
          kind: "completion",
          relativePath: fixture.parentPath,
          needle: "<Child ",
          cursorOffset: "<Child ".length,
          expectLabels: ["label", "count"],
          forbidLabels: ["badge"],
          label: `${fixture.lane.id} cycle ${cycle}: removed prop completion`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "completion",
          relativePath: fixture.childPath,
          needle: fixture.lane.framework === "vue" ? ':title="props.' : "title={label",
          cursorOffset:
            fixture.lane.framework === "vue" ? ':title="props.'.length : "title={label".length,
          expectLabels: [],
          forbidLabels: ["badge"],
          informational: true,
          label: `${fixture.lane.id} cycle ${cycle}: removed member stays absent`,
        },
        failures,
      );

      apply(replaceOnce(buffer, fixture.pickIntact, fixture.pickBroken));
      const broken = await session.runProbe({
        kind: "hover",
        relativePath: fixture.childPath,
        needle: usageNeedle,
        cursorOffset,
        expectIncludes: [],
        informational: true,
        label: `${fixture.lane.id} cycle ${cycle}: broken-syntax hover settles`,
      });
      if (broken.classification !== "answered") {
        failures.add(`cycle ${cycle}: broken-syntax hover settled as ${broken.classification}`);
      }

      apply(replaceOnce(buffer, fixture.pickBroken, fixture.pickIntact));
      await convergeProbe(
        context,
        {
          kind: "hover",
          relativePath: fixture.childPath,
          needle: usageNeedle,
          cursorOffset,
          expectIncludes: [to],
          forbidIncludes: fixture.lane.framework === "vue" ? [from] : [from, "any"],
          label: `${fixture.lane.id} cycle ${cycle}: recovered hover`,
        },
        failures,
      );
      await convergeProbe(
        context,
        {
          kind: "definition",
          relativePath: fixture.childPath,
          needle: usageNeedle,
          cursorOffset,
          expectLineNeedle: fixture.lane.framework === "vue" ? `const ${to}` : `let ${to}`,
          label: `${fixture.lane.id} cycle ${cycle}: recovered definition`,
        },
        failures,
      );
    }

    return buildReceipt(context, startedAtMs, { finalSanityPass: null, failures: failures.list });
  } finally {
    context.sampler?.stop();
  }
}
