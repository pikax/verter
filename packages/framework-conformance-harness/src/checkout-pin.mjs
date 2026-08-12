// Verifies a local git checkout is byte-exactly the pinned official source
// commit/tree from domain-pin.mjs, and refuses (throws) on any drift: wrong
// HEAD, wrong tree, or a dirty working tree. This is the harness's
// source-drift refusal for the two GIT SOURCE domains (Vue, Svelte).
//
// Mirrors the assertCheckout() pattern already proven in
// docs/arch/refactor/rev11/evidence/framework-conformance/generate-official-case-manifests.mjs,
// factored out so the harness's own self-tests can exercise it directly
// against a deliberately-mutated checkout.

import { execFileSync } from "node:child_process";

export class CheckoutDriftError extends Error {
  constructor(message, details) {
    super(message);
    this.name = "CheckoutDriftError";
    this.details = details;
  }
}

function git(cwd, ...args) {
  return execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8" }).trim();
}

/**
 * @param {string} checkoutRoot local path to a git working tree
 * @param {{ commit: string, tree: string }} domain from domain-pin.mjs
 * @returns {{ commit: string, tree: string }} the verified identity
 */
export function assertCheckoutPinned(checkoutRoot, domain) {
  let head;
  try {
    head = git(checkoutRoot, "rev-parse", "HEAD");
  } catch (error) {
    throw new CheckoutDriftError(`${checkoutRoot}: not a git checkout (${error.message})`, {
      checkoutRoot,
    });
  }
  if (head !== domain.commit) {
    throw new CheckoutDriftError(
      `${checkoutRoot}: HEAD drift — expected commit ${domain.commit}, got ${head}`,
      { checkoutRoot, expected: domain.commit, actual: head, kind: "commit" },
    );
  }
  const tree = git(checkoutRoot, "rev-parse", "HEAD^{tree}");
  if (tree !== domain.tree) {
    throw new CheckoutDriftError(
      `${checkoutRoot}: tree drift — expected tree ${domain.tree}, got ${tree}`,
      { checkoutRoot, expected: domain.tree, actual: tree, kind: "tree" },
    );
  }
  const status = git(checkoutRoot, "status", "--porcelain");
  if (status !== "") {
    throw new CheckoutDriftError(`${checkoutRoot}: dirty checkout (uncommitted changes present)`, {
      checkoutRoot,
      status,
      kind: "dirty",
    });
  }
  return { commit: head, tree };
}

/** Reads a single file's blob at HEAD without touching the working tree. */
export function readPinnedBlob(checkoutRoot, relativePath) {
  return git(checkoutRoot, "show", `HEAD:${relativePath}`);
}
