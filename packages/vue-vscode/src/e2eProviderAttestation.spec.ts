import { describe, expect, it } from "vitest";

import { attestE2eTypeProviderLog } from "./e2eProviderAttestation";

describe("E2E type-provider attestation", () => {
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

  it("accepts either attested editor tsserver or managed tsserver for the tsserver rail", () => {
    expect(
      attestE2eTypeProviderLog(
        "Type provider status: editor-tsserver (attested pid 10)",
        "tsserver",
      ),
    ).toMatchObject({ publicKind: "editor-tsserver", route: "tsserver" });
    expect(attestE2eTypeProviderLog("Type provider status: tsserver", "tsserver")).toMatchObject({
      publicKind: "tsserver",
      route: "tsserver",
    });
  });
});
