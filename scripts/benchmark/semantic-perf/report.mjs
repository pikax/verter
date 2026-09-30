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
  return s.n > 1 && s.max - s.min > 1048576 ? `${f(s.median)} [${f(s.min)}–${f(s.max)}]` : f(s.median);
};
const fmtRatio = (v) => {
  if (!v || v.ratio == null) return "—";
  const word = v.verdict === "verter" ? "Verter" : v.verdict === "tsc" ? "tsc" : "overlap";
  return `${word} (×${v.ratio.toFixed(2)})`;
};
const esc = (s) => String(s ?? "").replace(/\|/g, "\\|").replace(/\n/g, " ");

function referenceText(ref) {
  if (!ref) return "no reference";
  if (ref.gap) return ref.gap;
  if (ref.byConstruction) {
    return `\`${esc(ref.digest?.preview)}\` (measurement: ${ref.measuredKilled}; the API's answer is the constructed one)`;
  }
  const answer = ref.errorAny ? "error any" : `\`${esc(ref.digest?.preview)}\``;
  const codes = ref.codes.filter((c) => c !== 2322);
  return codes.length ? `${answer} + TS${codes.join("/TS")}` : answer;
}

export function renderMarkdown(run) {
  const { meta, summary, validation } = run;
  const o = meta.options;
  const lines = [];
  const push = (...l) => lines.push(...l);
  push("# Semantic benchmark: Verter vs tsc 7.0.2 on equivalent demands", "");
  push(`Validation: **${validation?.ok ? "PASSED" : "FAILED"}**${validation?.ok ? "" : ` (${validation.failures.length} failure(s); see the end)`}`, "");
  push("## Run", "");
  push(
    `- Source: \`${meta.tree.head.slice(0, 12)}\` on \`${meta.tree.branch}\`${meta.tree.dirty ? ` — **dirty** (${meta.tree.changedPaths} paths, diff sha256 \`${meta.tree.diffSha256.slice(0, 12)}\`)` : " (clean)"}`,
    `- Host: ${meta.host.platform}-${meta.host.arch}, ${meta.host.cpuModel} (${meta.host.logicalCpus} logical CPUs), ${(meta.host.totalMemoryBytes / 2 ** 30).toFixed(1)} GiB, node ${meta.host.node}`,
    `- tsc: ${meta.typescript.versionText}, ${meta.typescript.platformPackage} (exe sha256 \`${meta.typescript.exeSha256.slice(0, 12)}\`)`,
    `- Verter probe: release, sha256 \`${meta.binaries.probe.sha256.slice(0, 12)}\` (counted twin \`${meta.binaries.counted.sha256.slice(0, 12)}\`)`,
    `- Supervisor: \`${meta.binaries.supervisor.sha256.slice(0, 12)}\` (${meta.binaries.supervisor.origin}); cap ${o.memMb} MiB, deadline ${o.timeoutMs} ms; containment ${[
      ...new Set(
        run.invocations.map(
          (i) =>
            `**${i.supervisor?.containment ?? "unknown"}** (${i.supervisor?.backend ?? "?"}${i.supervisor?.containment === "sampled" ? ", no guaranteed overshoot bound" : ""})`,
        ),
      ),
    ].join(", ")}`,
    `- Schedule: ${o.warmup} warmup + ${o.repeat} measured invocations per arm, counterbalanced (scenario order and arm order reverse on alternate rounds); ${o.warmRepeats} in-process warm repeats; settings ${o.settings}; Verter library channel ${o.libMode}`,
    `- Arms: ${o.arms.map((a) => `\`${a}\``).join(", ")}`,
    "",
  );
  push("## How to read this", "");
  push(
    "- Both arms answer the same demand: the declared type of the alias `__Probe` in the same module, library and compiler options. Times are milliseconds, the median of the measured invocations with [min–max].",
    "- **cold**: the probe's first request in a fresh process (after the `__BenchInit` request absorbed one-time initialisation). tsc's figure is its server-side processing time (excludes IPC). **first answer** = setup + init + cold: the robust cross-tool figure, since each tool defers different work to its first request. **warm**: the same request repeated in the same process.",
    "- **peak / retained**: the engine process's own OS accounting, read by one reader for both tools while the process is alive after the requests (Windows: private commit; macOS: physical footprint). tsc's figure is the native API server, not the node client that drives it.",
    "- **observe** (materialising and printing the answer) is outside every timer for both tools and shown in the answers table, so work either tool defers to printing stays visible.",
    "- A verdict names a winner only when every measured repetition of one arm beats every repetition of the other (and by more than 1 ms for times); otherwise **overlap**. ×N is tsc's median over Verter's (above 1 favours Verter).",
    "- Only rows where Verter's answer **matched** tsc's measured answer enter the comparison. A wrong, partial, refused or killed answer is never a win; a beyond-tsc answer is reported separately and never counted as a speed win.",
    "",
  );

  const cells = summary.cells;
  const matched = cells.filter((c) => c.headline);
  push("## Head-to-head (matched answers only)", "");
  if (!matched.length) push("_No row has a matched answer from both headline arms._", "");
  else {
    push(
      "| scenario | setting | answer | Verter cold | tsc cold | cold | Verter first answer | tsc first answer | first answer | Verter warm | tsc warm | Verter peak MB | tsc peak MB | peak | Verter retained MB | tsc retained MB |",
      "|---|---|---|---:|---:|---|---:|---:|---|---:|---:|---:|---:|---|---:|---:|",
    );
    for (const c of matched) {
      const v = c.arms.verter.metrics;
      const t = c.arms["tsc-api"].metrics;
      push(
        `| ${c.scenario} | ${c.setting} | \`${esc(c.reference?.digest?.preview?.slice(0, 40))}\` | ${fmtMs(v.coldMs)} | ${fmtMs(t.coldMs)} | ${fmtRatio(c.headline.coldMs)} | ${fmtMs(v.firstAnswerMs)} | ${fmtMs(t.firstAnswerMs)} | ${fmtRatio(c.headline.firstAnswerMs)} | ${fmtMs(v.warmMs)} | ${fmtMs(t.warmMs)} | ${fmtMb(v.peakBytes)} | ${fmtMb(t.peakBytes)} | ${fmtRatio(c.headline.peakBytes)} | ${fmtMb(v.retainedBytes)} | ${fmtMb(t.retainedBytes)} |`,
      );
    }
    push("");
    const tally = { verter: 0, tsc: 0, overlap: 0 };
    for (const c of matched) tally[c.headline.firstAnswerMs.verdict] = (tally[c.headline.firstAnswerMs.verdict] ?? 0) + 1;
    push(`First-answer verdicts over ${matched.length} matched row(s): Verter ${tally.verter}, tsc ${tally.tsc}, overlap ${tally.overlap}.`, "");
  }

  push("## Answers (every row)", "");
  push(
    "| scenario | setting | tsc 7.0.2 (measured) | Verter | detail | Verter cold | tsc cold | tsc round trip | Verter observe | tsc observe |",
    "|---|---|---|---|---|---:|---:|---:|---:|---:|",
  );
  for (const c of cells) {
    const v = c.arms.verter;
    const t = c.arms["tsc-api"];
    push(
      `| ${c.scenario} | ${c.setting} | ${referenceText(c.reference)} | **${v?.class ?? "not run"}** | ${esc(v?.detail).slice(0, 120)} | ${fmtMs(v?.metrics?.coldMs)} | ${fmtMs(t?.metrics?.coldMs)} | ${fmtMs(t?.metrics?.coldRoundTripMs)} | ${fmtMs(v?.metrics?.observeMs)} | ${fmtMs(t?.metrics?.observeMs)} |`,
    );
  }
  push("", `Verter classes: ${Object.entries(summary.verterClassCounts).map(([k, n]) => `${k} ${n}`).join(", ")}.`, "");

  const beyond = cells.filter((c) => c.arms.verter?.class === "beyond-tsc");
  if (beyond.length) {
    push("## Beyond tsc's limits (reported separately, never a speed win)", "");
    push("| scenario | setting | tsc stops with | Verter first answer | tsc time to its fallback (first answer) | Verter peak MB | tsc peak MB |", "|---|---|---|---:|---:|---:|---:|");
    for (const c of beyond) {
      const v = c.arms.verter.metrics;
      const t = c.arms["tsc-api"]?.metrics;
      push(`| ${c.scenario} | ${c.setting} | ${referenceText(c.reference)} | ${fmtMs(v.firstAnswerMs)} | ${fmtMs(t?.firstAnswerMs)} | ${fmtMb(v.peakBytes)} | ${fmtMb(t?.peakBytes)} |`);
    }
    push("");
  }

  if (o.arms.includes("verter-obs")) {
    push("## Observability cost (Verter with audit, timing, footprint and metrics capture on; not compared with tsc)", "");
    push("| scenario | setting | production cold | observability-on cold | cost | production peak MB | observability-on peak MB |", "|---|---|---:|---:|---|---:|---:|");
    for (const c of cells) {
      if (!c.arms["verter-obs"] || !c.arms.verter) continue;
      const cost = c.obsCost?.coldMs;
      push(
        `| ${c.scenario} | ${c.setting} | ${fmtMs(c.arms.verter.metrics.coldMs)} | ${fmtMs(c.arms["verter-obs"].metrics.coldMs)} | ${cost?.ratio != null ? `×${cost.ratio.toFixed(2)} (${cost.verdict === "verter" ? "slower" : cost.verdict === "tsc" ? "faster" : "overlap"})` : "—"} | ${fmtMb(c.arms.verter.metrics.peakBytes)} | ${fmtMb(c.arms["verter-obs"].metrics.peakBytes)} |`,
      );
    }
    push("");
  }

  if (o.arms.includes("verter-counted")) {
    push("## Verter work and allocation counts (instrumented run; its times are not compared)", "");
    push("| scenario | setting | cold-request allocations | allocated MB | relation proofs | semantic nodes | memo entries | retention peak MB |", "|---|---|---:|---:|---:|---:|---:|---:|");
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
      "Verter exposes no whole-program diagnostic pass, so there is no Verter arm here: these rows show what tsc's full check of the same program costs, in both thread modes, next to the demanded-probe numbers above. `Memory used` is tsc's own counter, reported apart from the OS peak.",
      "",
    );
    push("| scenario | setting | diagnostics | parallel wall | parallel check | parallel OS peak MB | parallel Memory used MB | single wall | single check | single OS peak MB | single Memory used MB |", "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (const c of cli) {
      const p = c.arms["tsc-cli"];
      const s = c.arms["tsc-cli-1"];
      push(
        `| ${c.scenario} | ${c.setting} | ${(p?.codes ?? s?.codes ?? []).map((x) => `TS${x}`).join(", ") || "none"} | ${fmtMs(p?.wallMs)} | ${fmtMs(p?.tscCheckMs)} | ${fmtMb(p?.peakBytes)} | ${fmtMb(p?.tscMemoryUsedBytes)} | ${fmtMs(s?.wallMs)} | ${fmtMs(s?.tscCheckMs)} | ${fmtMb(s?.peakBytes)} | ${fmtMb(s?.tscMemoryUsedBytes)} |`,
      );
    }
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
