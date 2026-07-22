import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { isAbsolute, join, relative, resolve, sep } from "node:path";

import {
  EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY,
  EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY,
  EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY,
  editorTsserverAttestationFileName,
  parseEditorTsserverAttestationReceipt,
  type EditorTsserverAttestationReceipt,
} from "@verter/language-shared";

export const VERTER_TYPESCRIPT_PLUGIN_ID = "@verter/typescript-plugin";

export interface EditorTsserverBootstrapPlan {
  directory: string;
  nonce: string;
  receiptPath: string;
  pluginConfig: Record<string, unknown>;
  lspArgs: string[];
}

export interface EditorTsserverBootstrapRuntime {
  activate: () => PromiseLike<unknown> | unknown;
  configurePlugin: (
    pluginId: string,
    config: Record<string, unknown>,
  ) => PromiseLike<unknown> | unknown;
  /** Drive one workspace file through the editor TS feature so its configured project exists. */
  prepareProject?: () => PromiseLike<unknown> | unknown;
  waitForAttestation?: (
    plan: Pick<EditorTsserverBootstrapPlan, "receiptPath" | "nonce">,
  ) => PromiseLike<EditorTsserverAttestationReceipt>;
}

/**
 * Whether the configured policy selects the editor-owned tsserver tier.
 *
 * EXPLICIT selection only. On this tier the LSP owns no TypeScript engine and
 * hands carrier rename to the plugin running inside VS Code's own tsserver, so
 * the tier serves only when tsserver keeps the carrier in a configured project
 * the plugin has a live runtime for. That is a property of the user's project
 * topology, it cannot be verified before the LSP has published its carriers,
 * and a workspace where it does not hold gets no rename at all. The automatic
 * policy therefore never selects it; `tsserver` means the workspace tsserver
 * the setting advertises.
 */
export function typeProviderRoutesEditorTsserver(typeProvider: string | undefined): boolean {
  return typeProvider === "editor-tsserver";
}

/** The editor plugin owns carrier source features only after this tier attests. */
export function editorTsserverOwnsCarrierSourceFeatures(lspArgs: readonly string[]): boolean {
  return lspArgs.length > 0;
}

/** Select a carrier that can activate this plugin and belongs to the challenged workspace. */
export function selectEditorTsserverBootstrapCarrier(
  workspaceRoot: string,
  candidates: readonly string[],
): string | undefined {
  const root = resolve(workspaceRoot);
  return candidates.find((candidate) => {
    const fromRoot = relative(root, resolve(candidate));
    return (
      fromRoot.length > 0 &&
      fromRoot !== ".." &&
      !fromRoot.startsWith(`..${sep}`) &&
      !isAbsolute(fromRoot)
    );
  });
}

/** Whether a receipt names an on-disk configured project inside this workspace. */
export function receiptIncludesConfiguredProject(
  receipt: EditorTsserverAttestationReceipt,
  workspaceRoot: string,
  exists: (path: string) => boolean = existsSync,
): boolean {
  const root = resolve(workspaceRoot);
  return receipt.projects.some((project) => {
    if (/inferredProject/i.test(project) || !/\.json$/i.test(project)) return false;
    const candidate = resolve(project);
    return (
      selectEditorTsserverBootstrapCarrier(root, [candidate]) !== undefined && exists(candidate)
    );
  });
}

export function planEditorTsserverBootstrap(opts: {
  root: string;
  rng?: (size: number) => Buffer;
  mkdir?: (path: string) => void;
}): EditorTsserverBootstrapPlan {
  const nonce = (opts.rng ?? randomBytes)(16).toString("hex");
  const directory = join(opts.root, `verter-editor-tsserver-${nonce}`);
  (opts.mkdir ?? ((path) => void mkdirSync(path, { recursive: true })))(directory);
  const receiptPath = join(directory, editorTsserverAttestationFileName(nonce));
  return {
    directory,
    nonce,
    receiptPath,
    pluginConfig: {
      enable: true,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
      [EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY]: { directory, nonce },
    },
    lspArgs: [`--editor-tsserver-receipt=${receiptPath}`, `--editor-tsserver-nonce=${nonce}`],
  };
}

export async function waitForEditorTsserverAttestation(
  plan: Pick<EditorTsserverBootstrapPlan, "receiptPath" | "nonce">,
  opts: {
    timeoutMs?: number;
    pollMs?: number;
    exists?: (path: string) => boolean;
    read?: (path: string) => string;
    accept?: (receipt: EditorTsserverAttestationReceipt) => boolean;
  } = {},
): Promise<EditorTsserverAttestationReceipt> {
  const timeoutMs = opts.timeoutMs ?? 5_000;
  const pollMs = opts.pollMs ?? 25;
  const exists = opts.exists ?? existsSync;
  const read = opts.read ?? ((path) => readFileSync(path, "utf8"));
  const started = Date.now();
  let lastFailure = "receipt not written";
  while (Date.now() - started < timeoutMs) {
    if (exists(plan.receiptPath)) {
      try {
        const receipt = parseEditorTsserverAttestationReceipt(
          JSON.parse(read(plan.receiptPath)),
          plan.nonce,
        );
        if (receipt && (!opts.accept || opts.accept(receipt))) return receipt;
        lastFailure = receipt
          ? "receipt did not identify the required configured project"
          : "receipt was malformed, unbound, or from another session";
      } catch (error) {
        lastFailure = error instanceof Error ? error.message : String(error);
      }
    }
    await new Promise<void>((resolve) => setTimeout(resolve, pollMs));
  }
  throw new Error(`editor tsserver plugin attestation timed out: ${lastFailure}`);
}

/**
 * Activate VS Code's TypeScript service, configure this contributed plugin, and
 * require a project-bound receipt written from inside that exact editor process.
 * Every externally controlled operation is bounded independently so activation
 * cannot hold Verter startup indefinitely.
 */
export async function attestEditorTsserverBootstrap(
  plan: EditorTsserverBootstrapPlan,
  runtime: EditorTsserverBootstrapRuntime,
  opts: {
    operationTimeoutMs?: number;
    attestationTimeoutMs?: number;
    acceptAttestation?: (receipt: EditorTsserverAttestationReceipt) => boolean;
  } = {},
): Promise<EditorTsserverAttestationReceipt> {
  const operationTimeoutMs = opts.operationTimeoutMs ?? 5_000;
  await runBoundedEditorOperation(
    "editor TypeScript activation",
    runtime.activate,
    operationTimeoutMs,
  );
  if (runtime.prepareProject) {
    await runBoundedEditorOperation(
      "editor TypeScript configured-project preparation",
      runtime.prepareProject,
      operationTimeoutMs,
    );
  }
  await runBoundedEditorOperation(
    "editor tsserver plugin configuration",
    () => runtime.configurePlugin(VERTER_TYPESCRIPT_PLUGIN_ID, plan.pluginConfig),
    operationTimeoutMs,
  );
  const receipt = await runBoundedEditorOperation(
    "editor tsserver plugin attestation",
    () =>
      runtime.waitForAttestation
        ? runtime.waitForAttestation({
            receiptPath: plan.receiptPath,
            nonce: plan.nonce,
          })
        : waitForEditorTsserverAttestation(
            { receiptPath: plan.receiptPath, nonce: plan.nonce },
            { accept: opts.acceptAttestation },
          ),
    opts.attestationTimeoutMs ?? 5_500,
  );
  if (opts.acceptAttestation && !opts.acceptAttestation(receipt)) {
    throw new Error("editor tsserver attestation did not identify the required project");
  }
  return receipt;
}

function runBoundedEditorOperation<T>(
  label: string,
  operation: () => PromiseLike<T> | T,
  timeoutMs: number,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${timeoutMs}ms`)),
      timeoutMs,
    );
    Promise.resolve()
      .then(operation)
      .then(
        (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        (error) => {
          clearTimeout(timer);
          reject(error);
        },
      );
  });
}
