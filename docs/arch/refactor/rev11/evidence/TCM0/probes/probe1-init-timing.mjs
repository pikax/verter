// TCM0 probe 1 — session initialisation and snapshot timing.
// Charter item 2: "Probe session initialisation".
//
// MEASUREMENT ONLY. This probe asserts one thing (that the cold path completes at all, i.e. no hang);
// every number it prints is an observation, never a pass/fail. It runs N independent iterations, each in
// a fresh API + fresh fixture, and reports min/median/max, because a single sample on a shared developer
// machine is not a characterisation — the spread between the fastest and slowest iteration here routinely
// exceeds 10x. Do not derive an acceptance threshold from one run of this probe; see
// ../performance-baselines.md for why absolute wall-clock figures from this host are not a locked bar.
import {
  resolveCandidate,
  loadSyncApi,
  makeFixture,
  record,
  check,
  assert,
  section,
  finish,
} from "./harness.mjs";

const ITERATIONS = Number(process.env.TCM0_PROBE_ITERATIONS ?? 10);

const candidate = resolveCandidate();
const { API } = await loadSyncApi(candidate);

section(`probe1 init timing — typescript@${candidate.version} (gitHead ${candidate.gitHead})`);
record("iterations", ITERATIONS);

const construct = [],
  cold = [],
  warm = [];
let completed = 0;

for (let i = 0; i < ITERATIONS; i++) {
  const fx = makeFixture();
  try {
    const t0 = performance.now();
    const api = new API({ cwd: fx.root });
    construct.push(performance.now() - t0);

    const t1 = performance.now();
    const snapshot = api.updateSnapshot({ openProjects: [fx.tsconfig] });
    cold.push(performance.now() - t1);

    const t2 = performance.now();
    const snapshot2 = api.updateSnapshot({ openProjects: [fx.tsconfig] });
    warm.push(performance.now() - t2);

    if (snapshot.getProjects().length === 1) completed++;

    snapshot2.dispose();
    snapshot.dispose();
    api.close();
  } finally {
    fx.dispose();
  }
}

const stat = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const med = s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
  return { min: s[0], med, max: s[s.length - 1] };
};
const fmt = (label, xs) => {
  const { min, med, max } = stat(xs);
  record(
    label,
    `min=${min.toFixed(0)}ms median=${med.toFixed(0)}ms max=${max.toFixed(0)}ms  (spread ${(max / Math.max(min, 0.5)).toFixed(0)}x)`,
  );
  return med;
};

const medConstruct = fmt("API construction", construct);
const medCold = fmt("first updateSnapshot (cold, opens project)", cold);
const medWarm = fmt("second updateSnapshot (unchanged)", warm);

record("warm median as a fraction of cold median", `${((medWarm / medCold) * 100).toFixed(1)}%`);
record("raw construction (ms)", construct.map((x) => x.toFixed(0)).join(" "));
record("raw cold (ms)", cold.map((x) => x.toFixed(0)).join(" "));
record("raw warm (ms)", warm.map((x) => x.toFixed(0)).join(" "));

// The ONE assertion: the cold path completes on every iteration. That is the charter's actual question
// ("probe session initialisation" / no hang) and it does not depend on a wall-clock threshold.
check(`the cold session path completes on all ${ITERATIONS} iterations (no hang)`, () => {
  assert(completed === ITERATIONS, `${completed}/${ITERATIONS} iterations opened the project`);
  return `${completed}/${ITERATIONS}, each opening exactly one project`;
});

record(
  "NOTE",
  "every figure above is an observation on a shared machine, not an acceptance threshold",
);
finish();
