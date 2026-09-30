// results.md: the readable report of one run. Every number is an absolute
// measurement of one arm; nothing is baseline-subtracted.

import { ARMS } from "./analyze.mjs";
import { UNCOVERED } from "./scenarios.mjs";

const fmtMs = (s) => {
  if (!s) return "—";
  const f = (v) => (v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2));
  return s.n > 1 ? `${f(s.median)} [${f(s.min)}–${f(s.max)}]` : f(s.median);
};
const fmtMb = (s) => {
  if (!s) return "—";
  const f = (v) => (v / 1048576).toFixed(1);
  return s.n > 1 && s.max - s.min > 1048576
    ? `${f(s.median)} [${f(s.min)}–${f(s.max)}]`
    : f(s.median);
};
const fmtRatio = (v) => {
  if (!v || v.verdict === "n/a") return "—";
  const word = v.verdict === "verter" ? "Verter" : v.verdict === "tsc" ? "tsc" : "overlap";
  // A median below the timer's resolution has no meaningful ratio.
  return v.ratio != null && Number.isFinite(v.ratio) && v.ratio > 0 && v.ratioMeaningful !== false
    ? `${word} (×${v.ratio.toFixed(2)})`
    : word;
};
const esc = (s) =>
  String(s ?? "")
    .replace(/\|/g, "\\|")
    .replace(/\n/g, " ");

function referenceText(ref) {
  if (!ref) return "no reference";
  if (ref.gap) return ref.gap;
  if (ref.byConstruction)
    return `\`${esc(ref.digest?.preview)}\` (measurement: ${ref.measuredGap}; the API's answer is the constructed one)`;
  const answer = ref.errorAny ? "error any" : `\`${esc(ref.digest?.preview)}\``;
  const codes = ref.codes.filter((c) => c !== 2322);
  return codes.length ? `${answer} + TS${codes.join("/TS")}` : answer;
}

/** A power receipt on one line. */
const powerText = (p) => {
  if (!p || !Object.values(p).some(Boolean)) return "not available";
  return Object.entries(p)
    .filter(([, v]) => v)
    .map(([k, v]) => `${k}: ${String(v).replace(/\s+/g, " ").trim()}`)
    .join("; ");
};

export function renderMarkdown(run) {
  const { meta, summary, validation } = run;
  const o = meta.options;
  const lines = [];
  const push = (...l) => lines.push(...l);
  push("# Semantic benchmark: Verter vs tsc 7.0.2 on equivalent demands", "");
  push(
    `Validation: **${validation?.ok ? "PASSED" : "FAILED"}**${validation?.ok ? "" : ` (${validation.failures.length} failure(s); see the end)`}`,
    "",
  );
  if (Object.keys(meta.tuning ?? {}).length)
    push(
      `**Tuned run** (not a baseline): ${Object.entries(meta.tuning)
        .map(([k, v]) => `\`${k}=${v}\``)
        .join(", ")}`,
      "",
    );
  push("## Run", "");
  const containment = [
    ...new Set(
      run.invocations
        .filter((i) => i.supervisor)
        .map(
          (i) =>
            `**${i.supervisor.containment ?? "unknown"}** (${i.supervisor.backend ?? "?"}${i.supervisor.containment === "sampled" ? ", no guaranteed overshoot bound" : ""})`,
        ),
    ),
  ].join(", ");
  push(
    `- Source: \`${meta.tree.head.slice(0, 12)}\` on \`${meta.tree.branch}\`${meta.tree.dirty ? ` — **dirty** (${meta.tree.changedPaths} paths, diff sha256 \`${meta.tree.diffSha256.slice(0, 12)}\`)` : " (clean)"}`,
    `- Host: ${meta.host.platform}-${meta.host.arch}, ${meta.host.cpuModel} (${meta.host.logicalCpus} logical CPUs), ${(meta.host.totalMemoryBytes / 2 ** 30).toFixed(1)} GiB, node ${meta.host.node}`,
    `- tsc: ${meta.typescript.versionText}, ${meta.typescript.platformPackage} (exe sha256 \`${meta.typescript.exeSha256.slice(0, 12)}\`, passed to the API explicitly)`,
    `- Verter probe: release (${meta.binaries.probe.identity?.targetArch}), sha256 \`${meta.binaries.probe.sha256.slice(0, 12)}\` (counted twin \`${meta.binaries.counted.sha256.slice(0, 12)}\`); ${(meta.build.rustc ?? "").split("\n")[0]}`,
    `- Engine memory budget ${o.memMb} MiB for both tools; each process tree is contained at ${o.memMb + o.infraMb} MiB (the budget plus ${o.infraMb} MiB for the tree's other members); deadline ${o.timeoutMs} ms of the engine's own work (the supervisor allows ${o.startupAllowanceMs ?? 0} ms more for process start)`,
    `- Supervisor: \`${meta.binaries.supervisor.sha256.slice(0, 12)}\` (${meta.binaries.supervisor.origin}); containment ${containment}`,
    `- Invocations: ${run.invocations.length} (${run.invocations.filter((i) => i.skipped).length} skipped after a warmup killed at the memory cap)`,
    `- Schedule: ${o.warmup} warmup + ${o.repeat} measured fresh-process invocations per arm, counterbalanced (scenario order reverses on alternate rounds; each cell's arm order alternates round by round, so every pair of arms runs in each order equally often in every cell, to within one for an odd count); ${o.warmRepeats} warm repeats in each arm's one live process (its first measured invocation); settings ${o.settings}; Verter library channel ${o.libMode}`,
    `- Tier: **${o.tier ?? "(none)"}**${o.only?.length ? ` (scenarios chosen with --only ${o.only.join(",")})` : ""}`,
    `- Arms: ${o.arms.map((a) => `\`${a}\``).join(", ")}`,
    `- Power (recorded, not enforced): at start ${powerText(meta.host.power?.atStart)}; at end ${powerText(meta.host.power?.atEnd)}`,
    `- Build: \`CARGO_INCREMENTAL=${meta.build.env?.CARGO_INCREMENTAL ?? "?"}\`, \`RUSTC\` bound to the toolchain's rustc (sha256 \`${String(meta.build.rustcSha256 ?? "?").slice(0, 12)}\`)`,
    `- Environment: ${meta.environment?.inherited ? "**inherited from the caller (tuned run)**" : `constructed for every child (${(meta.environment?.runtime?.names ?? []).join(", ")})`}${(meta.environment?.ignoredTuning ?? []).length ? `; the caller's ${meta.environment.ignoredTuning.map((n) => `\`${n}\``).join(", ")} did not reach any child` : ""}`,
    "",
  );
  push("## How to read this", "");
  push(
    "- Both arms answer the same demand: the declared type of the alias `__Probe` in the same module, library and compiler options. Times are milliseconds, the median of the measured invocations with [min–max].",
    "- Engine start is reported apart and in no headline: Verter's **host construction**, tsc's **spawn** (process start, including the server's session construction; client-side) and its **initialize handshake** — they are not comparable work. **setup** opens the project. **cold**: the probe's first request (after the `__BenchInit` request absorbed one-time initialisation). **first type handle** = setup + init + cold: from opening the project to holding the demanded type's handle, the robust cross-tool figure, since each tool splits its work between opening a project and its first request differently. It claims no fully printed answer: printing is **observe**, outside every timer. **warm**: the same request repeated in the same process.",
    "- tsc's request times are the server's own processing time as it reports it (excluding IPC; its clock is coarse on Windows), with the client's round trip shown beside it.",
    "- **peak / retained**: each engine process's own OS accounting (Windows: private commit; macOS: physical footprint), read by one reader for both tools with the engine alive after the requests and **before** anything is observed; tsc's figure is the native API server, never its node client. Figures after observation are reported apart. An engine whose own peak exceeds the budget counts as exhausting it.",
    `- A verdict is a descriptive rule, not a statistical test: it names a winner only when every measured repetition of one arm beats every repetition of the other by more than the timer resolution, whose basis is shown here (tsc's server clock is ${summary.resolution?.clock ?? "uncalibrated"}: ${summary.resolution?.calibrationSamples ?? 0} trivial calibration requests, ${summary.resolution?.calibrationZeroShare === null || summary.resolution?.calibrationZeroShare === undefined ? "n/a" : Math.round(100 * summary.resolution.calibrationZeroShare) + "%"} of them read 0 ms; quantum ${summary.resolution?.tscQuantumMs ?? "n/a"} ms from **${summary.resolution?.basis ?? "none"}**${summary.resolution?.basis === "workload heuristic" ? " — the calibration cannot bound a coarse clock, so this quantum is the smallest positive server time the workload produced, not a measured clock property" : ""}; two quanta per reading difference): ${summary.resolution?.single} ms for one request, ${summary.resolution?.sum} ms for first type handle, a sum of three; otherwise **overlap**. ×N is tsc's median over Verter's (above 1 favours Verter).`,
    "- Only rows where Verter's answer **matched** tsc's measured answer enter the comparison. A wrong, partial, refused, unverified or killed answer is never a win; a beyond-tsc answer is reported separately and never counted as a speed win.",
    "",
  );

  const cells = summary.cells;
  const matched = cells.filter((c) => c.headline);
  push("## Head-to-head (matched answers only)", "");
  if (!matched.length) push("_No row has a matched answer from both headline arms._", "");
  else {
    push(
      "| scenario | setting | answer | Verter cold | tsc cold | cold | Verter first type handle | tsc first type handle | first type handle | Verter warm | tsc warm | Verter peak MB | tsc peak MB | peak | Verter retained MB | tsc retained MB |",
      "|---|---|---|---:|---:|---|---:|---:|---|---:|---:|---:|---:|---|---:|---:|",
    );
    for (const c of matched) {
      const v = c.arms.verter.metrics;
      const t = c.arms["tsc-api"].metrics;
      push(
        `| ${c.scenario} | ${c.setting} | \`${esc(c.reference?.digest?.preview?.slice(0, 40))}\` | ${fmtMs(v.coldMs)} | ${fmtMs(t.coldMs)} | ${fmtRatio(c.headline.coldMs)} | ${fmtMs(v.firstTypeMs)} | ${fmtMs(t.firstTypeMs)} | ${fmtRatio(c.headline.firstTypeMs)} | ${fmtMs(v.warmMs)} | ${fmtMs(t.warmMs)} | ${fmtMb(v.peakBytes)} | ${fmtMb(t.peakBytes)} | ${fmtRatio(c.headline.peakBytes)} | ${fmtMb(v.retainedBytes)} | ${fmtMb(t.retainedBytes)} |`,
      );
    }
    push("");
    const tally = (metric) => {
      const t = { verter: 0, tsc: 0, overlap: 0 };
      for (const c of matched)
        t[c.headline[metric].verdict] = (t[c.headline[metric].verdict] ?? 0) + 1;
      return `Verter ${t.verter}, tsc ${t.tsc}, overlap ${t.overlap}`;
    };
    push(
      `Over ${matched.length} matched row(s) — first type handle: ${tally("firstTypeMs")}; cold: ${tally("coldMs")}; peak memory: ${tally("peakBytes")}.`,
      "",
    );
  }

  push(
    "## Phases (every row both headline arms completed; engine start is not comparable work)",
    "",
  );
  push(
    "| scenario | setting | Verter host construction | tsc spawn (process + session, client-side) | tsc initialize handshake | Verter setup | tsc setup | Verter init | tsc init | Verter peak after observe MB | tsc peak after observe MB |",
    "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
  );
  for (const c of cells) {
    const v = c.arms.verter?.metrics;
    const t = c.arms["tsc-api"]?.metrics;
    if (!v?.setupMs || !t?.setupMs) continue;
    push(
      `| ${c.scenario} | ${c.setting} | ${fmtMs(v.engineStartMs)} | ${fmtMs(t.spawnMs)} | ${fmtMs(t.engineStartMs)} | ${fmtMs(v.setupMs)} | ${fmtMs(t.setupMs)} | ${fmtMs(v.initMs)} | ${fmtMs(t.initMs)} | ${fmtMb(v.observePeakBytes)} | ${fmtMb(t.observePeakBytes)} |`,
    );
  }
  push("");

  push("## Answers (every row)", "");
  push(
    "| scenario | setting | tsc 7.0.2 (measured) | tsc arm | Verter | detail | Verter cold | tsc cold | tsc round trip | Verter observe | tsc observe |",
    "|---|---|---|---|---|---|---:|---:|---:|---:|---:|",
  );
  for (const c of cells) {
    const v = c.arms.verter;
    const t = c.arms["tsc-api"];
    push(
      `| ${c.scenario} | ${c.setting} | ${referenceText(c.reference)} | ${t?.class ?? "not run"}${t?.detail ? ` (${esc(t.detail).slice(0, 60)})` : ""} | **${v?.class ?? "not run"}** | ${esc(v?.detail).slice(0, 120)} | ${fmtMs(v?.metrics?.coldMs)} | ${fmtMs(t?.metrics?.coldMs)} | ${fmtMs(t?.metrics?.coldRoundTripMs)} | ${fmtMs(v?.metrics?.observeMs)} | ${fmtMs(t?.metrics?.observeMs)} |`,
    );
  }
  push(
    "",
    `Verter classes: ${Object.entries(summary.verterClassCounts)
      .map(([k, n]) => `${k} ${n}`)
      .join(", ")}.`,
    "",
  );

  const beyond = cells.filter((c) => c.arms.verter?.class === "beyond-tsc");
  if (beyond.length) {
    push("## Beyond tsc's limits (reported separately, never a speed win)", "");
    push(
      "| scenario | setting | tsc stops with | Verter first type handle | tsc time to its fallback (first type handle) | Verter peak MB | tsc peak MB |",
      "|---|---|---|---:|---:|---:|---:|",
    );
    for (const c of beyond) {
      const v = c.arms.verter.metrics;
      const t = c.arms["tsc-api"]?.metrics;
      push(
        `| ${c.scenario} | ${c.setting} | ${referenceText(c.reference)} | ${fmtMs(v.firstTypeMs)} | ${fmtMs(t?.firstTypeMs)} | ${fmtMb(v.peakBytes)} | ${fmtMb(t?.peakBytes)} |`,
      );
    }
    push("");
  }

  if (o.arms.includes("verter-obs")) {
    push(
      "## Observability cost (Verter with audit, timing, footprint and metrics capture on; not compared with tsc)",
      "",
    );
    push(
      "| scenario | setting | production cold | observability-on cold | cost | production peak MB | observability-on peak MB |",
      "|---|---|---:|---:|---|---:|---:|",
    );
    for (const c of cells) {
      if (!c.arms["verter-obs"] || !c.arms.verter) continue;
      const cost = c.obsCost?.coldMs;
      const word =
        cost?.verdict === "verter" ? "slower" : cost?.verdict === "tsc" ? "faster" : "overlap";
      push(
        `| ${c.scenario} | ${c.setting} | ${fmtMs(c.arms.verter.metrics.coldMs)} | ${fmtMs(c.arms["verter-obs"].metrics.coldMs)} | ${cost?.ratio != null && cost.ratioMeaningful !== false ? `×${cost.ratio.toFixed(2)} (${word})` : word} | ${fmtMb(c.arms.verter.metrics.peakBytes)} | ${fmtMb(c.arms["verter-obs"].metrics.peakBytes)} |`,
      );
    }
    push("");
  }

  if (o.arms.includes("verter-counted")) {
    push("## Verter work and allocation counts (instrumented run; its times are not compared)", "");
    push(
      "| scenario | setting | cold-request allocations | allocated MB | relation proofs | semantic nodes | memo entries | retention peak MB |",
      "|---|---|---:|---:|---:|---:|---:|---:|",
    );
    for (const c of cells) {
      const k = c.arms["verter-counted"];
      const r = c.arms.verter?.retention;
      if (!k) continue;
      push(
        `| ${c.scenario} | ${c.setting} | ${k.coldAllocations?.median ?? "—"} | ${k.coldAllocatedBytes ? (k.coldAllocatedBytes.median / 1048576).toFixed(1) : "—"} | ${r?.relationProofs ?? "—"} | ${r?.semanticNodes ?? "—"} | ${r?.semanticMemoEntries ?? "—"} | ${r ? (r.peakTotalBytes / 1048576).toFixed(1) : "—"} |`,
      );
    }
    push("");
  }

  const cli = cells.filter((c) => c.arms["tsc-cli"] || c.arms["tsc-cli-1"]);
  if (cli.length) {
    push("## Whole program: tsc -p (reference only)", "");
    push(
      "Verter exposes no whole-program diagnostic pass, so there is no Verter arm here: these rows show what tsc's full check costs, in both thread modes, for the scenario plus `declare const __bench_use: __Probe;` (a use, so the full check computes the probe's answer; tsc resolves an unused alias lazily). Wall times include process start; a killed run shows its time to termination instead. `Memory used` is tsc's own counter, reported apart from the OS peak.",
      "",
    );
    push(
      "| scenario | setting | parallel: status | diagnostics | wall | check | OS peak MB | Memory used MB | single: status | diagnostics | wall | check | OS peak MB | Memory used MB |",
      "|---|---|---|---|---:|---:|---:|---:|---|---|---:|---:|---:|---:|",
    );
    const armCells = (s) =>
      s
        ? `${s.status} | ${s.codes ? s.codes.map((x) => `TS${x}`).join(", ") || "none" : "—"} | ${s.wallMs ? fmtMs(s.wallMs) : s.terminationMs ? `killed at ${fmtMs(s.terminationMs)}` : "—"} | ${fmtMs(s.tscCheckMs)} | ${fmtMb(s.peakBytes)} | ${fmtMb(s.tscMemoryUsedBytes)}`
        : "not run | — | — | — | — | —";
    for (const c of cli)
      push(
        `| ${c.scenario} | ${c.setting} | ${armCells(c.arms["tsc-cli"])} | ${armCells(c.arms["tsc-cli-1"])} |`,
      );
    push("");
  }

  push("## Not covered by this harness", "");
  for (const u of UNCOVERED) push(`- **${u.family}**: ${u.reason}.`);
  push("");
  push("## Arms", "");
  for (const a of o.arms) push(`- \`${a}\`: ${ARMS[a].label}`);
  push("");
  if (!validation?.ok) {
    push("## Validation failures", "");
    for (const f of validation.failures) push(`- ${esc(f)}`);
    push("");
  }
  if (validation?.warnings?.length) {
    push("## Validation warnings", "");
    for (const w of validation.warnings) push(`- ${esc(w)}`);
    push("");
  }
  return lines.join("\n") + "\n";
}
