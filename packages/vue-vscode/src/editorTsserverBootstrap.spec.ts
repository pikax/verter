/**
 * @ai-generated - Exercises the editor-owned tsserver challenge, bounded activation,
 * configuration, and project-bound attestation lifecycle.
 */
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import {
  attestEditorTsserverBootstrap,
  editorTsserverOwnsCarrierSourceFeatures,
  VERTER_TYPESCRIPT_PLUGIN_ID,
  planEditorTsserverBootstrap,
  receiptIncludesConfiguredProject,
  selectEditorTsserverBootstrapCarrier,
  typeProviderRoutesEditorTsserver,
  waitForEditorTsserverAttestation,
} from "./editorTsserverBootstrap";

const NONCE = "0123456789abcdef0123456789abcdef";

describe("editor tsserver bootstrap", () => {
  it("selects a framework carrier inside the challenged workspace", () => {
    const workspaceRoot = join("D:", "ws");
    const outside = join("D:", "other", "Outside.vue");
    const inside = join(workspaceRoot, "src", "App.vue");

    expect(selectEditorTsserverBootstrapCarrier(workspaceRoot, [outside, inside])).toBe(inside);
    expect(selectEditorTsserverBootstrapCarrier(workspaceRoot, [outside])).toBeUndefined();
  });

  it("builds a paired plugin challenge and neutral LSP receipt arguments", () => {
    const made: string[] = [];
    const plan = planEditorTsserverBootstrap({
      root: "/tmp",
      rng: () => Buffer.from(NONCE, "hex"),
      mkdir: (path) => made.push(path),
    });
    expect(VERTER_TYPESCRIPT_PLUGIN_ID).toBe("@verter/typescript-plugin");
    expect(plan.directory).toBe(join("/tmp", `verter-editor-tsserver-${NONCE}`));
    expect(made).toEqual([plan.directory]);
    expect(plan.pluginConfig).toEqual({
      enable: true,
      editorOwnsCarrierMembership: true,
      editorOwnsCarrierSourceFeatures: true,
      editorTsserverAttestation: { directory: plan.directory, nonce: NONCE },
    });
    expect(plan.lspArgs).toEqual([
      `--editor-tsserver-receipt=${plan.receiptPath}`,
      `--editor-tsserver-nonce=${NONCE}`,
    ]);
  });

  it("accepts only a project-bound receipt from the challenged editor process", async () => {
    const plan = {
      receiptPath: "/tmp/receipt.json",
      nonce: NONCE,
    };
    await expect(
      waitForEditorTsserverAttestation(plan, {
        timeoutMs: 20,
        pollMs: 1,
        exists: () => true,
        read: () =>
          JSON.stringify({
            version: 1,
            nonce: NONCE,
            pid: 4242,
            projects: ["/ws/tsconfig.json"],
          }),
      }),
    ).resolves.toEqual({
      version: 1,
      nonce: NONCE,
      pid: 4242,
      projects: ["/ws/tsconfig.json"],
    });
  });

  it("activates and prepares a configured project before issuing the attestation challenge", async () => {
    const plan = planEditorTsserverBootstrap({
      root: "/tmp",
      rng: () => Buffer.from(NONCE, "hex"),
      mkdir: () => {},
    });
    const operations: string[] = [];
    const receipt = {
      version: 1 as const,
      nonce: NONCE,
      pid: 4242,
      projects: ["/ws/tsconfig.json"],
    };

    await expect(
      attestEditorTsserverBootstrap(plan, {
        activate: async () => {
          operations.push("activate");
        },
        configurePlugin: async (pluginId, config) => {
          operations.push("configure");
          expect(pluginId).toBe(VERTER_TYPESCRIPT_PLUGIN_ID);
          expect(config).toEqual(plan.pluginConfig);
        },
        prepareProject: async () => {
          operations.push("prepare-project");
        },
        waitForAttestation: async () => {
          operations.push("attest");
          return receipt;
        },
      }),
    ).resolves.toEqual(receipt);
    expect(operations).toEqual(["activate", "prepare-project", "configure", "attest"]);
  });

  it("ignores an inferred-project receipt until the required configured project appears", async () => {
    let reads = 0;
    await expect(
      waitForEditorTsserverAttestation(
        { receiptPath: "/tmp/receipt.json", nonce: NONCE },
        {
          timeoutMs: 50,
          pollMs: 1,
          exists: () => true,
          read: () => {
            reads++;
            return JSON.stringify({
              version: 1,
              nonce: NONCE,
              pid: 4242,
              projects:
                reads === 1
                  ? ["/dev/null/inferredProject1*"]
                  : ["/dev/null/inferredProject1*", "/ws/tsconfig.json"],
            });
          },
          accept: (receipt) => receipt.projects.includes("/ws/tsconfig.json"),
        },
      ),
    ).resolves.toMatchObject({ projects: ["/dev/null/inferredProject1*", "/ws/tsconfig.json"] });
    expect(reads).toBeGreaterThan(1);
  });

  it("distinguishes an on-disk configured project from inferred or out-of-workspace projects", () => {
    const base = { version: 1 as const, nonce: NONCE, pid: 4242 };
    const exists = (file: string) => file.replace(/\\/g, "/").endsWith("/ws/tsconfig.json");

    expect(
      receiptIncludesConfiguredProject(
        { ...base, projects: ["/dev/null/inferredProject1*"] },
        "/ws",
        exists,
      ),
    ).toBe(false);
    expect(
      receiptIncludesConfiguredProject(
        { ...base, projects: ["/other/tsconfig.json"] },
        "/ws",
        exists,
      ),
    ).toBe(false);
    expect(
      receiptIncludesConfiguredProject({ ...base, projects: ["/ws/tsconfig.json"] }, "/ws", exists),
    ).toBe(true);
  });

  it.each([
    ["activation", "activate"],
    ["plugin configuration", "configurePlugin"],
  ] as const)("bounds a stalled %s operation", async (label, stalledOperation) => {
    const plan = planEditorTsserverBootstrap({
      root: "/tmp",
      rng: () => Buffer.from(NONCE, "hex"),
      mkdir: () => {},
    });

    await expect(
      attestEditorTsserverBootstrap(
        plan,
        {
          activate: () =>
            stalledOperation === "activate" ? new Promise<never>(() => {}) : Promise.resolve(),
          configurePlugin: () =>
            stalledOperation === "configurePlugin"
              ? new Promise<never>(() => {})
              : Promise.resolve(),
          waitForAttestation: async () => ({
            version: 1,
            nonce: NONCE,
            pid: 4242,
            projects: ["/ws/tsconfig.json"],
          }),
        },
        { operationTimeoutMs: 10 },
      ),
    ).rejects.toThrow(new RegExp(`${label} timed out`, "i"));
  });

  it("times out on a receipt without a bound project", async () => {
    await expect(
      waitForEditorTsserverAttestation(
        { receiptPath: "/tmp/receipt.json", nonce: NONCE },
        {
          timeoutMs: 10,
          pollMs: 1,
          exists: () => true,
          read: () => JSON.stringify({ version: 1, nonce: NONCE, pid: 4242, projects: [] }),
        },
      ),
    ).rejects.toThrow(/unbound|timed out/i);
  });

  it("routes ONLY the explicit editor-tsserver policy to this tier", () => {
    expect(typeProviderRoutesEditorTsserver("editor-tsserver")).toBe(true);
    // This tier hands carrier rename to a plugin inside the editor's own
    // tsserver, and whether that plugin can serve a workspace depends on
    // tsserver's project topology, which Verter neither controls nor can
    // verify before the LSP has published. The automatic policy therefore
    // never selects it, and `tsserver` means the workspace tsserver the
    // setting advertises.
    expect(typeProviderRoutesEditorTsserver("auto")).toBe(false);
    expect(typeProviderRoutesEditorTsserver("shared-tsgo")).toBe(false);
    expect(typeProviderRoutesEditorTsserver("tsserver")).toBe(false);
    expect(typeProviderRoutesEditorTsserver("tsgo")).toBe(false);
    expect(typeProviderRoutesEditorTsserver("extension")).toBe(false);
    expect(typeProviderRoutesEditorTsserver("off")).toBe(false);
  });

  it("owns carrier source features only after an editor-tsserver receipt is selected", () => {
    expect(editorTsserverOwnsCarrierSourceFeatures([])).toBe(false);
    expect(
      editorTsserverOwnsCarrierSourceFeatures([
        "--editor-tsserver-receipt=/tmp/receipt.json",
        "--editor-tsserver-nonce=nonce",
      ]),
    ).toBe(true);
  });
});
