import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(here, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");

const read = (rel: string) => readFileSync(path.join(repoRoot, rel), "utf8");

/**
 * The packaged VSIX must actually carry the engine it advertises.
 *
 * `release.yml` cross-compiles a per-platform `verter-lsp`, copies it into
 * `packages/vue-vscode/bin/`, and `findLspBinary` resolves
 * `<extensionPath>/bin/verter-lsp` as the bundled path. `stageShimBinary`
 * prunes every `bin/` entry outside its whitelist, so the engine only survives
 * packaging if the whitelist admits it.
 */
describe("VSIX engine payload", () => {
  const stageBin = read("packages/vue-vscode/stage-bin.mjs");

  it("whitelists the LSP engine so staging does not delete it", () => {
    const match = stageBin.match(/EXTRA_ALLOWED_BIN_ENTRIES\s*=\s*\[([^\]]*)\]/);
    expect(match, "EXTRA_ALLOWED_BIN_ENTRIES must exist in stage-bin.mjs").toBeTruthy();

    const entries = match![1];
    expect(
      entries.includes("verter-lsp"),
      "stage-bin.mjs prunes every bin/ entry outside its whitelist. Without `verter-lsp` " +
        "the release workflow's pre-staged engine is deleted before `vsce package`, and the " +
        "published VSIX ships with no engine (extension.ts falls through to a PATH lookup).",
    ).toBe(true);
  });

  it("whitelists the Windows engine filename too", () => {
    const match = stageBin.match(/EXTRA_ALLOWED_BIN_ENTRIES\s*=\s*\[([^\]]*)\]/);
    expect(
      match![1].includes("verter-lsp.exe"),
      "Windows ships `verter-lsp.exe`; a POSIX-only whitelist drops the engine on win32.",
    ).toBe(true);
  });

  it("still prunes unknown bin/ entries", () => {
    // The whitelist must stay a whitelist — a wildcard would let arbitrary
    // artifacts into the published VSIX.
    expect(stageBin).toMatch(/if\s*\(!allowedBinEntries\.includes\(entry\)\)/);
  });
});

const bracketDepth = (text: string) =>
  (text.match(/\[/g)?.length ?? 0) - (text.match(/\]/g)?.length ?? 0);

const splitNeedsValue = (value: string): string[] =>
  value
    .replace(/[[\]]/g, "\n")
    .split(/[,\n]/)
    .map((token) =>
      token
        .trim()
        .replace(/^-\s+/, "")
        .replace(/^["']|["']$/g, "")
        .trim(),
    )
    .filter(Boolean);

/**
 * `jobs.<id>.needs` in a GitHub workflow is written three ways, and this file
 * uses all three: a bare scalar (`needs: validate`), a single-line flow
 * sequence (`needs: [validate, test]`), and a block form whose value starts on
 * the NEXT line and wraps across several. A same-line-only regex reads the
 * block form as "declares no needs at all" — which is not a noisy guard, it is
 * a disabled one: a guard that mis-fires on the real workflow can no longer
 * distinguish a broken release gate from its own parse failure.
 *
 * So parse the graph structurally instead, and answer reachability over it.
 */
const parseNeedsGraph = (yaml: string): Map<string, string[]> => {
  const lines = yaml
    // Trailing `# ...` comments are not part of the value.
    .split("\n")
    .map((line) => line.replace(/\s+#.*$/, ""))
    // Blank and comment-only lines carry no YAML structure at ANY indentation:
    // neither one opens or closes a block, so a block sequence continues right
    // across them. Dropping them up front is what makes that true here — the
    // continuation scan below reads a blank line as "nothing indented deeper
    // follows", which would silently truncate `needs:` at its first comment.
    .filter((line) => line.trim() !== "" && !line.trim().startsWith("#"));
  const graph = new Map<string, string[]>();
  let inJobs = false;
  let job: string | null = null;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^jobs:\s*$/.test(line)) {
      inJobs = true;
      continue;
    }
    if (!inJobs) continue;
    // Any other column-0 key closes the `jobs:` mapping.
    if (!/^\s/.test(line)) {
      inJobs = false;
      continue;
    }

    const header = line.match(/^ {2}([\w.-]+):\s*$/);
    if (header) {
      job = header[1];
      graph.set(job, []);
      continue;
    }
    if (job === null) continue;

    const needs = line.match(/^ {4}needs:(.*)$/);
    if (!needs) continue;

    const head = needs[1].trim();
    const parts = head ? [head] : [];
    let depth = bracketDepth(head);

    // Consume the continuation lines: everything indented deeper than the
    // `needs:` key when the value is block form, and everything up to the
    // closing bracket when a flow sequence wraps. Every line here is
    // significant — blanks and comments were filtered out above.
    for (let j = i + 1; j < lines.length; j++) {
      const next = lines[j];
      const deeper = /^ {5,}\S/.test(next);
      if (depth <= 0 && !(head === "" && deeper)) break;
      parts.push(next.trim());
      depth += bracketDepth(next);
      i = j;
    }

    graph.set(job, splitNeedsValue(parts.join("\n")));
  }

  return graph;
};

/**
 * Breadth-first walk of the needs graph. Returns the shortest chain from
 * `from` to `target`, or `null` when `target` is unreachable.
 *
 * Transitive reachability is the property that matters: GitHub skips a job
 * whose `needs` did not all succeed, and that skip propagates down the whole
 * chain. `publish-vscode` needs `build-vsix`, which needs `test`, so a red
 * suite skips `build-vsix` and therefore skips `publish-vscode` too. Demanding
 * DIRECT membership would reject that sound chain.
 */
const chainTo = (graph: Map<string, string[]>, from: string, target: string): string[] | null => {
  const seen = new Set<string>([from]);
  const queue: string[][] = [[from]];
  while (queue.length > 0) {
    const chain = queue.shift()!;
    for (const next of graph.get(chain[chain.length - 1]) ?? []) {
      if (next === target) return [...chain, next];
      if (seen.has(next)) continue;
      seen.add(next);
      queue.push([...chain, next]);
    }
  }
  return null;
};

/**
 * A tag push publishes to crates.io, npm and the VS Code Marketplace. Every
 * publish job must therefore transitively depend on a job that actually runs
 * the test suite — otherwise a red tree publishes silently.
 */
describe("release gating", () => {
  const release = read(".github/workflows/release.yml");
  const graph = parseNeedsGraph(release);

  const publishJobs = ["publish-crates", "publish-npm", "publish-vscode"];

  it.each(publishJobs)("%s transitively depends on a job that runs tests", (job) => {
    expect(graph.has(job), `release.yml must define a \`${job}:\` job`).toBe(true);

    const needs = graph.get(job) ?? [];
    expect(needs.length, `${job} must declare needs:`).toBeGreaterThan(0);

    const chain = chainTo(graph, job, "test");
    expect(
      chain,
      `${job} publishes to a public registry. Its direct needs are [${needs.join(", ")}], ` +
        "and no chain from there reaches the `test` job. A tag push therefore publishes " +
        "without tests, clippy or fmt ever running.",
    ).not.toBeNull();
  });

  it("parses scalar, inline-sequence and block-form needs: alike", () => {
    // The bug this guards: a same-line-only `needs:` regex silently reports
    // ZERO needs for the block form, so the gate above passes vacuously — or,
    // as it actually did, fails for a reason that has nothing to do with the
    // release workflow being ungated.
    const parsed = parseNeedsGraph(
      [
        "on:",
        "  push:",
        "jobs:",
        "  scalar:",
        "    needs: alpha",
        "  inline:",
        "    needs: [alpha, beta]",
        "  block-sequence:",
        "    needs:",
        "      - alpha",
        "      - beta",
        "    if: always()",
        "  block-flow:",
        "    needs:",
        "      [",
        "        alpha,",
        "        beta,",
        "      ]",
        "    runs-on: ubuntu-latest",
        "  none:",
        "    runs-on: ubuntu-latest",
        "",
        "permissions:",
        "  contents: read",
      ].join("\n"),
    );

    expect(parsed.get("scalar")).toEqual(["alpha"]);
    expect(parsed.get("inline")).toEqual(["alpha", "beta"]);
    expect(parsed.get("block-sequence")).toEqual(["alpha", "beta"]);
    expect(parsed.get("block-flow")).toEqual(["alpha", "beta"]);
    expect(parsed.get("none")).toEqual([]);
    // `permissions:` is a sibling of `jobs:`, not a job.
    expect(parsed.has("contents")).toBe(false);
  });

  it("keeps a block sequence going across a comment line", () => {
    // The narrower instance of the very bug this parser replaced. The block
    // form parses — until a `# ...` line appears BETWEEN its entries, at which
    // point everything after the comment vanished from the graph. YAML gives a
    // comment line no structural meaning at any indentation: a block sequence
    // ends at the first SIGNIFICANT line indented no deeper than its key.
    const parsed = parseNeedsGraph(
      [
        "jobs:",
        "  block-sequence:",
        "    needs:",
        "      - alpha",
        "      # beta is the slow one, keep it last",
        "      - beta",
        "    if: always()",
        "  block-flow:",
        "    needs:",
        "      [",
        "        alpha,",
        "        # and beta",
        "        beta,",
        "      ]",
        "    runs-on: ubuntu-latest",
      ].join("\n"),
    );

    expect(parsed.get("block-sequence")).toEqual(["alpha", "beta"]);
    expect(parsed.get("block-flow")).toEqual(["alpha", "beta"]);
  });

  it("keeps a block sequence going across a blank line", () => {
    const parsed = parseNeedsGraph(
      [
        "jobs:",
        "  spaced:",
        "    needs:",
        "      - alpha",
        "",
        "      - beta",
        "    runs-on: ubuntu-latest",
        "  next-job:",
        "    needs: [gamma]",
      ].join("\n"),
    );

    expect(parsed.get("spaced")).toEqual(["alpha", "beta"]);
    // A blank line is skipped, not swallowed: the sequence still ENDS at the
    // next line indented no deeper than `needs:`, so `next-job` is a job and
    // `runs-on` is not one of `spaced`'s dependencies.
    expect(parsed.get("next-job")).toEqual(["gamma"]);
  });

  it("only reports a chain that actually exists", () => {
    const parsed = parseNeedsGraph(
      ["jobs:", "  a:", "    needs: [b]", "  b:", "    needs: [c]", "  c:", "  d:"].join("\n"),
    );

    expect(chainTo(parsed, "a", "c")).toEqual(["a", "b", "c"]);
    expect(chainTo(parsed, "a", "d")).toBeNull();
    expect(chainTo(parsed, "c", "a")).toBeNull();
  });

  it("defines a test job that runs the canonical Rust gate and the JS suite", () => {
    expect(release, "release.yml must define a `test:` job").toMatch(/^ {2}test:$/m);
    const body = release.match(/^ {2}test:\n([\s\S]*?)(?=^ {2}\S|\Z)/m)?.[1] ?? "";
    expect(body, "the test job must run the canonical Rust gate").toContain("scripts/gate.mjs");
    expect(body, "the test job must run clippy with -D warnings").toMatch(
      /clippy[\s\S]*-D warnings/,
    );
    expect(body, "the test job must check formatting").toContain("cargo fmt");
    expect(body, "the test job must run the JS suite").toMatch(/pnpm (run )?test/);
  });
});
