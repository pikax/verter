import { describe, expect, it } from "vitest";

import { DesktopLanguageLifecycle, connectionErrorDisposition } from "./desktopLanguageLifecycle";

describe("DesktopLanguageLifecycle", () => {
  it("admits each lost server into recovery once, and never a replaced one", () => {
    const lifecycle = new DesktopLanguageLifecycle(5);
    const first = lifecycle.beginEpoch();

    expect(lifecycle.reportLost(first, "the server process exited")).toBe(true);
    // The failed start the close causes reports the same loss.
    expect(lifecycle.reportLost(first, "the server failed to start")).toBe(false);

    const second = lifecycle.beginEpoch();
    expect(lifecycle.isCurrent(first)).toBe(false);
    expect(lifecycle.markRunning(first)).toBe(false);
    expect(lifecycle.reportLost(first, "late close")).toBe(false);
    expect(lifecycle.snapshot().phase).toBe("starting");

    expect(lifecycle.markRunning(second)).toBe(true);
    expect(lifecycle.reportLost(second, "the server process exited")).toBe(true);
  });

  it("keeps the restart attempt visible across the restart's own start, and clears it once running", () => {
    const lifecycle = new DesktopLanguageLifecycle(5);
    lifecycle.markRunning(lifecycle.beginEpoch());
    lifecycle.markRecovering(2, "the server process exited");

    const restarted = lifecycle.beginEpoch();
    expect(lifecycle.snapshot()).toMatchObject({
      phase: "starting",
      restartAttempt: 2,
      reason: "the server process exited",
    });

    lifecycle.markRunning(restarted);
    expect(lifecycle.snapshot()).toEqual({
      phase: "running",
      epoch: restarted,
      restartAttempt: 0,
      maxRestarts: 5,
    });
  });

  it("admits no further loss once stopped, until a new epoch begins", () => {
    const lifecycle = new DesktopLanguageLifecycle(5);
    const epoch = lifecycle.beginEpoch();
    lifecycle.markStopped("5 automatic restarts failed");

    expect(lifecycle.reportLost(epoch, "late close")).toBe(false);
    expect(lifecycle.snapshot().phase).toBe("stopped");

    const manual = lifecycle.beginEpoch();
    expect(lifecycle.reportLost(manual, "the server process exited")).toBe(true);
  });

  it("goes quiet when disposed: no epoch is current and no listener fires", () => {
    const lifecycle = new DesktopLanguageLifecycle(5);
    const epoch = lifecycle.beginEpoch();
    let changes = 0;
    lifecycle.onDidChange(() => {
      changes += 1;
    });

    lifecycle.dispose();
    lifecycle.markStopped("after disposal");
    lifecycle.markRecovering(1, "after disposal");

    expect(lifecycle.isCurrent(epoch)).toBe(false);
    expect(lifecycle.reportLost(epoch, "after disposal")).toBe(false);
    expect(lifecycle.snapshot().phase).toBe("disposed");
    expect(changes).toBe(0);
  });
});

describe("connectionErrorDisposition", () => {
  it("tolerates three connection errors and shuts the fourth down", () => {
    expect([1, 2, 3, 4, undefined].map(connectionErrorDisposition)).toEqual([
      "continue",
      "continue",
      "continue",
      "shutdown",
      "shutdown",
    ]);
  });
});
