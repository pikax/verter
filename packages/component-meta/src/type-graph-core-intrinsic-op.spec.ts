import { describe, expect, it } from "vitest";
import { CompilerIntrinsicTypeOp } from "@verter/proto";
import * as core from "./type-graph-core.js";

const CORE_PREFIX = "INTRINSIC_OP_";

// The core module carries no protobuf runtime, so it mirrors the shared proto
// enum by hand. This pins the mirror to the generated enum in both directions.
describe("intrinsic op codes", () => {
  it("mirror the shared proto CompilerIntrinsicTypeOp exactly", () => {
    const proto = Object.fromEntries(
      Object.entries(CompilerIntrinsicTypeOp).filter(
        (entry): entry is [string, number] => typeof entry[1] === "number",
      ),
    );
    const mirrored = Object.fromEntries(
      Object.entries(core)
        .filter(([name]) => name.startsWith(CORE_PREFIX))
        .map(([name, value]) => [name.slice(CORE_PREFIX.length), value]),
    );
    expect(mirrored).toEqual(proto);
  });

  it("names every real op and only the real ops", () => {
    for (const [name, op] of Object.entries(CompilerIntrinsicTypeOp)) {
      if (typeof op !== "number") continue;
      const display = core.intrinsicOpDisplayName(op);
      if (op === CompilerIntrinsicTypeOp.UNSPECIFIED) {
        expect(display, name).toBe("unknown intrinsic");
      } else {
        expect(display, name).not.toBe("unknown intrinsic");
      }
    }
  });
});
