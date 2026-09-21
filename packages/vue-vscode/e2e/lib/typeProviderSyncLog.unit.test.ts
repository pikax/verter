import { expect, it } from "vitest";
import { typeProviderSyncCompleteSince } from "./typeProviderSyncLog";

const FIRST_SERVER =
  "Verter ready (init generation 1)\nTypeProviderSyncComplete (init generation 1)\n";

it("does not let an earlier server's completion satisfy a restarted server", () => {
  // Init generations restart at 1 with every server process, so the first
  // server's lines carry exactly the numbers the restarted one will use.
  const restartedAt = FIRST_SERVER.length;
  expect(typeProviderSyncCompleteSince(FIRST_SERVER, restartedAt)).toBe("awaiting-ready");

  const booting = `${FIRST_SERVER}Verter ready (init generation 1)\n`;
  expect(typeProviderSyncCompleteSince(booting, restartedAt)).toBe("awaiting-sync");

  const synced = `${booting}TypeProviderSyncComplete (init generation 1)\n`;
  expect(typeProviderSyncCompleteSince(synced, restartedAt)).toBe("complete");
});

it("accepts the first server's completion when nothing was restarted", () => {
  expect(typeProviderSyncCompleteSince(FIRST_SERVER, 0)).toBe("complete");
});

it("ignores a completion for a generation older than the latest ready one", () => {
  const superseded =
    "Verter ready (init generation 1)\nVerter ready (init generation 2)\n" +
    "TypeProviderSyncComplete (init generation 1)\n";
  expect(typeProviderSyncCompleteSince(superseded, 0)).toBe("awaiting-sync");
  expect(
    typeProviderSyncCompleteSince(`${superseded}TypeProviderSyncComplete (init generation 2)\n`, 0),
  ).toBe("complete");
});
