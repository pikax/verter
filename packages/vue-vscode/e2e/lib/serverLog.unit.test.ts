import { describe, expect, it } from "vitest";
import { didOpenHandlersInFlight } from "./serverLog";

const enter = (n: number) =>
  `2026-09-22T10:15:32.001402Z  INFO verter_lsp::server::handler_guard: HANDLER_ENTER did_open active=${n} thread=ThreadId(2)\n`;
const exit = (n: number) =>
  `2026-09-22T10:15:36.297838Z  INFO verter_lsp::server::handler_guard: HANDLER_EXIT did_open active=${n} elapsed=5.183847705s thread=ThreadId(2)\n`;
const hover =
  "2026-09-22T10:15:36.021346Z  INFO verter_lsp::server::handler_guard: HANDLER_ENTER hover active=61 thread=ThreadId(2)\n";
const lifecycle =
  "2026-09-22T10:15:32.001402Z  INFO verter_lsp::server::lifecycle: did_open: file:///fixtures/vue-parity/src/matrix/SlotsEmits.vue\n";

describe("didOpenHandlersInFlight", () => {
  it("is zero for a log with no opens", () => {
    expect(didOpenHandlersInFlight("")).toBe(0);
    expect(didOpenHandlersInFlight(hover + lifecycle)).toBe(0);
  });

  it("counts opens the server entered but has not exited", () => {
    // The replay storm: three opens queued, one finished.
    const log = enter(1) + enter(2) + enter(3) + exit(2);
    expect(didOpenHandlersInFlight(log)).toBe(2);
    expect(didOpenHandlersInFlight(log + exit(1) + exit(0))).toBe(0);
  });

  it("ignores every other handler and the lifecycle line", () => {
    const log = hover + lifecycle + enter(1) + hover + exit(0);
    expect(didOpenHandlersInFlight(log)).toBe(0);
    expect(didOpenHandlersInFlight(hover + lifecycle + enter(1))).toBe(1);
  });

  it("reads only the lines written after the mark", () => {
    const before = enter(1) + enter(2);
    const after = enter(1) + exit(0);
    expect(didOpenHandlersInFlight(before + after, before.length)).toBe(0);
    expect(didOpenHandlersInFlight(before + after)).toBe(2);
  });

  it("never lets an exit that belongs to the previous server drive the count negative", () => {
    // The old server's open finished after the mark was taken; the new one has
    // not opened anything yet, and its first open must still count as one.
    const log = exit(0) + exit(0) + enter(1);
    expect(didOpenHandlersInFlight(log)).toBe(1);
  });
});
