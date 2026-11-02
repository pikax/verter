import { createHash } from "node:crypto";
import { readFileSync, readdirSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const MODE_IDS = [
  "cold-medium",
  "first-warm-medium",
  "repeated-warm-medium",
  "incremental",
  "edit-revert",
  "range-cursor",
  "profile-config-transition",
  "policy-capability-transition",
  "provider-transition",
  "project-open",
  "project-close",
  "long-churn",
  "cold-large",
  "cancellation",
  "disabled-unsupported-unopened-masked",
];
const LANGUAGES = [
  "vue",
  "svelte",
  "html",
  "javascript",
  "typescript",
  "jsx",
  "tsx",
  "css",
  "scss",
  "less",
];
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const same = isDeepStrictEqual;
const nonempty = (v) => typeof v === "string" && v.length > 0;
const list = (v) => Array.isArray(v) && v.length > 0;
const unique = (rows) => new Set(rows).size === rows.length;

// Catalogs use a deliberately restricted TOML subset: single-line JSON-compatible
// scalars/arrays, tables and arrays of tables. Unknown syntax and duplicate keys
// fail, rather than leaving an unvalidated portion of the authority document.
export function readCatalog(text) {
  const root = Object.create(null);
  let current = root;
  for (const [index, raw] of text.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const table = /^\[\[([a-z_]+)\]\]$/.exec(line);
    const singleton = /^\[([a-z_]+)\]$/.exec(line);
    if (table) {
      const name = table[1];
      if (name in root && !Array.isArray(root[name])) throw Error(`duplicate table ${name}`);
      root[name] ??= [];
      current = Object.create(null);
      root[name].push(current);
    } else if (singleton) {
      const name = singleton[1];
      if (name in root) throw Error(`duplicate table ${name}`);
      current = root[name] = Object.create(null);
    } else {
      const field = /^([a-z_][a-z_0-9]*) = (.+)$/.exec(line);
      if (!field || ["__proto__", "constructor", "prototype"].includes(field[1]))
        throw Error(`unsupported catalog syntax at line ${index + 1}`);
      if (field[1] in current) throw Error(`duplicate key ${field[1]} at line ${index + 1}`);
      const value = JSON.parse(field[2]);
      const scalar = (v) =>
        typeof v === "string" || typeof v === "boolean" || Number.isSafeInteger(v);
      if (!(scalar(value) || (Array.isArray(value) && value.every(scalar))))
        throw Error(`unsupported catalog value at line ${index + 1}`);
      current[field[1]] = value;
    }
  }
  return root;
}

function confined(root, path) {
  if (!nonempty(path) || isAbsolute(path) || path.includes("\\") || path.split("/").includes(".."))
    throw Error(`invalid repository path ${path}`);
  const actual = realpathSync(resolve(root, path));
  if (!actual.startsWith(realpathSync(root) + sep)) throw Error(`outside repository path ${path}`);
  return actual;
}

export function loadFormatterLock(root = ROOT) {
  const read = (path) => readFileSync(confined(root, path));
  const compatibility = readdirSync(resolve(root, "catalogs"))
    .filter((name) => /^formatter-compatibility(?:-[a-z][a-z0-9-]*)?-v1\.toml$/.test(name))
    .sort()
    .map((name) => ({
      ...readCatalog(read(`catalogs/${name}`).toString()),
      catalog_path: `catalogs/${name}`,
    }));
  const performance = readCatalog(read("catalogs/formatter-performance-v1.toml").toString());
  const inventory = JSON.parse(read("tests/formatter/inventory.json"));
  const sourceInventory = JSON.parse(read(inventory.source_inventory));
  const artifacts = {};
  for (const c of [...compatibility, performance]) {
    for (const row of c.asset ?? []) {
      const bytes = read(row.path);
      artifacts[row.path] = {
        sha256: sha256(bytes),
        bytes: bytes.length,
        ...(row.path.endsWith(".json") ? { json: JSON.parse(bytes) } : {}),
        ...(row.path.endsWith(".txt") || row.path.endsWith(".vue")
          ? { text: bytes.toString("utf8") }
          : {}),
      };
    }
  }
  const routeSources = Object.fromEntries(
    inventory.routes.map((r) => [r.path, read(r.path).toString()]),
  );
  return { compatibility, performance, inventory, sourceInventory, artifacts, routeSources };
}

export function validateFormatterLock(lock) {
  const errors = [];
  const check = (ok, why) => {
    if (!ok) errors.push(why);
  };
  const { compatibility, performance: p, inventory, artifacts } = lock;
  const artifact = (path) => {
    const safe =
      nonempty(path) &&
      !isAbsolute(path) &&
      !path.includes("\\") &&
      !path.split("/").includes("..");
    check(
      safe && path.startsWith("fixtures/formatter/") && path in artifacts,
      `missing/invalid fixture path ${path}`,
    );
    return safe ? artifacts[path] : undefined;
  };
  const assetJson = (path) => artifact(path)?.json;
  const base = compatibility.find(
    (c) => c.catalog_path === "catalogs/formatter-compatibility-v1.toml",
  );
  check(!!base, "missing base compatibility catalog");
  if (!base) return errors;
  check(same(base.languages, LANGUAGES), "incomplete language population");
  const languageOwners = new Set();
  for (const c of compatibility) {
    check(
      c.schema === 1 &&
        c.prettier_release === base.prettier_release &&
        /^\d+\.\d+\.\d+$/.test(c.prettier_release),
      "compatibility schema/release mismatch",
    );
    const extension = c.catalog_path !== base.catalog_path;
    if (extension) {
      const language = /^catalogs\/formatter-compatibility-([a-z][a-z0-9-]*)-v1\.toml$/.exec(
        c.catalog_path,
      )?.[1];
      check(
        same(c.languages, [language]) &&
          !LANGUAGES.includes(language) &&
          !languageOwners.has(language),
        `invalid extension language ${language}`,
      );
      languageOwners.add(language);
      for (const corpus of c.corpus ?? [])
        check(
          corpus.language === language &&
            corpus.path.startsWith(`fixtures/formatter/corpora/${language}/`),
          "extension corpus language/path mismatch",
        );
    }
    const options = c.option ?? [];
    const support = assetJson(c.oracle_support ?? base.oracle_support);
    const extra = [
      "objectWrap",
      "experimentalTernaries",
      "experimentalOperatorPosition",
      "checkIgnorePragma",
    ];
    const expectedNames = [...(support?.options ?? []).map((o) => o.name), ...extra].sort();
    check(
      unique(options.map((o) => o.name)) && same(options.map((o) => o.name).sort(), expectedNames),
      "incomplete option vocabulary",
    );
    check(support?.release === c.prettier_release, "oracle release mismatch");
    for (const o of options) {
      check(
        nonempty(o.value_kind) &&
          nonempty(o.default_json) &&
          Array.isArray(o.applicable_languages) &&
          Array.isArray(o.unsupported_values),
        `incomplete option ${o.name}`,
      );
      try {
        JSON.parse(o.default_json);
        JSON.parse(o.accepted_values_json);
      } catch {
        check(false, `invalid option JSON ${o.name}`);
      }
    }
    for (const key of ["option_sets", "recovery_modes", "request_modes"])
      check(list(c[key]) && unique(c[key]), `missing/duplicate ${key}`);
    check(
      same(c.recovery_modes, ["valid", "verbatim-island", "malformed"]),
      "incomplete recovery modes",
    );
    check(same(c.request_modes, ["full", "range", "cursor"]), "incomplete request modes");
    const corpora = new Map();
    for (const row of c.corpus ?? []) {
      check(!corpora.has(row.id), `duplicate corpus ${row.id}`);
      corpora.set(row.id, row);
      const a = artifact(row.path);
      check(
        row.path.startsWith("fixtures/formatter/corpora/") && a?.sha256 === row.sha256,
        `corpus digest/path mismatch ${row.id}`,
      );
      check(
        c.languages.includes(row.language) && nonempty(row.purpose),
        `invalid corpus ${row.id}`,
      );
    }
    const seen = new Set();
    const ids = new Set();
    for (const row of c.cell ?? []) {
      const key = [row.language, row.option_set, row.recovery_mode, row.request_mode].join("|");
      check(!seen.has(key) && !ids.has(row.id), `duplicate cell ${row.id}`);
      seen.add(key);
      ids.add(row.id);
      check(
        c.languages.includes(row.language) &&
          c.option_sets.includes(row.option_set) &&
          c.recovery_modes.includes(row.recovery_mode) &&
          c.request_modes.includes(row.request_mode),
        `cell outside admitted matrix ${row.id}`,
      );
      const corpus = corpora.get(row.corpus_id);
      check(
        corpus?.language === row.language && corpus.id === `${row.language}-${row.recovery_mode}`,
        `cell corpus mismatch ${row.id}`,
      );
      check(
        ["prettier-exact", "verter-default", "unsupported"].includes(row.classification),
        `unknown cell classification ${row.id}`,
      );
      const options = assetJson(row.option_set);
      check(!!options && typeof options.values === "object", `missing option set ${row.id}`);
      const expected = assetJson(row.expected_outcome);
      if (!expected) continue;
      if (row.classification === "unsupported")
        check(
          expected.outcome === "unsupported" &&
            nonempty(expected.reason) &&
            expected.output === null,
          `unsupported cell claims success ${row.id}`,
        );
      else {
        check(
          expected.outcome === "complete" && expected.reason === null,
          `complete cell claims refusal ${row.id}`,
        );
        const output = artifact(expected.output);
        check(typeof output?.text === "string", `missing output ${row.id}`);
        check(
          same(expected.second_pass, { output: expected.output, edits: [] }),
          `idempotence expectation ${row.id}`,
        );
        if (row.classification === "verter-default")
          check(
            expected.divergence in (assetJson(c.divergences ?? base.divergences) ?? {}),
            `unratified divergence ${row.id}`,
          );
        else
          check(
            expected.divergence === null && row.recovery_mode === "valid",
            `exact cell divergence ${row.id}`,
          );
        if (row.recovery_mode === "verbatim-island")
          check(output?.text === artifacts[corpus?.path]?.text, `recovery bytes changed ${row.id}`);
        if (row.request_mode === "cursor")
          check(
            Number.isInteger(expected.formatted_cursor) &&
              expected.formatted_cursor >= 0 &&
              expected.formatted_cursor <= (output?.bytes ?? -1),
            `invalid formatted cursor ${row.id}`,
          );
      }
    }
    for (const lang of c.languages)
      for (const option of c.option_sets)
        for (const recovery of c.recovery_modes)
          for (const mode of c.request_modes)
            check(
              seen.has([lang, option, recovery, mode].join("|")),
              `missing cell ${lang}/${option}/${recovery}/${mode}`,
            );
  }
  for (const c of [...compatibility, p]) {
    check(list(c.asset) && unique(c.asset.map((a) => a.path)), "missing/duplicate asset inventory");
    for (const row of c.asset ?? [])
      check(artifact(row.path)?.sha256 === row.sha256, `asset digest mismatch ${row.path}`);
  }
  check(p.schema === 1, "performance schema");
  const requiredCalibration = [
    "seed",
    "iterations",
    "rotate_left_bits",
    "xor_constant",
    "add_constant",
    "pseudocode",
    "operation_schedule",
    "warmup_samples",
    "measured_samples",
    "sample_order",
    "reported_statistic",
    "reference_ns",
    "normalized_ms",
    "measurement_policy",
  ];
  check(
    requiredCalibration.every((key) => key in (p.calibration ?? {})) &&
      p.calibration?.iterations === 10000000 &&
      p.calibration?.reference_ns === 25000000 &&
      p.calibration?.seed === 305419896 &&
      p.calibration?.rotate_left_bits === 13 &&
      p.calibration?.xor_constant === 2654435769 &&
      p.calibration?.add_constant === 2135587861 &&
      p.calibration?.warmup_samples === 5 &&
      p.calibration?.measured_samples === 20 &&
      same(p.calibration?.operation_schedule, [
        "rotate-left-u64",
        "xor-u64",
        "wrapping-add-u64",
        "black-box-final-value",
      ]) &&
      same(p.calibration?.sample_order, ["baseline", "candidate", "candidate", "baseline"]) &&
      ["pseudocode", "reported_statistic", "normalized_ms", "measurement_policy"].every((key) =>
        nonempty(p.calibration?.[key]),
      ),
    "incomplete calibration",
  );
  check(
    same(
      (p.mode ?? []).map((m) => m.id),
      MODE_IDS,
    ),
    "incomplete numeric mode population",
  );
  for (const m of p.mode ?? [])
    check(
      Number.isFinite(m.latency_p95_ms) &&
        m.latency_p95_ms >= 0 &&
        nonempty(m.allocation_bytes) &&
        nonempty(m.retention) &&
        typeof m.peak_rss_applicable === "boolean" &&
        Number.isSafeInteger(m.peak_rss_delta_bytes) &&
        m.evidence_only === true,
      `incomplete numeric mode ${m.id}`,
    );
  const requiredCounters = [
    "A_in",
    "B_out",
    "B_emit",
    "B_copy",
    "T_out",
    "C_in",
    "V_in",
    "V_visit",
    "D_out",
    "D_visit",
    "E_out",
    "M_out",
    "Q_in",
    "Delta_in",
    "parser_invocations",
    "allocated_bytes",
    "retained_bytes",
  ];
  check(
    same(
      p.counter?.map((c) => c.name),
      requiredCounters,
    ) && p.counter.every((c) => nonempty(c.unit)),
    "incomplete counter units",
  );
  const requiredBounds = [
    "parser_invocations",
    "V_visit",
    "D_out",
    "D_visit",
    "B_copy",
    "B_emit",
    "edit_comparisons",
    "M_out",
    "map_query_probes",
    "all_query_probes",
    "native_stack_growth_frames",
    "renderer_work_stack_entries",
  ];
  check(
    same(
      p.bound?.map((b) => b.counter),
      requiredBounds,
    ) && p.bound.every((b) => nonempty(b.ceiling)),
    "incomplete structural bounds",
  );
  check(
    p.cache_entries_per_source === 2 &&
      p.cache_entries_global === 64 &&
      p.cache_bytes_global === 100663296 &&
      nonempty(p.result_ceiling) &&
      nonempty(p.eviction),
    "cache/result ceilings",
  );
  const fixtureIds = new Set();
  const checkedViews = new Set();
  for (const f of p.fixture ?? []) {
    check(!fixtureIds.has(f.id), `duplicate performance fixture ${f.id}`);
    fixtureIds.add(f.id);
    const data = artifact(f.path);
    check(
      f.path.startsWith("fixtures/formatter/performance/") && data?.sha256 === f.sha256,
      `performance digest/path ${f.id}`,
    );
    check(
      Number.isInteger(f.authored_utf8_bytes) &&
        f.authored_utf8_bytes > 0 &&
        Number.isInteger(f.carrier_blocks) &&
        f.carrier_blocks > 0 &&
        Number.isInteger(f.authored_view_records) &&
        f.authored_view_records > 0 &&
        list(f.mode_tags),
      `fixture units ${f.id}`,
    );
    if (f.view_inventory) {
      const v = assetJson(f.view_inventory);
      check(
        data?.bytes === f.authored_utf8_bytes &&
          v?.authored_utf8_bytes === f.authored_utf8_bytes &&
          v?.carrier_blocks?.length === f.carrier_blocks &&
          v?.records?.length === f.authored_view_records,
        `authored inventory ${f.id}`,
      );
      if (!checkedViews.has(f.view_inventory)) {
        checkedViews.add(f.view_inventory);
        let end = 0;
        for (const row of v?.records ?? []) {
          check(
            row.start === end && row.end >= row.start && row.end <= f.authored_utf8_bytes,
            `record coverage ${f.id}`,
          );
          end = row.end;
        }
        check(end === f.authored_utf8_bytes, `record EOF coverage ${f.id}`);
      }
    }
  }
  for (const id of [
    "small",
    "medium",
    "large",
    "incremental",
    "range-cursor",
    "project-lifecycle",
    "long-churn",
  ])
    check(fixtureIds.has(id), `missing performance fixture ${id}`);
  const fixtures = new Map((p.fixture ?? []).map((f) => [f.id, f]));
  check(
    fixtures.get("small")?.authored_utf8_bytes === 4096 &&
      fixtures.get("small")?.carrier_blocks >= 3,
    "small fixture shape",
  );
  check(
    fixtures.get("medium")?.authored_utf8_bytes === 102400 &&
      fixtures.get("medium")?.carrier_blocks >= 10 &&
      fixtures.get("medium")?.authored_view_records === 10000,
    "medium fixture shape",
  );
  check(
    fixtures.get("large")?.authored_utf8_bytes === 1048576 &&
      fixtures.get("large")?.carrier_blocks >= 40,
    "large fixture shape",
  );
  const lifecycle = assetJson(fixtures.get("project-lifecycle")?.path);
  check(
    lifecycle?.sources?.length === 128 && unique(lifecycle.sources.map((s) => s.canonical_source)),
    "project lifecycle identities",
  );
  for (const s of lifecycle?.sources ?? [])
    check(fixtureIds.has(s.fixture_id), `missing lifecycle fixture ${s.fixture_id}`);
  const lifecycleIds = new Set(lifecycle?.sources?.map((s) => s.fixture_id));
  const workload = assetJson(fixtures.get("range-cursor")?.path);
  const queries = workload?.queries;
  // Workload geometry has one medium result basis. Miniature controls are
  // separate and must never override that basis on an individual query.
  check(workload?.source_fixture_id === "medium", "range/cursor query basis must be medium");
  check(
    same(workload?.prepared_result_edits, [
      { start: 40, end: 40, text: " " },
      { start: 42, end: 43, text: "" },
      { start: 40385, end: 40386, text: "1" },
    ]),
    "range/cursor query basis edits",
  );
  for (const q of queries ?? [])
    check(
      !("source_path" in q) &&
        !("formatted_path" in q) &&
        !("source_fixture_id" in q) &&
        Number.isInteger(q.start) &&
        Number.isInteger(q.end) &&
        q.start >= 0 &&
        q.end >= q.start &&
        q.end <= fixtures.get("medium")?.authored_utf8_bytes,
      `range/cursor query basis ${q.ordinal}`,
    );
  check(
    queries?.length === 1000 && queries.every((q, i) => q.ordinal === i + 1),
    "range/cursor 1000-query population",
  );
  const actions = p.churn_action ?? [];
  check(actions.length === 10000, "churn requires exactly 10,000 actions");
  const actionKinds = new Set();
  const highestRevision = new Map();
  const identities = assetJson("fixtures/formatter/performance/identities.json");
  for (const [i, a] of actions.entries()) {
    check(a.ordinal === i + 1, `noncontiguous churn ordinal ${i + 1}`);
    check(
      lifecycleIds.has(a.source_fixture_id) &&
        nonempty(a.action) &&
        Number.isInteger(a.delta_removed_bytes) &&
        Number.isInteger(a.delta_inserted_bytes) &&
        a.delta_removed_bytes >= 0 &&
        a.delta_inserted_bytes >= 0 &&
        a.delta_removed_bytes + a.delta_inserted_bytes <= 64,
      `churn source/edit ${i + 1}`,
    );
    check(
      identities?.profiles.some((p) => p.id === a.profile_id) &&
        identities.configs.some((p) => p.id === a.config_id) &&
        identities.policies.some((p) => p.id === a.policy_id),
      `churn identity ${i + 1}`,
    );
    check(
      nonempty(a.cancellation_observation) &&
        nonempty(a.expected_cache_disposition) &&
        MODE_IDS.includes(a.ceiling_mode),
      `churn disposition/ceiling ${i + 1}`,
    );
    assetJson(a.transition_payload);
    if (a.action === "cancellation")
      check(
        a.cancellation_observation === "after-first-final-sink-append" &&
          a.expected_cache_disposition === "no-admission" &&
          a.source_revision > (highestRevision.get(a.source_fixture_id) ?? -1),
        `cancellation observation/admission ${i + 1}`,
      );
    highestRevision.set(
      a.source_fixture_id,
      Math.max(a.source_revision, highestRevision.get(a.source_fixture_id) ?? -1),
    );
    if (a.action === "edit-revert")
      check(
        a.ceiling_mode === "incremental" &&
          a.pair_start_ordinal === i &&
          actions[i - 1]?.action === "incremental" &&
          actions[i - 1]?.source_fixture_id === a.source_fixture_id,
        `edit-revert pair allowance ${i + 1}`,
      );
    if (a.edit_payload) {
      const e = assetJson(a.edit_payload);
      check(
        Buffer.byteLength(e?.removed ?? "") === a.delta_removed_bytes &&
          Buffer.byteLength(e?.inserted ?? "") === a.delta_inserted_bytes,
        `churn payload ${i + 1}`,
      );
    }
    actionKinds.add(a.action);
  }
  for (const kind of [
    "cold",
    "warm",
    "incremental",
    "edit-revert",
    "profile-transition",
    "config-transition",
    "policy-transition",
    "cancellation",
  ])
    check(actionKinds.has(kind), `missing churn action ${kind}`);
  // A reviewed route is identified by its symbol; the reviewed inventory
  // records where each route lived when it was reviewed, while this lock binds
  // it to the file that carries it in the current tree (a route survives a
  // refactor that moves its file, which is exactly what "retain" means).
  const routeSymbols = inventory.routes.map((r) => r.symbol);
  check(unique(routeSymbols), "duplicate route owner");
  const sourceRoutes = lock.sourceInventory.currentRoutes;
  check(unique(sourceRoutes.map((r) => r.symbol)), "duplicate reviewed route symbol");
  check(
    same(routeSymbols.sort(), sourceRoutes.map((r) => r.symbol).sort()),
    "incomplete route inventory",
  );
  for (const route of inventory.routes) {
    const prior = sourceRoutes.find((r) => r.symbol === route.symbol);
    const expectedOwner = prior?.deletionOwner ?? prior?.integrationOwner ?? "markup-auto-close";
    check(
      route.disposition === prior?.disposition && route.owner === expectedOwner,
      `route ownership ${route.symbol}`,
    );
    // A provider is advertised either as a struct-literal field or assigned on
    // the capability set by its per-slot contributor.
    const symbol = route.symbol.split("::").at(-1);
    const pattern = symbol.endsWith("provider")
      ? new RegExp(`\\b${symbol}\\s*[:=]`)
      : new RegExp(`\\bfn\\s+${symbol}\\s*\\(`);
    check(
      pattern.test(lock.routeSources[route.path] ?? ""),
      `missing route symbol ${route.symbol}`,
    );
  }
  const laws = assetJson(base.laws);
  for (const key of ["coordinates", "renderer", "edits", "maps", "range", "cursor", "stability"])
    check(!!laws?.[key], `missing law ${key}`);
  for (const example of laws?.edits.examples ?? []) {
    let rendered = Buffer.from(example.source);
    const edits = [...example.edits].reverse();
    for (const e of edits)
      rendered = Buffer.concat([
        rendered.subarray(0, e.start),
        Buffer.from(e.text),
        rendered.subarray(e.end),
      ]);
    check(rendered.toString() === example.formatted, "invalid edit law example");
  }
  return errors;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const errors = validateFormatterLock(loadFormatterLock());
  for (const error of errors) console.error(error);
  console.log(errors.length ? "formatter lock: FAIL" : "formatter lock: PASS");
  process.exitCode = errors.length ? 1 : 0;
}
