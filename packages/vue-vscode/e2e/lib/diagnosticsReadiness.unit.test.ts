import { afterEach, expect, it, vi } from "vitest";
import { waitForDiagnosticReceipt } from "./diagnosticsReadiness";

afterEach(() => vi.useRealTimers());

it("does not accept a quiet native batch or a provider receipt for an earlier edit", async () => {
  vi.useFakeTimers();
  const since = Date.now();
  let status = { version: 1, ready: false };
  let diagnostics: string[] = [];
  let finished = false;
  const result = waitForDiagnosticReceipt(
    async () => status,
    () => 2,
    () => since,
    () => diagnostics,
    2000,
    500,
  ).then((value) => {
    finished = true;
    return value;
  });
  await vi.advanceTimersByTimeAsync(700);
  expect(finished).toBe(false);
  status = { version: 1, ready: true };
  await vi.advanceTimersByTimeAsync(100);
  expect(finished).toBe(false);
  diagnostics = ["TypeScript error"];
  status = { version: 2, ready: true };
  await vi.advanceTimersByTimeAsync(550);
  expect(await result).toEqual(["TypeScript error"]);
});

it("waits for the editor collection after first observing provider completion", async () => {
  vi.useFakeTimers();
  let stableSince = Date.now();
  let ready = false;
  let diagnostics: string[] = [];
  let finished = false;
  const result = waitForDiagnosticReceipt(
    async () => ({ version: 1, ready }),
    () => 1,
    () => stableSince,
    () => diagnostics,
    2000,
    500,
  ).then((value) => {
    finished = true;
    return value;
  });
  await vi.advanceTimersByTimeAsync(700);
  ready = true;
  await vi.advanceTimersByTimeAsync(50);
  expect(finished).toBe(false);
  diagnostics = ["provider error"];
  stableSince = Date.now();
  await vi.advanceTimersByTimeAsync(500);
  expect(await result).toEqual(["provider error"]);
});

it("fails the deadline when provider diagnostics never complete", async () => {
  vi.useFakeTimers();
  const result = waitForDiagnosticReceipt(
    async () => undefined,
    () => 1,
    () => 0,
    () => [],
    500,
    100,
  );
  const rejected = expect(result).rejects.toThrow("did not complete");
  await vi.advanceTimersByTimeAsync(500);
  await rejected;
});

it("keeps the deadline when the readiness request itself stalls", async () => {
  vi.useFakeTimers();
  const result = waitForDiagnosticReceipt(
    () => new Promise(() => {}),
    () => 1,
    () => 0,
    () => [],
    500,
    100,
  );
  const rejected = expect(result).rejects.toThrow("did not complete");
  await vi.advanceTimersByTimeAsync(500);
  await rejected;
});
