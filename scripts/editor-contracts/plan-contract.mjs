import assert from "node:assert/strict";
import path from "node:path";

const ROOT_TRANSPORTS = new Set(["argv", "initialize"]);

/**
 * Validate how an editor's production launch plan conveys its workspace root.
 *
 * Compiled clients append the root as the final positional argument. Helix's
 * shipped `languages.toml` cannot do that, so Helix relies on the standard LSP
 * initialize payload. Keeping this distinction explicit prevents a smoke-only
 * argv mutation from testing a launch shape users never run.
 */
export function assertWorkspaceRootTransport(plan, root) {
  assert.equal(
    ROOT_TRANSPORTS.has(plan.workspaceRootTransport),
    true,
    "shipping plan must declare workspaceRootTransport as argv or initialize",
  );

  if (plan.workspaceRootTransport === "argv") {
    assert.ok(plan.args.length > 0, "argv-root shipping plan must have launch arguments");
    assert.equal(
      path.resolve(plan.args.at(-1)),
      root,
      "argv-root shipping plan must keep the workspace root last",
    );
    return;
  }

  assert.equal(
    plan.args.some((arg) => typeof arg === "string" && path.resolve(arg) === root),
    false,
    "initialize-root shipping plan must not inject the workspace root into argv",
  );
  assert.equal(
    plan.args.every((arg) => typeof arg === "string" && arg.startsWith("--")),
    true,
    "initialize-root shipping plan must not contain positional arguments",
  );
}
