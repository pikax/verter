import { describe, expect, it } from "vitest";

import {
  assertSharedTsgoServedWithoutFallback,
  attestE2eTypeProviderLog,
} from "./e2eProviderAttestation";

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
