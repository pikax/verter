import { describe, expect, it } from "vitest";

import {
  assertSharedTsgoServedWithoutFallback,
  attestE2eTypeProviderLog,
} from "./e2eProviderAttestation";

describe("E2E type-provider attestation", () => {
  it("accepts a deliberate provider-off route only when the public status is none", () => {
    expect(
      attestE2eTypeProviderLog(
        "Type provider status: none (no configured TypeScript project (tsconfig.json))",
        "off",
      ),
    ).toMatchObject({ publicKind: "none", route: "off" });
    expect(() => attestE2eTypeProviderLog("Type provider status: tsgo", "off")).toThrow(
      /requested off.*reported tsgo/i,
    );
  });

  it("rejects the former vacuous managed-tsgo run before feature assertions", () => {
    expect(() =>
      attestE2eTypeProviderLog(
        "Type provider status: none (tsgo binary not found)\nTypeProviderSyncComplete (init generation 1)",
        "tsgo",
      ),
    ).toThrow(/requested managed tsgo.*reported none/i);
  });

  it("distinguishes owned tsgo from the shared editor route", () => {
    expect(attestE2eTypeProviderLog("Type provider status: tsgo", "tsgo")).toMatchObject({
      publicKind: "tsgo",
      route: "managed-tsgo",
    });

    expect(() =>
      attestE2eTypeProviderLog(
        "[shared-tsgo] armed: shim=x realTsgo=y controlDir=z (SHARED editor-attach overlay will bind lazily per query)\n" +
          "Type provider status: tsgo (attested editor-owned Native Preview Program)",
        "tsgo",
      ),
    ).toThrow(/requested managed tsgo.*shared route was armed/i);
  });

  it("requires shared-tsgo provenance and an armed editor rendezvous", () => {
    expect(() => attestE2eTypeProviderLog("Type provider status: tsgo", "shared-tsgo")).toThrow(
      /missing editor-owned Native Preview provenance/i,
    );

    expect(
      attestE2eTypeProviderLog(
        "[shared-tsgo] armed: shim=x realTsgo=y controlDir=z (SHARED editor-attach overlay will bind lazily per query)\n" +
          "Type provider status: tsgo (attested editor-owned Native Preview Program; managed TSGO remains cold)",
        "shared-tsgo",
      ),
    ).toMatchObject({ publicKind: "tsgo", route: "shared-tsgo" });
  });

  it("holds each tsserver-family rail to the exact engine it asked for", () => {
    expect(attestE2eTypeProviderLog("Type provider status: tsserver", "tsserver")).toMatchObject({
      publicKind: "tsserver",
      route: "tsserver",
    });
    expect(
      attestE2eTypeProviderLog(
        "Type provider status: editor-tsserver (attested pid 10)",
        "editor-tsserver",
      ),
    ).toMatchObject({ publicKind: "editor-tsserver", route: "editor-tsserver" });
    // The two rails are distinct engines with distinct topologies. Accepting the
    // editor plugin for a `tsserver` run is what let a route that served nothing
    // pass as the workspace tsserver.
    expect(() =>
      attestE2eTypeProviderLog(
        "Type provider status: editor-tsserver (attested pid 10)",
        "tsserver",
      ),
    ).toThrow(/reported editor-tsserver/);
    expect(() =>
      attestE2eTypeProviderLog("Type provider status: tsserver", "editor-tsserver"),
    ).toThrow(/reported tsserver/);
  });

  it("accepts the extension-hosted route only on its own topology", () => {
    // The extension-hosted service reports the tsserver KIND, so the kind line
    // alone cannot tell it apart from a run that fell back to the workspace
    // tsserver. The topology line is the discriminator, and a run missing it —
    // or reporting the workspace engine — must not attest as `extension`.
    const served =
      "Type provider status: tsserver (extension-hosted TypeScript language service (Experiment E))\n" +
      "Type provider topology: extension-hosted";
    expect(attestE2eTypeProviderLog(served, "extension")).toEqual({
      publicKind: "tsserver",
      reason: "extension-hosted TypeScript language service (Experiment E",
      route: "extension",
    });

    expect(() =>
      attestE2eTypeProviderLog(
        "Type provider status: tsserver (workspace TypeScript 5.9.2)\n" +
          "Type provider topology: project-tsserver",
        "extension",
      ),
    ).toThrow(/topology was project-tsserver/);

    expect(() =>
      attestE2eTypeProviderLog(
        "Type provider status: tsserver (workspace TypeScript 5.9.2)",
        "extension",
      ),
    ).toThrow(/topology was unreported/);

    expect(() =>
      attestE2eTypeProviderLog(
        "Type provider status: none (no workspace TypeScript resolved)",
        "extension",
      ),
    ).toThrow(/reported none/);
  });

  it("requires an actual shared feature result and rejects every managed fallback signal", () => {
    expect(() =>
      assertSharedTsgoServedWithoutFallback(
        "[shared-tsgo] armed: shim=x realTsgo=y controlDir=z\n" +
          "Type provider status: tsgo (attested editor-owned Native Preview Program)",
      ),
    ).toThrow(/no carrier feature was served/i);

    expect(() =>
      assertSharedTsgoServedWithoutFallback(
        "editor-owned tsgo served carrier feature; managed fallback remained cold\n" +
          "editor-owned tsgo diagnostics did not engage; activating managed fallback",
      ),
    ).toThrow(/managed fallback was activated/i);

    expect(() =>
      assertSharedTsgoServedWithoutFallback(
        "editor-owned tsgo served carrier diagnostics; managed fallback remained cold",
      ),
    ).not.toThrow();
  });
});
