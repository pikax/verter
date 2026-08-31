import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { deflateRawSync } from "node:zlib";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// eslint-disable-next-line -- packaging helpers are JavaScript executed by package.mjs
// @ts-expect-error -- stage-bin.mjs intentionally has no generated declaration file.
import { assertVsixContainsMcpEngine, listVsixEntries } from "../stage-bin.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(here, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");

const read = (rel: string) => readFileSync(path.join(repoRoot, rel), "utf8");

const workflowJobs = (yaml: string): Map<string, string> =>
  new Map(
    [...yaml.matchAll(/^ {2}([\w.-]+):\n([\s\S]*?)(?=^ {2}\S|(?![\s\S]))/gm)].map((match) => [
      match[1],
      match[2],
    ]),
  );

const workflowRunCommands = (job: string): string[] => {
  const lines = job.split("\n");
  const commands: string[] = [];

  for (let i = 0; i < lines.length; i++) {
    const match = lines[i].match(/^(\s*)(?:-\s+)?run:\s*(.*?)\s*$/);
    if (!match) continue;

    const indentation = match[1].length;
    const scalar = match[2];
    if (scalar !== "" && !/^[|>][+-]?$/.test(scalar)) {
      commands.push(scalar);
      continue;
    }

    const block: string[] = [];
    while (i + 1 < lines.length) {
      const next = lines[i + 1];
      if (next.trim() !== "" && next.length - next.trimStart().length <= indentation) break;
      i += 1;
      block.push(next.trim());
    }
    commands.push(block.join("\n"));
  }

  return commands;
};

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

describe("playground deploy build", () => {
  const netlify = read("netlify.toml");
  const viteConfig = read("packages/playground/vite.config.ts");

  it("uses the artifact-free Vue shell transform only on Netlify", () => {
    expect(netlify).toMatch(/VERTER_PLAYGROUND_SHELL_COMPILER\s*=\s*"vue"/);
    expect(viteConfig).toContain('process.env.VERTER_PLAYGROUND_SHELL_COMPILER ?? "verter"');
    expect(viteConfig).toContain('shellCompiler === "vue" ? vue() : verter()');
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
  const releaseCheck = read(".github/workflows/release-check.yml");
  const graph = parseNeedsGraph(release);

  const publishJobs = ["publish-crates", "publish-npm", "publish-vscode"];

  it("keeps PR release validation read-only and reserves the full rehearsal for dispatch", () => {
    expect(releaseCheck).toMatch(/permissions:\n  contents: read/);

    const jobs = workflowJobs(releaseCheck);
    const pullRequest = jobs.get("pull-request-contract") ?? "";
    expect(pullRequest).toContain("if: github.event_name == 'pull_request'");
    expect(pullRequest).toContain("pnpm --filter verter-vscode exec vitest run");
    expect(pullRequest).toContain("src/releasePackaging.spec.ts");
    expect(pullRequest).toContain("node --test scripts/githubctl/tests/release-plan.test.mjs");

    const dispatched = jobs.get("dry-run") ?? "";
    expect(dispatched).toContain("if: github.event_name == 'workflow_dispatch'");
    expect(dispatched).toContain("uses: ./.github/workflows/release.yml");
    expect(dispatched).toContain("dry_run: true");
    for (const permission of [
      "contents: write",
      "deployments: write",
      "id-token: write",
      "checks: write",
      "issues: write",
      "pull-requests: write",
    ]) {
      expect(dispatched).toContain(permission);
    }
  });

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

  // @ai-generated - Discriminates the workflow parser's final-job EOF boundary.
  it("parses the final workflow job when the file ends inside that job", () => {
    const parsed = workflowJobs(
      [
        "jobs:",
        "  first:",
        "    steps:",
        "      - run: first-command",
        "  final:",
        "    steps:",
        "      - run: final-command",
      ].join("\n"),
    );

    expect(parsed.get("first")).toContain("first-command");
    expect(parsed.get("final")).toContain("final-command");
  });

  it("defines a test job that runs the canonical Rust gate and the JS suite", () => {
    expect(release, "release.yml must define a `test:` job").toMatch(/^ {2}test:$/m);
    const body = workflowJobs(release).get("test") ?? "";
    expect(body, "the release test job must run the exhaustive canonical Rust gate").toContain(
      "node scripts/gate.mjs --exhaustive",
    );
    expect(body, "the test job must run clippy with -D warnings").toMatch(
      /clippy[\s\S]*-D warnings/,
    );
    expect(body, "the test job must check formatting").toContain("cargo fmt");
    expect(body, "the test job must run the JS suite").toMatch(/pnpm (run )?test/);
  });

  // @ai-generated - Release shares the dedicated 16 GiB runner policy with CI.
  it("gives the release Rust gate a dedicated-runner memory ceiling", () => {
    const body = workflowJobs(release).get("test") ?? "";
    expect(body, "the release gate must retain 4 GiB of runner headroom").toContain(
      "node scripts/gate.mjs --exhaustive --memory-limit 12GiB",
    );
  });

  // @ai-generated - Every hermetic gate invocation needs an explicitly provisioned oracle cache.
  it("provisions the offline oracle cache before every release workflow gate invocation", () => {
    const provision =
      "node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs";
    const gate = "node scripts/gate.mjs";
    let gateInvocations = 0;

    for (const [job, body] of workflowJobs(release)) {
      let availableProvisions = 0;
      for (const command of workflowRunCommands(body)) {
        if (command.includes(provision)) availableProvisions += 1;
        if (!command.includes(gate)) continue;

        gateInvocations += 1;
        expect(
          availableProvisions,
          `release.yml job \`${job}\` invokes \`${command}\` without a preceding unused oracle-cache provisioning step`,
        ).toBeGreaterThan(0);
        availableProvisions -= 1;
      }
    }

    expect(
      gateInvocations,
      "release.yml must invoke the canonical gate at least once",
    ).toBeGreaterThan(0);
  });
});

describe("CI Rust path eligibility", () => {
  const ci = read(".github/workflows/ci.yml");

  // @ai-generated - Pins the bounded first sccache rollout and cold-build memory fix.
  it("runs the Rust test gate through the measured GitHub sccache lane", () => {
    const body = workflowJobs(ci).get("rust-test") ?? "";
    expect(body, "rust-test must install the reviewed sccache action").toContain(
      "mozilla-actions/sccache-action@v0.0.11",
    );
    expect(body, "rust-test must pin the measured sccache binary").toContain("version: v0.17.0");
    expect(body, "rust-test must enable the GitHub Actions cache backend").toContain(
      'SCCACHE_GHA_ENABLED: "true"',
    );
    expect(body, "rust-test must namespace its shared cache generation").toContain(
      'SCCACHE_GHA_VERSION: "verter-rust-test-v1"',
    );
    expect(body, "the gate must use the repository's telemetry wrapper").toContain(
      "node scripts/run-cached.mjs -- node scripts/gate.mjs --exhaustive --memory-limit 12GiB",
    );
  });

  it("pins the required VS Code matrix to a reviewed editor release", () => {
    const body = workflowJobs(ci).get("vscode-e2e") ?? "";
    expect(body, "the required matrix must not resolve a moving stable editor").toContain(
      'E2E_VSCODE_VERSION: "1.135.0"',
    );
  });

  it("runs endurance routes independently and retains actionable editor diagnostics", () => {
    const endurance = workflowJobs(ci).get("endurance-lsp") ?? "";
    expect(endurance, "one failing route must not prevent the other endurance routes").toContain(
      "fail-fast: false",
    );
    expect(
      endurance,
      "the endurance strategy must enumerate every required provider route",
    ).toContain("route: [tsserver, tsgo, shared-tsgo]");
    expect(endurance, "the harness must receive the current matrix route").toContain(
      "VERTER_ENDURANCE_PROVIDER: ${{ matrix.route }}",
    );
    expect(endurance, "each route must retain its own receipt artifact").toContain(
      "endurance-lsp-receipts-${{ matrix.route }}",
    );

    const vscode = workflowJobs(ci).get("vscode-e2e") ?? "";
    expect(vscode, "failed E2E runs must retain the extension/LSP log").toContain(
      "/tmp/verter-e2e-*.log",
    );
    expect(vscode, "failed E2E runs must retain the exact run-summary oracle").toContain(
      "/tmp/verter-e2e-*.log.runsummary",
    );
  });

  // @ai-generated - Pins every non-Rust input consumed by the canonical Rust gate job.
  it("runs the Rust job when gate, harness, provider, or install-graph inputs change", () => {
    const rustFilter = ci.match(/^ {12}rust:\n([\s\S]*?)(?=^ {12}[\w-]+:\n)/m)?.[1] ?? "";
    const requiredInputs = [
      "packages/framework-conformance-harness/**",
      "packages/language-shared/**",
      "packages/svelte-jsx/**",
      "packages/typescript-plugin/**",
      "scripts/gate*.mjs",
      ".npmrc",
      ".nvmrc",
      "package.json",
      "pnpm-lock.yaml",
      "pnpm-workspace.yaml",
    ];

    expect(rustFilter, "detect-changes must define a rust path filter").not.toBe("");
    for (const input of requiredInputs) {
      expect(
        rustFilter,
        `the Rust gate consumes \`${input}\`; changing it must make detect-changes.rust true`,
      ).toContain(`'${input}'`);
    }
  });
});

/**
 * The packaged VSIX must also carry the standalone MCP engine.
 *
 * `verter.mcp.enabled` defaults to true and the extension SPAWNS
 * `bin/verter-mcp` — a VSIX without it re-ships the original dead-setting
 * defect while CI stays green (E2E resolves `target/debug/verter-mcp`, which
 * CI builds; a marketplace install has no `target/`). Three rails hold it:
 * the release workflow stages the per-target artifact fail-closed, the
 * staging whitelist admits it, and `package.mjs` inspects the PACKED VSIX
 * bytes and refuses to produce one without the engine.
 */
describe("VSIX MCP engine payload", () => {
  const stageBin = read("packages/vue-vscode/stage-bin.mjs");
  const release = read(".github/workflows/release.yml");
  const packageMjs = read("packages/vue-vscode/package.mjs");

  it("whitelists the MCP engine so staging does not delete it", () => {
    const match = stageBin.match(/EXTRA_ALLOWED_BIN_ENTRIES\s*=\s*\[([^\]]*)\]/);
    expect(match, "EXTRA_ALLOWED_BIN_ENTRIES must exist in stage-bin.mjs").toBeTruthy();
    expect(
      match![1].includes("verter-mcp"),
      "stage-bin.mjs prunes every bin/ entry outside its whitelist. Without `verter-mcp` " +
        "the release workflow's pre-staged MCP engine is deleted before `vsce package`.",
    ).toBe(true);
    expect(
      match![1].includes("verter-mcp.exe"),
      "Windows ships `verter-mcp.exe`; a POSIX-only whitelist drops the engine on win32.",
    ).toBe(true);
  });

  it("build-vsix depends on build-mcp so the artifacts exist to stage", () => {
    const graph = parseNeedsGraph(release);
    expect(graph.has("build-vsix"), "release.yml must define build-vsix").toBe(true);
    expect(
      graph.get("build-vsix"),
      "build-vsix downloads mcp-* artifacts; without a direct needs edge to build-mcp " +
        "they may not exist yet and the download silently matches nothing.",
    ).toContain("build-mcp");
  });

  it("stages the per-target MCP artifact fail-closed before packaging", () => {
    const body = workflowJobs(release).get("build-vsix") ?? "";
    expect(body, "build-vsix must download the mcp-* artifacts").toContain("pattern: mcp-*");
    expect(body, "build-vsix must stage the per-target verter-mcp binary").toContain(
      "/tmp/mcp-artifacts/mcp-${lsp_pkg}/${MCP_BIN}",
    );
    expect(
      body,
      "a missing MCP artifact must fail the packaging step (test -f), not fall through",
    ).toMatch(/test -f "\$MCP_SOURCE"/);
    expect(body, "win32 stages the .exe engine name").toContain('MCP_BIN="verter-mcp.exe"');
  });

  it("package.mjs inspects the packed VSIX for the engine", () => {
    expect(
      packageMjs,
      "package.mjs must call assertVsixContainsMcpEngine on the produced .vsix — the " +
        "staging inputs can all look right while vsce packs something else.",
    ).toContain("assertVsixContainsMcpEngine");
  });
});

// ---------------------------------------------------------------------------
// Functional discrimination for the VSIX inspection itself: a hand-rolled
// zip whose entries carry REAL content bytes. The inspector parses the zip
// central directory AND validates the packed engine's executable header
// against the target, so name-only satisfaction (a host-arch binary under
// the right name — the vsce-prepublish-overwrite hazard) must fail.
// ---------------------------------------------------------------------------

interface ZipEntrySpec {
  readonly name: string;
  readonly content?: Buffer;
}

/** Build a minimal valid zip of STORED entries with the given contents. */
function zipWithEntries(entries: ZipEntrySpec[]): Buffer {
  const chunks: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  for (const { name, content } of entries) {
    const nameBytes = Buffer.from(name, "utf8");
    const data = content ?? Buffer.alloc(0);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); // local file header signature
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0, 8); // method 0 = stored
    local.writeUInt32LE(data.length, 18); // compressed size
    local.writeUInt32LE(data.length, 22); // uncompressed size
    local.writeUInt16LE(nameBytes.length, 26);
    chunks.push(local, nameBytes, data);

    const cd = Buffer.alloc(46);
    cd.writeUInt32LE(0x02014b50, 0); // central directory signature
    cd.writeUInt16LE(20, 4); // version made by
    cd.writeUInt16LE(20, 6); // version needed
    cd.writeUInt16LE(0, 10); // method 0 = stored
    cd.writeUInt32LE(data.length, 20); // compressed size
    cd.writeUInt32LE(data.length, 24); // uncompressed size
    cd.writeUInt16LE(nameBytes.length, 28);
    cd.writeUInt32LE(offset, 42); // local header offset
    central.push(cd, nameBytes);
    offset += local.length + nameBytes.length + data.length;
  }
  const cdStart = offset;
  const cdBuffer = Buffer.concat(central);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(cdBuffer.length, 12);
  eocd.writeUInt32LE(cdStart, 16);
  return Buffer.concat([...chunks, cdBuffer, eocd]);
}

/** Minimal ELF header bytes: magic + e_machine (LE u16 at 18). */
function elfBytes(machine: number): Buffer {
  const bytes = Buffer.alloc(24);
  bytes.set([0x7f, 0x45, 0x4c, 0x46], 0);
  bytes.writeUInt16LE(machine, 18);
  return bytes;
}

/** Minimal thin little-endian 64-bit Mach-O bytes: MH_MAGIC_64 + cputype. */
function machoBytes(cputype: number): Buffer {
  const bytes = Buffer.alloc(12);
  bytes.set([0xcf, 0xfa, 0xed, 0xfe], 0);
  bytes.writeUInt32LE(cputype, 4);
  return bytes;
}

/** Minimal PE bytes: MZ + e_lfanew → PE\0\0 + Machine. */
function peBytes(machine: number): Buffer {
  const bytes = Buffer.alloc(0x60);
  bytes.set([0x4d, 0x5a], 0);
  bytes.writeUInt32LE(0x40, 0x3c);
  bytes.set([0x50, 0x45, 0x00, 0x00], 0x40);
  bytes.writeUInt16LE(machine, 0x44);
  return bytes;
}

const ELF_X64 = () => elfBytes(0x3e);
const ELF_ARM64 = () => elfBytes(0xb7);
const MACHO_ARM64 = () => machoBytes(0x0100000c);
const PE_X64 = () => peBytes(0x8664);

describe("VSIX inspection helpers", () => {
  let scratchDir: string;
  beforeEach(() => {
    scratchDir = mkdtempSync(path.join(tmpdir(), "verter-vsix-inspect-"));
  });
  afterEach(() => {
    rmSync(scratchDir, { recursive: true, force: true });
  });

  const writeZip = (name: string, entries: ZipEntrySpec[]): string => {
    const zipPath = path.join(scratchDir, name);
    writeFileSync(zipPath, zipWithEntries(entries));
    return zipPath;
  };

  it("lists exact central-directory entry names", () => {
    const zipPath = writeZip("sample.vsix", [
      { name: "extension/package.json" },
      { name: "extension/bin/verter-mcp", content: ELF_X64() },
    ]);
    expect(listVsixEntries(zipPath)).toEqual([
      "extension/package.json",
      "extension/bin/verter-mcp",
    ]);
  });

  it("accepts a VSIX whose engine bytes match the target", () => {
    const linux = writeZip("ok-linux.vsix", [
      { name: "extension/bin/verter-mcp", content: ELF_X64() },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: linux, vsceTarget: "linux-x64" }),
    ).not.toThrow();

    const mac = writeZip("ok-mac.vsix", [
      { name: "extension/bin/verter-mcp", content: MACHO_ARM64() },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: mac, vsceTarget: "darwin-arm64" }),
    ).not.toThrow();

    const win = writeZip("ok-win.vsix", [
      { name: "extension/bin/verter-mcp.exe", content: PE_X64() },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: win, vsceTarget: "win32-x64" }),
    ).not.toThrow();
  });

  it("REFUSES a VSIX without the engine", () => {
    const zipPath = writeZip("missing.vsix", [{ name: "extension/package.json" }]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: zipPath, vsceTarget: "linux-x64" }),
    ).toThrow(/does not contain extension\/bin\/verter-mcp/);
  });

  it("REFUSES a right-named engine with wrong-platform bytes (prepublish overwrite)", () => {
    // The exact hazard: packaging darwin-arm64 on a Linux runner where a
    // newer HOST build overwrote the staged cross-target engine — an ELF
    // binary named verter-mcp inside a mac VSIX.
    const elfInMacVsix = writeZip("elf-in-mac.vsix", [
      { name: "extension/bin/verter-mcp", content: ELF_ARM64() },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: elfInMacVsix, vsceTarget: "darwin-arm64" }),
    ).toThrow(/ELF/);
  });

  it("REFUSES a right-format engine with the wrong CPU arch", () => {
    const wrongArch = writeZip("wrong-arch.vsix", [
      { name: "extension/bin/verter-mcp", content: ELF_ARM64() },
    ]);
    // The matcher must name BOTH sides of the mismatch — only the
    // format/arch-mismatch branch produces this text. A loose /arch/ would
    // also match this fixture's own filename inside any other error message
    // (every refusal embeds vsixPath), proving nothing about the error class.
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: wrongArch, vsceTarget: "linux-x64" }),
    ).toThrow(/is ELF\/aarch64 but the linux-x64 VSIX needs ELF\/x86_64/);
  });

  it("REFUSES engine bytes that are no recognized executable at all", () => {
    const script = writeZip("script.vsix", [
      { name: "extension/bin/verter-mcp", content: Buffer.from("#!/bin/sh\necho nope\n") },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: script, vsceTarget: "linux-x64" }),
    ).toThrow(/not a recognized|recognized executable/i);
  });

  it("requires the EXACT per-target name — an .exe does not satisfy POSIX, nor vice versa", () => {
    const exeOnly = writeZip("exe-only.vsix", [
      { name: "extension/bin/verter-mcp.exe", content: PE_X64() },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: exeOnly, vsceTarget: "linux-x64" }),
    ).toThrow(/does not contain/);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: exeOnly, vsceTarget: "win32-x64" }),
    ).not.toThrow();

    const posixOnly = writeZip("posix-only.vsix", [
      { name: "extension/bin/verter-mcp", content: ELF_X64() },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: posixOnly, vsceTarget: "win32-x64" }),
    ).toThrow(/does not contain extension\/bin\/verter-mcp\.exe/);
  });

  it("a universal build requires the HOST platform's engine name and format", () => {
    const machoHost = writeZip("host.vsix", [
      { name: "extension/bin/verter-mcp", content: MACHO_ARM64() },
    ]);
    expect(() =>
      assertVsixContainsMcpEngine({
        vsixPath: machoHost,
        vsceTarget: undefined,
        hostPlatform: "darwin",
        hostArch: "arm64",
      }),
    ).not.toThrow();
    expect(() =>
      assertVsixContainsMcpEngine({
        vsixPath: machoHost,
        vsceTarget: undefined,
        hostPlatform: "win32",
        hostArch: "x64",
      }),
    ).toThrow(/verter-mcp\.exe/);
    // Same name, wrong bytes for the host: a Mach-O engine in a linux
    // universal build fails on format, not name.
    expect(() =>
      assertVsixContainsMcpEngine({
        vsixPath: machoHost,
        vsceTarget: undefined,
        hostPlatform: "linux",
        hostArch: "x64",
      }),
    ).toThrow(/MachO|ELF/);
  });

  it("reads entry bytes back from DEFLATE-compressed entries too", () => {
    // vsce writes deflated entries; the byte validation must not depend on
    // stored-only zips. Round-trip through a real deflate.
    const deflated = deflateRawSync(ELF_X64());
    const nameBytes = Buffer.from("extension/bin/verter-mcp", "utf8");
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(8, 8); // method 8 = deflate
    local.writeUInt32LE(deflated.length, 18);
    local.writeUInt32LE(ELF_X64().length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    const cd = Buffer.alloc(46);
    cd.writeUInt32LE(0x02014b50, 0);
    cd.writeUInt16LE(20, 4);
    cd.writeUInt16LE(20, 6);
    cd.writeUInt16LE(8, 10);
    cd.writeUInt32LE(deflated.length, 20);
    cd.writeUInt32LE(ELF_X64().length, 24);
    cd.writeUInt16LE(nameBytes.length, 28);
    cd.writeUInt32LE(0, 42);
    const cdStart = 30 + nameBytes.length + deflated.length;
    const eocd = Buffer.alloc(22);
    eocd.writeUInt32LE(0x06054b50, 0);
    eocd.writeUInt16LE(1, 8);
    eocd.writeUInt16LE(1, 10);
    eocd.writeUInt32LE(46 + nameBytes.length, 12);
    eocd.writeUInt32LE(cdStart, 16);
    const zipPath = path.join(scratchDir, "deflated.vsix");
    writeFileSync(zipPath, Buffer.concat([local, nameBytes, deflated, cd, nameBytes, eocd]));
    expect(() =>
      assertVsixContainsMcpEngine({ vsixPath: zipPath, vsceTarget: "linux-x64" }),
    ).not.toThrow();
  });
});
