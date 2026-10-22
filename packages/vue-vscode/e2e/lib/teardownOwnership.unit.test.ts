import { describe, expect, it } from "vitest";
import type { ProcessRecord } from "../processInventory";
import { ownedProcessesUnder, survivingOwned } from "./teardownOwnership";

const row = (pid: number, parentPid: number, name: string, commandLine = name): ProcessRecord => ({
  pid,
  parentPid,
  name,
  commandLine,
});

const HOST = 100;
const inventory = [
  row(HOST, 1, "code"),
  row(200, HOST, "verter-lsp"),
  row(201, 200, "tsgo", "tsgo --lsp"),
  // Not owned: a sibling engine the editor started, and an unrelated process.
  row(300, HOST, "tsgo", "tsgo --lsp --shared"),
  row(400, 1, "verter-lsp"),
];

describe("ownedProcessesUnder", () => {
  it("owns the host's Verter LSP and what it spawned, not shared engines or other hosts' servers", () => {
    expect(ownedProcessesUnder(inventory, HOST).map((p) => p.pid)).toEqual([200, 201]);
  });
});

describe("survivingOwned", () => {
  const recorded = ownedProcessesUnder(inventory, HOST);

  it("reports an owned process that is still running", () => {
    expect(survivingOwned(recorded, [row(201, 1, "tsgo", "tsgo --lsp")]).map((p) => p.pid)).toEqual(
      [201],
    );
  });

  it("does not mistake a recycled pid for the owned process", () => {
    expect(survivingOwned(recorded, [row(200, 1, "notepad")])).toEqual([]);
  });

  it("reports nothing once every owned process is gone", () => {
    expect(survivingOwned(recorded, [row(300, 1, "tsgo", "tsgo --lsp --shared")])).toEqual([]);
  });
});
