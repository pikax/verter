/**
 * Per-profile participation for the VS Code host (COX0; contract
 * `docs/arch/kernel/cox0d-participation.md`).
 *
 * The host never decides participation. It sends the Rust server one
 * normalized input — the `verter.participation` preset, the optional visual
 * layers its user turned off, and the ownership cells of active competing
 * tools from the descriptor table below — and it gates its own client-local
 * producers (the CSS service, decorations) and the requests it sends on the
 * effective cells Rust returns in its receipt. Extension ids stay in this
 * module: Rust receives profile ids, operation keys, selectors and opaque
 * evidence digests only.
 *
 * vscode-free so it is unit-testable without the editor runtime.
 */

import { createHash } from "node:crypto";
import { CLIENT_FRAMEWORKS, PARTICIPATION_OPERATIONS } from "@verter/language-shared";

/** The custom request that replaces the server's participation input whole. */
export const UPDATE_PARTICIPATION_METHOD = "$/verter/updateParticipation";

/** The public preset vocabulary: exactly these four. */
export type ParticipationPreset = "auto" | "disabled" | "workspace" | "full";

export const PARTICIPATION_PRESETS: readonly ParticipationPreset[] = [
  "auto",
  "disabled",
  "workspace",
  "full",
];

/**
 * One validated competing-tool declaration: which framework profile's facets
 * an enabled tool serves itself. Installation alone proves nothing — a
 * declaration applies only while its extension is installed and enabled.
 */
export interface CompetingToolDeclaration {
  /** Stable, identity-free row key; the evidence digest is minted from it. */
  readonly key: string;
  /** The tool's editor extension id. Host-only: never sent to the server. */
  readonly extensionId: string;
  /** The manifest framework id whose capability cells overlap. */
  readonly frameworkId: string;
  /** The overlapping facets (participation operation keys). */
  readonly operations: readonly string[];
}

/** The interactive language-feature facets a framework language service serves. */
const LANGUAGE_SERVICE_OPERATIONS: readonly string[] = [
  "hover",
  "completion",
  "completion.resolve",
  "definition",
  "typeDefinition",
  "references",
  "rename.prepare",
  "rename",
  "documentHighlight",
  "signatureHelp",
  "documentSymbol",
  "foldingRange",
  "selectionRange",
  "linkedEditing",
  "documentLink",
  "callHierarchy.prepare",
  "callHierarchy.incoming",
  "callHierarchy.outgoing",
  "diagnostics.typescript",
  "codeAction.quickFix",
  "codeAction.organizeImports",
  "codeAction.refactor",
  "formatting",
  "documentColor",
  "colorPresentation",
  "semanticTokens",
  "inlayHint",
  "codeLens",
];

/**
 * The competing tools this host can observe. Each official framework language
 * extension serves its framework's interactive language features itself, so
 * under `auto` Verter stands down for exactly those cells while it stays
 * active; `full` keeps Verter serving them anyway.
 */
export const COMPETING_TOOLS: readonly CompetingToolDeclaration[] = [
  {
    key: "vue-official-language-features",
    extensionId: "Vue.volar",
    frameworkId: "vue",
    operations: LANGUAGE_SERVICE_OPERATIONS,
  },
  {
    key: "svelte-official-language-features",
    extensionId: "svelte.svelte-vscode",
    frameworkId: "svelte",
    operations: LANGUAGE_SERVICE_OPERATIONS,
  },
];

/** The participation profile id the manifest names a framework by. */
function frameworkProfileId(frameworkId: string): string | undefined {
  return CLIENT_FRAMEWORKS.find((framework) => framework.frameworkId === frameworkId)
    ?.participationProfileId;
}

function isOperation(operation: string): boolean {
  return PARTICIPATION_OPERATIONS.includes(operation);
}

/**
 * Why a declaration cannot establish ownership, or `undefined` when it is
 * valid: its framework must be one this host serves and every facet must be
 * a participation operation key.
 */
export function declarationProblem(declaration: CompetingToolDeclaration): string | undefined {
  if (frameworkProfileId(declaration.frameworkId) === undefined) {
    return `unknown framework ${declaration.frameworkId}`;
  }
  const unknown = declaration.operations.filter((operation) => !isOperation(operation));
  return unknown.length > 0 ? `unknown operations ${unknown.join(", ")}` : undefined;
}

/** The opaque, identity-free evidence digest of one declared cell. */
function evidenceDigest(key: string, operation: string): string {
  return createHash("sha256")
    .update(`verter.coexistence.declaration.v1\0${key}\0${operation}`)
    .digest("hex");
}

/** The user's optional visual layers, by setting. */
export interface VisualPreferences {
  readonly apiCallDecorations: boolean;
  readonly bindingColorDecorations: boolean;
  readonly propConstnessDecorations: boolean;
}

/** What the host knows when it builds an input. */
export interface ParticipationObservation {
  /**
   * The configured `verter.participation` value, as authored. The server
   * validates it: an invalid value is refused, never read as `auto`.
   */
  readonly preset: string;
  readonly visuals: VisualPreferences;
  /** The extension ids that are installed and enabled right now. */
  readonly enabledExtensionIds: ReadonlySet<string>;
}

/** The wire input (`verter_protocol::participation::ParticipationInput`). */
export interface ParticipationInput {
  version: 1;
  presets: Array<{
    target: { kind: "all" };
    selector: { kind: "all" };
    preset: string;
  }>;
  ownership: Array<{
    profile: { kind: "framework"; id: string };
    operation: string;
    selector: { kind: "all" };
    evidence: string;
  }>;
  disabledPresentations: Array<{ kind: "operation"; operation: string }>;
  unsupportedClientOperations: string[];
}

/**
 * Build the complete participation input. Declarations whose extension is
 * not enabled, or that fail validation, contribute nothing, so an unknown or
 * malformed declaration can never withdraw broadly. The output is ordered by
 * the declaration table, never by extension enumeration order.
 */
export function buildParticipationInput(
  observation: ParticipationObservation,
  declarations: readonly CompetingToolDeclaration[] = COMPETING_TOOLS,
): ParticipationInput {
  const ownership: ParticipationInput["ownership"] = [];
  for (const declaration of declarations) {
    if (!observation.enabledExtensionIds.has(declaration.extensionId)) continue;
    if (declarationProblem(declaration) !== undefined) continue;
    const id = frameworkProfileId(declaration.frameworkId) as string;
    for (const operation of declaration.operations) {
      ownership.push({
        profile: { kind: "framework", id },
        operation,
        selector: { kind: "all" },
        evidence: evidenceDigest(declaration.key, operation),
      });
    }
  }
  const disabledPresentations: ParticipationInput["disabledPresentations"] = [];
  const { visuals } = observation;
  if (!visuals.apiCallDecorations) {
    disabledPresentations.push({ kind: "operation", operation: "decoration.apiCalls" });
  }
  if (!visuals.bindingColorDecorations) {
    disabledPresentations.push({ kind: "operation", operation: "decoration.bindingColors" });
  }
  if (!visuals.propConstnessDecorations) {
    disabledPresentations.push({ kind: "operation", operation: "decoration.propConstness" });
  }
  return {
    version: 1,
    presets: [{ target: { kind: "all" }, selector: { kind: "all" }, preset: observation.preset }],
    ownership,
    disabledPresentations,
    unsupportedClientOperations: [],
  };
}

/** One effective cell of the server's receipt. */
export interface ReceiptCell {
  profile: { kind: string; id: string };
  operation: string;
  regionRole: string | null;
  mode: "disabled" | "workspaceOnly" | "full";
  admitted: boolean;
}

/** The server's answer to an accepted input. */
export interface ParticipationReceipt {
  mask: string;
  fence: number;
  changed: boolean;
  invalidEvidence: string[];
  cells: ReceiptCell[];
}

function cellKey(profileId: string, operation: string, regionRole: string | null): string {
  return `${profileId}\0${operation}\0${regionRole ?? ""}`;
}

/**
 * The effective cells the host gates on. Before the first receipt every cell
 * reads admitted (the `auto` default with no observed overlap); the server
 * still refuses anything it has withdrawn.
 */
export class ParticipationCells {
  private readonly admitted = new Map<string, boolean>();

  constructor(receipt?: ParticipationReceipt) {
    for (const cell of receipt?.cells ?? []) {
      this.admitted.set(cellKey(cell.profile.id, cell.operation, cell.regionRole), cell.admitted);
    }
  }

  /**
   * Whether an interactive `operation` demand on a document of `languageId`
   * (in `regionRole`, when given) is admitted. A document no framework
   * carrier owns has no framework cell here and is not gated by the host.
   */
  admits(languageId: string, operation: string, regionRole?: string): boolean {
    const framework = CLIENT_FRAMEWORKS.find((candidate) =>
      candidate.clientLanguageIds.includes(languageId),
    );
    if (!framework) return true;
    return (
      this.admitted.get(cellKey(framework.participationProfileId, operation, regionRole ?? null)) ??
      true
    );
  }

  /** Whether any served framework admits a workspace-wide `operation` demand. */
  anyAdmits(operation: string): boolean {
    return CLIENT_FRAMEWORKS.some(
      (framework) => this.admitted.get(cellKey(framework.participationProfileId, operation, null)) ?? true,
    );
  }
}

/** Whether a request error is the server's typed participation refusal. */
export function isParticipationRefusal(error: unknown): boolean {
  if (typeof error !== "object" || error === null) return false;
  const data = (error as { data?: unknown }).data;
  return typeof data === "object" && data !== null && "verterParticipation" in data;
}
