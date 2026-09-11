// @ai-generated - Covers replay discrimination, inventory changes, and source restoration.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import test from "node:test";

import {
  NODE_TEST_INVENTORY_PRELOAD,
  controlCommand,
  inventoryCommand,
  parseInventory,
  reapply,
} from "./closure-controls.mjs";
import { analyze, parseTerminalSummary } from "./closure-register.mjs";
import { PACKAGE_ROOT } from "./lib.mjs";

const REPO_ROOT = path.resolve(PACKAGE_ROOT, "..", "..");

const summary = (passed, failed = 0, skipped = 0) =>
  `tests ${passed + failed + skipped}\npass ${passed}\nfail ${failed}\ncancelled 0\nskipped ${skipped}\ntodo 0\n`;

// Answers the suite's registration inventory with exactly what the clean run
// before it reported, unless a case supplies its own listing, so a case about
// something else is not decided by the listing.
function withListing(spawn, listing) {
  let last = "";
  return (argv, adapter) => {
    if (argv.includes(NODE_TEST_INVENTORY_PRELOAD)) {
      if (listing !== undefined) return { status: 0, stdout: listing };
      const clean = parseTerminalSummary("node-test", last);
      return {
        status: 0,
        stdout: `node-test inventory: declared ${clean.selected} skipped ${clean.skipped}\n`,
      };
    }
    const result = spawn(argv, adapter);
    last = `${result?.stdout ?? ""}`;
    return result;
  };
}

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

const nextestListing = (matching, ignored) =>
  JSON.stringify({
    "rust-suites": {
      synthetic: {
        testcases: Object.fromEntries([
          ...Array.from({ length: matching }, (_, i) => [
            `runs_${i}`,
            { ignored: false, "filter-match": { status: "matches" } },
          ]),
          ...Array.from({ length: ignored }, (_, i) => [
            `ignored_${i}`,
            { ignored: true, "filter-match": { status: "mismatch", reason: "ignored" } },
          ]),
        ]),
      },
    },
  });

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
      reapply({ model, control, mirror, spawn: withListing(spawn) });
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
          spawn: withListing(() => {
            calls += 1;
            return clean;
          }),
        }),
      );
      assert.equal(calls, 1);
      assert.equal(fs.readFileSync(subject, "utf8"), control.reverted);
    });
});

test("replay holds the clean run to the suite's own inventory before planting", () => {
  for (const [clean, listing, refusal] of [
    // Green over fewer cases than the suite declares: the run stopped reaching
    // some of its registrations.
    [
      summary(3),
      "node-test inventory: declared 4 skipped 0\n",
      /listing of that selection on this tree holds 4/u,
    ],
    // A skip the suite's registrations do not declare.
    [summary(3, 0, 1), "node-test inventory: declared 4 skipped 0\n", /unexpected skip/u],
    // A suite whose registrations cannot be counted is refused, not trusted.
    [summary(3), "", /emitted no node-test listing/u],
  ])
    fixture(({ mirror, control, model, subject }) => {
      let calls = 0;
      assert.throws(
        () =>
          reapply({
            model,
            control,
            mirror,
            spawn: withListing(() => {
              calls += 1;
              return { status: 0, stdout: clean };
            }, listing),
          }),
        refusal,
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
          spawn: withListing(() => (++calls === 1 ? { status: 0, stdout: summary(3) } : mutated)),
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
          spawn: withListing(() => {
            if (++calls === 1) return { status: 0, stdout: summary(3) };
            assert.equal(fs.readFileSync(subject, "utf8"), applied);
            throw new Error("spawn failed");
          }),
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
      spawn: withListing((argv) => {
        calls.push(argv);
        return calls.length === 1
          ? { status: 0, stdout: summary(3) }
          : { status: 1, stdout: summary(2, 1) };
      }),
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
      spawn: (argv) => {
        if (argv[1] === "list") return { status: 0, stdout: nextestListing(9002, 547) };
        return ++calls === 1
          ? { status: 0, stdout: "Summary [ 1s] 9002 tests run: 9002 passed, 547 skipped" }
          : { status: 4, stdout: "Summary [ 1s] 0 tests run: 0 passed, 9549 skipped" };
      },
    });
    assert.equal(calls, 2);
    assert.equal(result.cleanSummary.executed, 9002);
  });
});

test("a nextest clean run is held to the runner's own listing of its selection", () => {
  fixture(({ mirror, control, model }) => {
    const adapter = model.register.adapter[0];
    adapter.runner = "cargo";
    adapter.argv_prefix = ["nextest", "run", "--locked"];
    adapter.summary_grammar = "nextest";
    model.register.proof[0].argv_tail = ["-p", "synthetic"];
    const command = {
      ...control,
      kind: "command",
      argv_delta: ["-E", "test(zzz)"],
      observed: "Summary [ 0s] 0 tests run: 0 passed, 7 skipped",
    };
    const runs = (clean, listing) => {
      const spawned = [];
      const spawn = (argv) => {
        spawned.push(argv.join(" "));
        if (argv[1] === "list") return { status: 0, stdout: listing };
        if (argv.includes("-E")) return { status: 4, stdout: command.observed };
        return { status: 0, stdout: clean };
      };
      return { spawn, spawned };
    };
    const agreeing = runs("Summary [ 1s] 6 tests run: 6 passed, 2 skipped", nextestListing(6, 2));
    reapply({ model, control: command, mirror, spawn: agreeing.spawn });
    assert.deepEqual(agreeing.spawned, [
      "nextest run --locked -p synthetic",
      "nextest list --message-format json --locked -p synthetic",
      "nextest run --locked -p synthetic -E test(zzz)",
    ]);
    assert.throws(
      () =>
        reapply({
          model,
          control: command,
          mirror,
          spawn: runs("Summary [ 1s] 5 tests run: 5 passed, 2 skipped", nextestListing(6, 2)).spawn,
        }),
      /listing of that selection on this tree holds 8/u,
    );
    assert.throws(
      () =>
        reapply({
          model,
          control: command,
          mirror,
          spawn: runs("Summary [ 1s] 4 tests run: 4 passed, 3 skipped", nextestListing(5, 2)).spawn,
        }),
      /unexpected skip/u,
    );
  });
});

test("a libtest command naming its cases with --exact must execute every named case", () => {
  fixture(({ mirror, control, model }) => {
    const adapter = model.register.adapter[0];
    adapter.runner = "cargo";
    adapter.argv_prefix = ["test", "--locked"];
    adapter.summary_grammar = "libtest";
    model.register.proof[0].argv_tail = ["-p", "synthetic", "--", "--exact", "a::one", "b::two"];
    const command = {
      ...control,
      kind: "command",
      argv_delta: ["--planted"],
      observed: "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 24 filtered out",
    };
    const listing = (names) =>
      `${names.map((name) => `${name}: test`).join("\n")}\n\n${names.length} tests, 0 benchmarks\n`;
    const clean = (passed) =>
      `test result: ok. ${passed} passed; 0 failed; 0 ignored; 0 measured; 25 filtered out\n`;
    const runs = (cleanStdout, listed) => (argv) => {
      if (argv.includes("--list")) return { status: 0, stdout: listed };
      if (argv.includes("--planted")) return { status: 101, stdout: command.observed };
      return { status: 0, stdout: cleanStdout };
    };
    reapply({
      model,
      control: command,
      mirror,
      spawn: runs(clean(2), listing(["a::one", "b::two"])),
    });
    // One named case no longer exists: the run is green and the listing agrees
    // with it, but the command's own selection is short.
    assert.throws(
      () =>
        reapply({ model, control: command, mirror, spawn: runs(clean(1), listing(["a::one"])) }),
      /names 2 cases with --exact, but the clean run executed 1/u,
    );
  });
});

test("a single-verdict tool is held to the script that re-derives its counted selection", () => {
  fixture(({ mirror, control }) => {
    const model = {
      register: {
        adapter: [{ id: "tool", runner: "node", argv_prefix: [], summary_grammar: "tool-line" }],
        proof: [
          {
            id: "dag-proof",
            control: control.id,
            adapter: "tool",
            argv_tail: ["roadmap/0.1.0-tama/tools/validate-program-dag.mjs", "--strict"],
            count_key: "nodes",
          },
        ],
      },
    };
    const command = {
      ...control,
      kind: "command",
      argv_delta: ["--planted"],
      observed: "ERROR: planted edge\n",
    };
    const [script] = inventoryCommand(model.register.adapter[0], model.register.proof[0].argv_tail);
    const runs = (nodes, listing) => (argv) => {
      if (argv[0] === script) return { status: 0, stdout: listing };
      if (argv.includes("--planted")) return { status: 1, stdout: command.observed };
      return { status: 0, stdout: `validate-program-dag: PASS nodes=${nodes} edges=9\n` };
    };
    reapply({
      model,
      control: command,
      mirror,
      spawn: runs(427, "tool-line inventory: nodes=427\n"),
    });
    assert.throws(
      () =>
        reapply({
          model,
          control: command,
          mirror,
          spawn: runs(425, "tool-line inventory: nodes=426\n"),
        }),
      /listing of that selection on this tree holds 426/u,
    );
    assert.throws(
      () =>
        reapply({
          model,
          control: command,
          mirror,
          spawn: runs(426, "tool-line inventory: edges=426\n"),
        }),
      /counts nodes, but the inventory of .* counts edges/u,
    );
  });
});

test("the inventory helpers count what the real runners select on this tree", () => {
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  const run = (argv, cwd = REPO_ROOT) =>
    spawnSync(process.execPath, argv, { cwd, env, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });

  const dag = "roadmap/0.1.0-tama/tools/validate-program-dag.mjs";
  const validated = run([dag, "--strict"]);
  assert.equal(validated.status, 0, validated.stderr);
  const listed = run(inventoryCommand({ summary_grammar: "tool-line" }, [dag, "--strict"]));
  assert.equal(listed.status, 0, listed.stderr);
  assert.equal(
    parseInventory("tool-line", listed.stdout).selected,
    parseTerminalSummary("tool-line", validated.stdout, "nodes").selected,
  );

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "closure-replay-inventory-"));
  try {
    const suite = path.join(dir, "suite.test.mjs");
    fs.writeFileSync(
      suite,
      [
        'import test, { describe } from "node:test";',
        'test("a", () => {});',
        'test.skip("b", () => {});',
        'describe("group", () => {',
        '  test("c", () => {});',
        '  test("d", { skip: true }, () => {});',
        "});",
        "",
      ].join("\n"),
    );
    const inventory = run(inventoryCommand({ summary_grammar: "node-test" }, ["--test", suite]));
    assert.equal(inventory.status, 0, inventory.stderr);
    const real = run(["--test", suite], dir);
    assert.equal(real.status, 0, real.stderr);
    const counted = parseTerminalSummary("node-test", real.stdout);
    assert.deepEqual(parseInventory("node-test", inventory.stdout), {
      selected: counted.selected,
      skipped: counted.skipped,
    });
    assert.deepEqual(parseInventory("node-test", inventory.stdout), { selected: 4, skipped: 2 });
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("every recorded node suite and the DAG validator record replay against an inventory", () => {
  const { model } = analyze(PACKAGE_ROOT);
  const inventoried = { "node-test": 0, "tool-line": 0 };
  for (const control of model.register.control) {
    const { adapter, argv } = controlCommand(model, control);
    if (adapter.summary_grammar === "node-test") {
      assert.ok(inventoryCommand(adapter, argv), `${control.id}: a node suite with no inventory`);
      inventoried["node-test"] += 1;
    } else if (adapter.summary_grammar === "tool-line" && inventoryCommand(adapter, argv))
      inventoried["tool-line"] += 1;
  }
  assert.ok(inventoried["node-test"] >= 1, "no recorded control replays a node suite");
  assert.ok(inventoried["tool-line"] >= 1, "no recorded control replays an inventoried tool");
});
