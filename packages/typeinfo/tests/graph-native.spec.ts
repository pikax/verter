/**
 * A resolve-symbol request crosses the real native boundary: the envelope
 * built by `buildTypeInfoRequest` goes through the `typeInfoWithAudit`
 * export, and the returned wire response decodes to the resolved graph.
 *
 * REGRESSION — fails if the export stops serving resolve-symbol, the wire
 * encode / decode drifts, or the answer stops naming its fact.
 */

import { toBinary } from "@bufbuild/protobuf";
import { TypeInfoGraphRequestSchema } from "@verter/proto";
import { VerterHost } from "@verter/native";
import { describe, expect, it } from "vitest";

import { buildTypeInfoRequest, decodeTypeInfoResult } from "../src/graph.js";

describe("typeInfoWithAudit (native binding, resolve-symbol)", () => {
  it("a resolve-symbol request through the native export answers its graph", () => {
    const host = new VerterHost({ auditEnabled: true });
    const canonicalId = "/fixtures/types.ts";
    host.upsert({
      canonicalId,
      inputId: canonicalId,
      source: Buffer.from("export type Foo = { msg: string };\n", "utf-8"),
    });

    const request = buildTypeInfoRequest({ canonicalId, name: "Foo" });
    const { response } = host.typeInfoWithAudit(
      Buffer.from(toBinary(TypeInfoGraphRequestSchema, request)),
    );
    const result = decodeTypeInfoResult(new Uint8Array(response));

    expect(result.kind).toBe("graph");
    if (result.kind !== "graph") throw new Error("graph arm expected");
    expect(result.root.kind).toBe("object");
    expect(result.graph.factIdentity?.queryIdentityDigest.length).toBe(32);
  });
});
