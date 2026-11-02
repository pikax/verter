#!/usr/bin/env node

/**
 * ci-cache-fit.mjs — read-only measurement of the repository's Actions cache.
 *
 * `collect` appends one JSON line per observation from GitHub's cache REST API.
 * It issues GET requests only. Listing a public repository's caches needs no
 * credential, so it runs anonymously unless `--token-env NAME` names a variable
 * holding a token; only the configured-limit endpoint then needs that token.
 * An unavailable endpoint is recorded with its HTTP status, never as zero.
 *
 *   node scripts/ci-cache-fit.mjs collect --out obs.jsonl
 *     [--repo owner/name] [--samples N] [--interval-seconds S] [--full-every K]
 *
 * `report` attributes those observations to the cache identities the workflows
 * declare and evaluates them against the allowance. It prints JSON, reads only
 * local files and never estimates: an identity without a recorded entry stays
 * unmeasured. An incomplete listing still records the size of each entry it read,
 * but its total is only a lower bound and never stands as the current occupancy.
 *
 *   node scripts/ci-cache-fit.mjs report --in obs.jsonl [--workflows dir]
 */

import { appendFileSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const API_ROOT = "https://api.github.com";
const PAGE_SIZE = 100;
const REQUEST_TIMEOUT_MS = 30_000;
const MAIN_REF = "refs/heads/main";
const CANDIDATE_FAMILIES = new Set(["rust-target", "compile-contracts", "explicit-cargo"]);

/** The documented allowance in both byte readings; neither is assumed to be the live value. */
export const ALLOWANCE_READINGS = Object.freeze({
  "10 GB (decimal)": 10_000_000_000,
  "10 GiB (binary)": 10 * 2 ** 30,
});

/** GET one API path; every outcome, including a transport failure, is data. */
export function createApi({ repo, token, fetchImpl = fetch }) {
  return async function get(path) {
    const headers = { Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" };
    if (token) headers.Authorization = `Bearer ${token}`;
    let response;
    try {
      response = await fetchImpl(`${API_ROOT}/repos/${repo}/${path}`, {
        method: "GET",
        headers,
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      });
    } catch (error) {
      return { ok: false, status: 0, message: String(error?.message ?? error) };
    }
    let body;
    try {
      body = await response.json();
    } catch {
      return { ok: false, status: response.status, message: "response body was not JSON" };
    }
    return response.ok
      ? { ok: true, status: response.status, body }
      : { ok: false, status: response.status, message: String(body?.message ?? "request failed") };
  };
}

/**
 * Read every page of one listing, largest entries first so a listing cut short
 * by rate limiting still holds most of the bytes. `complete` holds only when the
 * distinct entries read equal the one count the service reported on every page.
 * That rejects a repeated entry or a changed total, but not a swap (one entry
 * evicted and another saved between pages leaves both counts equal), so a full
 * observation also compares the listed bytes with the usage figure read beside
 * it (`usageMatchesListing` in the report).
 */
export async function listCaches(get, { ref } = {}) {
  const byId = new Map();
  const reported = new Set();
  const filter = ref ? `&ref=${encodeURIComponent(ref)}` : "";
  for (let page = 1; ; page += 1) {
    const result = await get(
      `actions/caches?per_page=${PAGE_SIZE}&page=${page}&sort=size_in_bytes&direction=desc${filter}`,
    );
    if (!result.ok) {
      return {
        complete: false,
        error: { page, status: result.status, message: result.message },
        entries: [...byId.values()],
      };
    }
    const { total_count: totalCount, actions_caches: items } = result.body;
    reported.add(totalCount);
    for (const item of items) byId.set(item.id, item);
    if (items.length < PAGE_SIZE) break;
  }
  const totals = [...reported];
  return {
    complete: totals.length === 1 && totals[0] === byId.size,
    reportedTotals: totals,
    entries: [...byId.values()],
  };
}

/** One observation: always the main-ref listing; everything else when `full`. */
export async function observe(get, { full, now = () => new Date() }) {
  const record = { at: now().toISOString(), scope: full ? "repository" : "main" };
  if (full) {
    const usage = await get("actions/cache/usage");
    record.usage = usage.ok
      ? { bytes: usage.body.active_caches_size_in_bytes, count: usage.body.active_caches_count }
      : { status: usage.status, message: usage.message };
    const limit = await get("actions/cache/storage-limit");
    record.limit = limit.ok ? limit.body : { status: limit.status, message: limit.message };
  }
  record.listing = await listCaches(get, full ? {} : { ref: MAIN_REF });
  return record;
}

/** Sample on a fixed cadence; `wait` is injected so a test never really waits. */
export async function collect(get, { samples, intervalMs, fullEvery, emit, wait = delay, now }) {
  for (let index = 0; index < samples; index += 1) {
    if (index > 0) await wait(intervalMs);
    emit(await observe(get, { full: index % fullEvery === 0, now }));
  }
}

/**
 * Name the consumer behind a service key. `candidate` marks a Rust build
 * identity that keying could retain; every other family is occupancy that must
 * be preserved. A key no rule recognises keeps its normalised spelling, so it
 * still counts in occupancy rather than disappearing. A rust-cache identity keeps
 * its environment hash, because the same job name built under a different
 * toolchain or environment is not the same build, while its lock hash only marks
 * a newer generation of that build.
 */
export function classifyKey(key) {
  const rust = /^v\d+-rust-(.+)-(Linux|macOS|Windows)-(\w+)-([0-9a-f]{8})-[0-9a-f]{8}$/.exec(key);
  if (rust) {
    return {
      family: "rust-target",
      candidate: true,
      identity: `rust-target:${rust[1]}@${rust[2]}-${rust[3]}#${rust[4]}`,
    };
  }
  const contract = /^(\w+)-(\w+)-compiler-contracts-v\d+-([a-z-]+?)-[0-9a-f]{64}/.exec(key);
  if (contract) {
    return {
      family: "compile-contracts",
      candidate: true,
      identity: `compile-contracts:${contract[3]}@${contract[1]}-${contract[2]}`,
    };
  }
  const cargo = /^(\w+)-([a-z-]*cargo[a-z-]*)-[0-9a-f]{64}$/.exec(key);
  if (cargo) {
    return {
      family: "explicit-cargo",
      candidate: true,
      identity: `explicit-cargo:${cargo[2]}@${cargo[1]}`,
    };
  }
  const preserved = [
    [/^sccache\//, "sccache"],
    [/^node-cache-/, "node-package-manager"],
    [/^vscode-host-/, "vscode-host"],
    [/-gradle-/, "gradle"],
    [/-playwright-/, "playwright"],
    [/-pnpm-/, "pnpm-store"],
  ];
  for (const [pattern, family] of preserved) {
    if (pattern.test(key)) return { family, candidate: false, identity: family };
  }
  const normalized = key.replace(/[0-9a-f]{32,}/g, "<hash>");
  return { family: "unclassified", candidate: false, identity: `unclassified:${normalized}` };
}

function declaredRows(workflow, job, groups, fields) {
  const uses = fields.uses ?? "";
  if (uses.startsWith("Swatinem/rust-cache@")) {
    const saveIf = fields["save-if"] ?? "";
    const savePolicy =
      saveIf === "false" ? "never" : saveIf.includes(MAIN_REF) ? "main-only" : "any-ref";
    const name = fields["shared-key"] ?? (fields.key ? `${fields.key}-${job}` : job);
    return [
      { workflow, job, mechanism: "rust-cache", savePolicy, identity: `rust-target:${name}` },
    ];
  }
  if (!uses.startsWith("actions/cache/save@") && !uses.startsWith("actions/cache@")) return [];
  const savePolicy = (fields.if ?? "").includes(MAIN_REF) ? "main-only" : "any-ref";
  const key = fields.key ?? "";
  const row = (identity) => ({ workflow, job, mechanism: "actions/cache", savePolicy, identity });
  if (key.includes("compiler-contract")) {
    return groups.map((group) => row(`compile-contracts:${group}`));
  }
  const cargo = /([a-z-]*cargo[a-z-]*)-\$\{\{/.exec(key);
  if (cargo) return [row(`explicit-cargo:${cargo[1].replace(/^-/, "")}`)];
  const preservedFamily = [
    ["gradle", "gradle"],
    ["playwright", "playwright"],
    ["vscode", "vscode-host"],
    ["pnpm", "pnpm-store"],
  ].find(([spelling]) => key.includes(spelling))?.[1];
  return [row(`preserved:${preservedFamily ?? job}`)];
}

/**
 * Whether a workflow's own triggers run it from main: `automatic` on a schedule,
 * an issue comment or a push that can include main (those events run with the
 * main ref), `manual` when only a dispatch can, otherwise `none`. A push with
 * no filter, or only a path filter, reaches main; `branches` must name main;
 * `branches-ignore` must not; a tag filter alone excludes branches. Block-style
 * and flow-style (`on: [push]`, `on: push`) trigger lists are both read.
 */
export function mainReachOf(text) {
  const triggers = new Set();
  const push = {
    branches: false,
    mainNamed: false,
    ignore: false,
    ignoresMain: false,
    tags: false,
  };
  let inOn = false;
  let trigger = null;
  let filter = null;
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*(#.*)?$/.test(line)) continue;
    const flow = /^on:\s*(?!#)(.+?)\s*(#.*)?$/.exec(line);
    if (flow) {
      for (const name of flow[1].replace(/[[\]]/g, "").split(",")) {
        if (/^[a-z_]+$/.test(name.trim())) triggers.add(name.trim());
      }
      break;
    }
    if (/^on:\s*(#.*)?$/.test(line)) {
      inOn = true;
      continue;
    }
    if (!inOn) continue;
    if (/^\S/.test(line)) break;
    const named = /^ {2}([a-z_]+):/.exec(line);
    if (named) {
      trigger = named[1];
      triggers.add(trigger);
      filter = null;
      continue;
    }
    if (trigger !== "push") continue;
    const key = /^\s+(branches|branches-ignore|tags|tags-ignore):\s*(.*)$/.exec(line);
    if (key) {
      filter = key[1];
      if (filter === "branches") push.branches = true;
      if (filter === "branches-ignore") push.ignore = true;
      if (filter === "tags") push.tags = true;
      if (filter === "branches") push.mainNamed ||= /\bmain\b/.test(key[2]);
      if (filter === "branches-ignore") push.ignoresMain ||= /\bmain\b/.test(key[2]);
    } else if (/^\s+[a-z_-]+:/.test(line)) {
      filter = null;
    } else if (filter === "branches") {
      push.mainNamed ||= /\bmain\b/.test(line);
    } else if (filter === "branches-ignore") {
      push.ignoresMain ||= /\bmain\b/.test(line);
    }
  }
  const pushesMain = !triggers.has("push")
    ? false
    : push.branches
      ? push.mainNamed
      : push.ignore
        ? !push.ignoresMain
        : !push.tags;
  if (pushesMain || triggers.has("schedule") || triggers.has("issue_comment")) return "automatic";
  return triggers.has("workflow_dispatch") ? "manual" : "none";
}

/**
 * Read one workflow's save-capable cache declarations without a YAML dependency,
 * one row per step or matrix group. Restore-only steps store nothing, and
 * setup-node's `cache:` input is reached through occupancy instead.
 */
export function scanWorkflow(workflow, text) {
  const rows = [];
  const mainReach = mainReachOf(text);
  let inJobs = false;
  let job = null;
  let groups = [];
  let fields = null;
  const flush = () => {
    if (fields) {
      for (const row of declaredRows(workflow, job, groups, fields))
        rows.push({ ...row, mainReach });
    }
    fields = null;
  };
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*(#.*)?$/.test(line)) continue;
    if (/^jobs:/.test(line)) {
      inJobs = true;
      continue;
    }
    if (!inJobs) continue;
    const jobHeader = /^ {2}([A-Za-z0-9_-]+):\s*(#.*)?$/.exec(line);
    if (jobHeader) {
      flush();
      job = jobHeader[1];
      groups = [];
      continue;
    }
    const matrixGroup = /^\s+group:\s*\[(.*)\]/.exec(line);
    if (matrixGroup) groups = matrixGroup[1].split(",").map((value) => value.trim());
    if (/^ {6}- /.test(line)) {
      flush();
      fields = {};
    }
    if (!fields) continue;
    const field = /^\s*(?:- )?(uses|if|id|key|shared-key|save-if):\s*(.*)$/.exec(line);
    if (field && fields[field[1]] === undefined) fields[field[1]] = field[2].trim();
  }
  flush();
  return rows;
}

export function scanWorkflowDirectory(directory) {
  return readdirSync(directory)
    .filter((name) => /\.ya?ml$/.test(name))
    .sort()
    .flatMap((name) => scanWorkflow(name, readFileSync(join(directory, name), "utf8")));
}

/** Occupancy of one complete listing, by consumer family and by ref class. */
function occupancyOf(record) {
  const byFamily = {};
  let listedBytes = 0;
  for (const entry of record.listing.entries) {
    const { family } = classifyKey(entry.key);
    const row = (byFamily[family] ??= {
      bytes: 0,
      count: 0,
      mainBytes: 0,
      otherRefBytes: 0,
      oldestSave: entry.created_at,
      newestSave: entry.created_at,
    });
    if (entry.created_at < row.oldestSave) row.oldestSave = entry.created_at;
    if (entry.created_at > row.newestSave) row.newestSave = entry.created_at;
    row.bytes += entry.size_in_bytes;
    row.count += 1;
    row[entry.ref === MAIN_REF ? "mainBytes" : "otherRefBytes"] += entry.size_in_bytes;
    listedBytes += entry.size_in_bytes;
  }
  const preservedBytes = Object.entries(byFamily).reduce(
    (total, [family, row]) =>
      total + (CANDIDATE_FAMILIES.has(family) ? row.otherRefBytes : row.bytes),
    0,
  );
  const usageBytes = record.usage?.bytes ?? null;
  // A listing cut short only sums what it read, so its preserved figure is a
  // lower bound. Whatever the service counted but the listing never attributed
  // may be preserved, which bounds it from above. Usage and listing are read
  // moments apart, so a listing that read more than the usage never lowers it.
  const preservedUpperBytes = record.listing.complete
    ? preservedBytes
    : usageBytes === null
      ? null
      : preservedBytes + Math.max(0, usageBytes - listedBytes);
  return {
    at: record.at,
    complete: record.listing.complete,
    listedBytes,
    preservedBytes,
    preservedUpperBytes,
    usageMatchesListing: usageBytes === null ? null : usageBytes === listedBytes,
    listedCount: record.listing.entries.length,
    usageBytes,
    usageCount: record.usage?.count ?? null,
    byFamily,
  };
}

/**
 * Attribute every main-saved entry any listing read to its identity: the service
 * reported that entry's size even when the listing around it was cut short. The
 * size of an identity is its newest save; older generations stay visible through
 * `generations` and in occupancy, and are never added to that size. Only a
 * complete listing can say an identity is no longer resident.
 */
function attributeMainEntries(observations) {
  const samples = observations.map((record) => ({
    at: record.at,
    complete: record.listing.complete,
    entries: record.listing.entries.filter((entry) => entry.ref === MAIN_REF),
  }));
  const identities = new Map();
  for (const { entries } of samples) {
    for (const entry of entries) {
      const { family, candidate, identity } = classifyKey(entry.key);
      if (!candidate) continue;
      const row = identities.get(identity) ?? { identity, family, byId: new Map() };
      row.byId.set(entry.id, entry);
      identities.set(identity, row);
    }
  }
  const last = samples.findLast((sample) => sample.complete);
  return [...identities.values()]
    .map(({ identity, family, byId }) => {
      const saves = [...byId.values()].sort(
        (a, b) => a.created_at.localeCompare(b.created_at) || a.id - b.id,
      );
      const newest = saves.at(-1);
      const seen = samples.filter((sample) => sample.entries.some((entry) => byId.has(entry.id)));
      return {
        identity,
        family,
        bytes: newest.size_in_bytes,
        savedAt: newest.created_at,
        generations: saves.length,
        samplesSeen: seen.length,
        firstSeen: seen[0].at,
        lastSeen: seen.at(-1).at,
        residentAtEnd: last ? last.entries.some((entry) => byId.has(entry.id)) : null,
      };
    })
    .sort((a, b) => a.bytes - b.bytes || a.identity.localeCompare(b.identity));
}

/** A declaration names no platform and may hold a matrix expression, so it is a template. */
function matchesDeclaration(declaredIdentity, measuredIdentity) {
  const template = declaredIdentity
    .split(/\$\{\{.*?\}\}/)
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join(".+");
  return new RegExp(`^${template}@`).test(measuredIdentity);
}

function admit(budgetBytes, ordered) {
  const fitting = [];
  const excluded = [];
  let usedBytes = 0;
  for (const { identity, bytes } of ordered) {
    if (usedBytes + bytes <= budgetBytes) {
      usedBytes += bytes;
      fitting.push(identity);
    } else {
      excluded.push({ identity, bytes, budgetLeftBytes: budgetBytes - usedBytes });
    }
  }
  return { budgetBytes, usedBytes, headroomBytes: budgetBytes - usedBytes, fitting, excluded };
}

/**
 * Evaluate fit from recorded bytes only. Measured candidates are admitted
 * smallest first, which retains the most identities; no reserve is added, no
 * eviction is assumed, and an unmeasured identity rules out `all-fit`.
 *
 * Preserved occupancy is only ever sampled, and a sample taken from a listing
 * cut short is a lower bound. Two budgets follow from it:
 * - `provisional`: allowance minus the lower bound. That is an UPPER bound on
 *   the space available, so the identities it admits are a ceiling, not a fit.
 * - `bounded`: allowance minus the upper bound on preserved occupancy at the
 *   sampled moments (`null` when some sample has no upper bound). Its subset
 *   held at every moment sampled; a peak between samples stays unobserved.
 * `outcome` is `all-fit` only when `bounded` admits every identity and none is
 * unmeasured. Anything else is `unestablished`: partial-fit is never certified
 * from sampled occupancy, and adoption stays held.
 */
export function evaluateFit({
  allowanceBytes,
  preservedLowerBytes,
  preservedUpperBytes,
  candidates,
  unmeasured,
}) {
  const ordered = [...candidates].sort(
    (a, b) => a.bytes - b.bytes || a.identity.localeCompare(b.identity),
  );
  const provisional = admit(allowanceBytes - preservedLowerBytes, ordered);
  const bounded =
    preservedUpperBytes === null ? null : admit(allowanceBytes - preservedUpperBytes, ordered);
  const allFit =
    bounded !== null &&
    bounded.excluded.length === 0 &&
    bounded.fitting.length > 0 &&
    unmeasured.length === 0;
  return {
    allowanceBytes,
    preservedLowerBytes,
    preservedUpperBytes,
    outcome: allFit ? "all-fit" : "unestablished",
    provisional,
    bounded,
  };
}

export function summarize(observations, declarations) {
  const repository = observations.filter((record) => record.scope === "repository");
  const completeRepository = repository.filter((record) => record.listing.complete);
  const failures = observations.flatMap((record) => {
    const found = [];
    if (!record.listing.complete) {
      const reason = record.listing.error ?? { message: "entries changed while paging" };
      found.push({ at: record.at, source: "listing", ...reason });
    }
    if (record.usage?.status) found.push({ at: record.at, source: "usage", ...record.usage });
    return found;
  });
  const summary = {
    window: {
      first: observations[0]?.at ?? null,
      last: observations.at(-1)?.at ?? null,
      observations: observations.length,
      completeRepositoryListings: completeRepository.length,
      repositoryListings: repository.length,
      completeMainListings: observations.filter(
        (record) => record.scope === "main" && record.listing.complete,
      ).length,
      mainListings: observations.filter((record) => record.scope === "main").length,
    },
    failures,
    limit: observations.findLast((record) => record.limit)?.limit ?? null,
  };
  if (completeRepository.length === 0) return { ...summary, available: false };

  // An incomplete listing is a lower bound on what it read, so it can only raise a peak.
  // Its preserved figure is likewise a lower bound; `preservedUpperBytes` bounds it from above.
  const occupancies = repository.map(occupancyOf);
  const latest = occupancies.findLast((occupancy) => occupancy.complete);
  const peakByFamily = {};
  for (const { byFamily } of occupancies) {
    for (const [family, row] of Object.entries(byFamily)) {
      peakByFamily[family] = Math.max(peakByFamily[family] ?? 0, row.bytes);
    }
  }
  const identities = attributeMainEntries(observations);
  const isPreserved = ({ identity }) => identity.startsWith("preserved:");
  const declared = declarations.map((row) => {
    const family = row.identity.slice("preserved:".length);
    const measuredAs = isPreserved(row)
      ? peakByFamily[family] === undefined
        ? []
        : [family]
      : identities
          .filter(({ identity }) => matchesDeclaration(row.identity, identity))
          .map(({ identity }) => identity);
    return { ...row, measuredAs };
  });
  const unmeasuredOf = (keep) => [
    ...new Set(
      declared
        .filter((row) => row.savePolicy !== "never" && row.mainReach === "automatic")
        .filter((row) => row.measuredAs.length === 0 && keep(row))
        .map((row) => row.identity),
    ),
  ];
  const unmeasured = unmeasuredOf((row) => !isPreserved(row)).sort();
  const unmeasuredPreserved = unmeasuredOf(isPreserved).sort();
  const preservedLowerBytes = Math.max(...occupancies.map(({ preservedBytes }) => preservedBytes));
  const upperBounds = occupancies.map(({ preservedUpperBytes }) => preservedUpperBytes);
  const preservedUpperBytes = upperBounds.includes(null) ? null : Math.max(...upperBounds);
  const candidates = identities.map(({ identity, bytes }) => ({ identity, bytes }));
  const listed = occupancies
    .filter(({ complete }) => complete)
    .map(({ listedBytes }) => listedBytes);
  const reportedUsage = occupancies.flatMap(({ usageBytes }) => usageBytes ?? []);
  return {
    ...summary,
    available: true,
    occupancy: {
      latest,
      minListedBytes: Math.min(...listed),
      peakListedBytes: Math.max(...listed),
      peakUsageBytes: Math.max(...reportedUsage),
      peakPreservedLowerBytes: preservedLowerBytes,
      peakPreservedUpperBytes: preservedUpperBytes,
      peakByFamily,
      series: occupancies.map(({ byFamily, ...row }) => ({
        ...row,
        familyBytes: Object.fromEntries(
          Object.entries(byFamily).map(([family, { bytes }]) => [family, bytes]),
        ),
      })),
      headroomAtLatest: Object.fromEntries(
        Object.entries(ALLOWANCE_READINGS).map(([name, bytes]) => [
          name,
          bytes - latest.listedBytes,
        ]),
      ),
    },
    identities,
    declared,
    unmeasured,
    unmeasuredPreserved,
    fit: Object.fromEntries(
      Object.entries(ALLOWANCE_READINGS).map(([name, allowanceBytes]) => [
        name,
        evaluateFit({
          allowanceBytes,
          preservedLowerBytes,
          preservedUpperBytes,
          candidates,
          unmeasured,
        }),
      ]),
    ),
  };
}

export function parseObservations(text) {
  return text
    .split(/\r?\n/)
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line));
}

async function main(argv) {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      out: { type: "string" },
      in: { type: "string" },
      repo: { type: "string", default: "pikax/verter" },
      samples: { type: "string", default: "1" },
      "interval-seconds": { type: "string", default: "240" },
      "full-every": { type: "string", default: "1" },
      "token-env": { type: "string" },
      workflows: { type: "string", default: join(".github", "workflows") },
    },
  });
  if (positionals[0] === "report" && values.in) {
    const observations = parseObservations(readFileSync(values.in, "utf8"));
    const summary = summarize(observations, scanWorkflowDirectory(values.workflows));
    console.log(JSON.stringify(summary, null, 2));
    return;
  }
  if (positionals[0] !== "collect" || !values.out) {
    throw new Error(
      "usage: ci-cache-fit.mjs collect --out <file> [options] | report --in <file> [--workflows <dir>]",
    );
  }
  const count = (name) => {
    const value = Number(values[name]);
    if (!Number.isInteger(value) || value < 1)
      throw new Error(`--${name} must be a positive integer`);
    return value;
  };
  const samples = count("samples");
  const intervalSeconds = count("interval-seconds");
  const fullEvery = count("full-every");
  const token = values["token-env"] ? process.env[values["token-env"]] : undefined;
  await collect(createApi({ repo: values.repo, token }), {
    samples,
    intervalMs: intervalSeconds * 1000,
    fullEvery,
    emit: (record) => {
      appendFileSync(values.out, `${JSON.stringify(record)}\n`);
      const { at, scope, listing, usage } = record;
      const total = usage?.bytes === undefined ? "" : ` usage=${usage.bytes}`;
      console.log(
        `${at} ${scope} entries=${listing.entries.length} complete=${listing.complete}${total}`,
      );
    },
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
