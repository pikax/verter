/**
 * Type-level pins for the `$/verter/applyStyleOverrides` wire contract.
 *
 * The server refuses a tokenless or partial-pair apply with a typed
 * `missingTokens` refusal and never applies unfenced, so the shared TS
 * mirror must (a) REQUIRE both captured structure tokens on the request
 * and (b) represent `missingTokens` alongside `revisionMismatch` on the
 * response refusal union.
 *
 * Checked by the vitest `tsc` typechecker (`vitest.config.ts` →
 * `test.typecheck`), not emitted or bundled.
 */
import { describe, it } from "vitest";

import { RequestType, type RequestParams, type RequestResponse } from "./request";

type ApplyParams = RequestParams[RequestType.ApplyStyleOverrides];
type ApplyResponse = RequestResponse[RequestType.ApplyStyleOverrides];

describe("applyStyleOverrides wire contract", () => {
  it("requires BOTH captured structure tokens on the request", () => {
    // A fully-tokened request type-checks.
    const tokened: ApplyParams = {
      uri: "file:///a.vue",
      overrides: [],
      documentRevisionToken: "rev-1",
      artifactToken: "artifact-1",
    };

    // @ts-expect-error — a tokenless request must NOT type-check: the server
    // refuses a revision-unvalidatable apply, so typed clients must not be
    // able to compile one.
    const tokenless: ApplyParams = { uri: "file:///a.vue", overrides: [] };

    // @ts-expect-error — a revision-only partial pair must NOT type-check.
    const revisionOnly: ApplyParams = {
      uri: "file:///a.vue",
      overrides: [],
      documentRevisionToken: "rev-1",
    };

    // @ts-expect-error — an artifact-only partial pair must NOT type-check.
    const artifactOnly: ApplyParams = {
      uri: "file:///a.vue",
      overrides: [],
      artifactToken: "artifact-1",
    };

    void [tokened, tokenless, revisionOnly, artifactOnly];
  });

  it("represents both refusal arms and keeps the union closed", () => {
    // `missingTokens` is representable — the server returns it for an
    // absent/partial token pair.
    const missingTokens: ApplyResponse = { success: false, refusal: "missingTokens" };
    // `revisionMismatch` stays representable.
    const revisionMismatch: ApplyResponse = { success: false, refusal: "revisionMismatch" };
    // A refusal-less success stays representable.
    const success: ApplyResponse = { success: true };

    // @ts-expect-error — the refusal union is CLOSED: unknown arms must not
    // type-check.
    const unknownRefusal: ApplyResponse = { success: false, refusal: "torn" };

    void [missingTokens, revisionMismatch, success, unknownRefusal];
  });
});
