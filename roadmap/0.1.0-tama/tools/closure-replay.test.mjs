// @ai-generated - Covers replay discrimination, inventory changes, and source restoration.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { reapply } from "./closure-controls.mjs";

const summary = (passed, failed = 0, skipped = 0) =>
  `tests ${passed + failed + skipped}\npass ${passed}\nfail ${failed}\ncancelled 0\nskipped ${skipped}\ntodo 0\n`;

function fixture(body) {
  const mirror = fs.mkdtempSync(path.join(os.tmpdir(), "closure-replay-unit-"));
  const control = {
    id: "guard",
    kind: "source",
    subject: "guard.mjs",
    reverted: "export const reject = true;\n",
    applied: "export const reject = false;\n",
    observed: summary(2, 1),
  };
  const model = {
    register: {
      adapter: [
        { id: "node", runner: "node", argv_prefix: ["--test"], summary_grammar: "node-test" },
      ],
      proof: [
        {
          id: "guard-proof",
          control: control.id,
          adapter: "node",
          argv_tail: ["guard.test.mjs"],
          terminal_summary: summary(3),
        },
      ],
    },
  };
  const subject = path.join(mirror, control.subject);
  fs.writeFileSync(subject, control.reverted);
  try {
    body({ mirror, control, model, subject });
  } finally {
    fs.rmSync(mirror, { recursive: true, force: true });
  }
}

test("replay accepts a changed passing inventory and restores the planted source", () => {
  for (const total of [8, 2])
    fixture(({ mirror, control, model, subject }) => {
      let calls = 0;
      const spawn = () => {
        calls += 1;
        const planted = fs.readFileSync(subject, "utf8") === control.applied;
        return {
          status: planted ? 1 : 0,
          stdout: summary(total - (planted ? 1 : 0), planted ? 1 : 0, 4),
        };
      };
      reapply({ model, control, mirror, spawn });
      assert.equal(calls, 2);
      assert.equal(fs.readFileSync(subject, "utf8"), control.reverted);
    });
});

test("replay rejects empty, incomplete, and failing clean runs before planting", () => {
  for (const clean of [
    { status: 0, stdout: summary(0, 0, 4) },
    { status: 0, stdout: summary(2, 1) },
    { status: 0, stdout: "no terminal summary" },
    { status: 0, stdout: summary(3).replace("tests 3", "tests 4") },
    { status: 1, stdout: summary(3) },
    { status: null, signal: "SIGTERM", stdout: summary(3) },
  ])
    fixture(({ mirror, control, model, subject }) => {
      let calls = 0;
      assert.throws(() =>
        reapply({
          model,
          control,
          mirror,
          spawn: () => {
            calls += 1;
            return clean;
          },
        }),
      );
      assert.equal(calls, 1);
      assert.equal(fs.readFileSync(subject, "utf8"), control.reverted);
    });
});

test("replay rejects a surviving mutation or a different refusal and restores the source", () => {
  for (const mutated of [
    { status: 0, stdout: summary(3) },
    { status: 1, stdout: summary(1, 2) },
    { status: 1, stdout: "unrelated crash without a test summary" },
    { status: null, signal: "SIGTERM", stdout: summary(2, 1) },
  ])
    fixture(({ mirror, control, model, subject }) => {
      let calls = 0;
      assert.throws(() =>
        reapply({
          model,
          control,
          mirror,
          spawn: () => (++calls === 1 ? { status: 0, stdout: summary(3) } : mutated),
        }),
      );
      assert.equal(calls, 2);
      assert.equal(fs.readFileSync(subject, "utf8"), control.reverted);
    });
});

test("replay preserves replacement bytes and restores CRLF source after a spawn exception", () => {
  fixture(({ mirror, control, model, subject }) => {
    const original = control.reverted.replaceAll("\n", "\r\n");
    fs.writeFileSync(subject, original);
    const applied = `${control.applied}// $& $1 $$ $\` $' \${{ github.sha }}\n`;
    let calls = 0;
    assert.throws(
      () =>
        reapply({
          model,
          control: { ...control, applied },
          mirror,
          spawn: () => {
            if (++calls === 1) return { status: 0, stdout: summary(3) };
            assert.equal(fs.readFileSync(subject, "utf8"), applied);
            throw new Error("spawn failed");
          },
        }),
      /spawn failed/u,
    );
    assert.equal(calls, 2);
    assert.equal(fs.readFileSync(subject, "utf8"), original);
  });
});

test("replay refuses unapplied or already present mutations without running a command", () => {
  fixture(({ mirror, control, model, subject }) => {
    for (const text of [control.applied, control.reverted.repeat(2), "unrelated source"]) {
      fs.writeFileSync(subject, text);
      let calls = 0;
      assert.throws(() =>
        reapply({
          model,
          control,
          mirror,
          spawn: () => {
            calls += 1;
          },
        }),
      );
      assert.equal(calls, 0);
      assert.equal(fs.readFileSync(subject, "utf8"), text);
    }
  });
});

test("a command mutation appends only new arguments and writes no source", () => {
  fixture(({ mirror, control, model, subject }) => {
    const command = { ...control, kind: "command", argv_delta: ["--empty-selection"] };
    const calls = [];
    reapply({
      model,
      control: command,
      mirror,
      spawn: (argv) => {
        calls.push(argv);
        return calls.length === 1
          ? { status: 0, stdout: summary(3) }
          : { status: 1, stdout: summary(2, 1) };
      },
    });
    assert.deepEqual(calls[1], [...calls[0], "--empty-selection"]);
    assert.equal(fs.readFileSync(subject, "utf8"), control.reverted);
    assert.throws(
      () =>
        reapply({
          model,
          control: { ...command, argv_delta: ["--test"] },
          mirror,
          spawn: () => assert.fail("must not run"),
        }),
      /already part of the command/u,
    );
  });
});

test("an empty nextest selector is detected independently of passing and skipped totals", () => {
  fixture(({ mirror, control, model }) => {
    const adapter = model.register.adapter[0];
    adapter.runner = "cargo";
    adapter.argv_prefix = ["nextest", "run"];
    adapter.summary_grammar = "nextest";
    model.register.proof[0].terminal_summary =
      "Summary [ 1s] 8997 tests run: 8997 passed, 547 skipped";
    const command = {
      ...control,
      kind: "command",
      argv_delta: ["-E", "test(absent)"],
      observed: "Summary [ 1s] 0 tests run: 0 passed, 9544 skipped",
    };
    let calls = 0;
    const result = reapply({
      model,
      control: command,
      mirror,
      spawn: () =>
        ++calls === 1
          ? { status: 0, stdout: "Summary [ 1s] 9002 tests run: 9002 passed, 547 skipped" }
          : { status: 4, stdout: "Summary [ 1s] 0 tests run: 0 passed, 9549 skipped" },
    });
    assert.equal(calls, 2);
    assert.equal(result.cleanSummary.executed, 9002);
  });
});
