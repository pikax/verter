/** Type-system guard for the retired style-ordinal RPC. */
import { describe, it } from "vitest";

import { RequestType } from "./request";

// A named type import must remain impossible. If the ordinal payload is
// exported again, TypeScript reports this directive as unused.
// @ts-expect-error StyleOverrideParam was retired with the ordinal RPC.
import type { StyleOverrideParam } from "./request";

describe("sealed block-content request surface", () => {
  it("does not expose the retired style-ordinal method", () => {
    // @ts-expect-error All supplied bytes use the host's stamped block handoff.
    const retired = RequestType.ApplyStyleOverrides;
    void retired;
  });
});
