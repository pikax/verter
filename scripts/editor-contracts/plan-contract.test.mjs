import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import { assertWorkspaceRootTransport } from "./plan-contract.mjs";

const root = path.resolve("fixture-root");

test("accepts a Helix-style rootless argv carried by initialize", () => {
  assert.doesNotThrow(() =>
    assertWorkspaceRootTransport(
      {
        workspaceRootTransport: "initialize",
        args: ["--type-provider=tsgo"],
      },
      root,
    ),
  );
});

test("rejects a smoke-only positional root on an initialize-root plan", () => {
  assert.throws(
    () =>
      assertWorkspaceRootTransport(
        {
          workspaceRootTransport: "initialize",
          args: ["--type-provider=tsgo", root],
        },
        root,
      ),
    /must not inject.*root/i,
  );
});

test("requires argv-root clients to keep the exact root last", () => {
  assert.doesNotThrow(() =>
    assertWorkspaceRootTransport(
      {
        workspaceRootTransport: "argv",
        args: ["--type-provider=tsgo", root],
      },
      root,
    ),
  );
  assert.throws(
    () =>
      assertWorkspaceRootTransport(
        {
          workspaceRootTransport: "argv",
          args: [root, "--type-provider=tsgo"],
        },
        root,
      ),
    /root last/i,
  );
});

test("rejects an undeclared root transport", () => {
  assert.throws(
    () => assertWorkspaceRootTransport({ args: ["--type-provider=tsgo"] }, root),
    /must declare workspaceRootTransport/i,
  );
});
