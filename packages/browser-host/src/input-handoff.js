// Asynchronous input acquisition to committed-snapshot handoff
// (browser side). Acquisition runs asynchronously OUTSIDE any semantic
// callback; the acquired rows and probed-missing keys are handed to the
// WASM host in one synchronous commit; later synchronous observations
// answer from the committed rows only. A requested-but-unacquired key
// raises typed NeedInputs naming the canonical — never a synchronous
// fetch inside a resolver.

/** Typed demand for the next asynchronous acquisition wave. */
export class NeedInputsError extends Error {
  /** @param {string[]} keys canonicals the committed snapshot never acquired */
  constructor(keys) {
    super(`NeedInputs: ${[...keys].join(", ")}`);
    this.name = "NeedInputsError";
    this.keys = [...keys];
  }
}

/**
 * Validate one acquisition wave before it is committed. Returns the
 * list of failures; an empty list means the wave is committable.
 * Fail-closed: a duplicate canonical, an overlap between acquired
 * and probed-missing, or a non-string row is reported, never silently
 * collapsed.
 *
 * @param {Array<{canonical: string, content: string}>} files
 * @param {string[]} missing
 * @returns {string[]}
 */
export function acquisitionWaveFailures(files, missing) {
  const failures = [];
  if (!Array.isArray(files)) return ["files is not an array"];
  if (!Array.isArray(missing)) return ["missing is not an array"];

  const seen = new Map();
  for (const [index, row] of files.entries()) {
    if (row == null || typeof row !== "object") {
      failures.push(`files[${index}] is not a row object`);
      continue;
    }
    if (typeof row.canonical !== "string" || row.canonical.length === 0) {
      failures.push(`files[${index}].canonical is not a non-empty string`);
      continue;
    }
    if (typeof row.content !== "string") {
      failures.push(`files[${index}].content is not a string`);
      continue;
    }
    const prior = seen.get(row.canonical);
    if (prior === "file") {
      // Identical duplicates collapse host-side; distinct contents are
      // refused host-side. Either way the wave stays well-formed here.
      continue;
    }
    if (prior === "missing") {
      failures.push(`canonical '${row.canonical}' is both acquired and probed missing`);
      continue;
    }
    seen.set(row.canonical, "file");
  }
  for (const [index, canonical] of missing.entries()) {
    if (typeof canonical !== "string" || canonical.length === 0) {
      failures.push(`missing[${index}] is not a non-empty string`);
      continue;
    }
    if (seen.get(canonical) === "file") {
      failures.push(`canonical '${canonical}' is both acquired and probed missing`);
      continue;
    }
    seen.set(canonical, "missing");
  }
  return failures;
}

/**
 * Run ONE asynchronous acquisition wave. `readFile` is the browser
 * filesystem adapter (OPFS, File System Access, or a worker message
 * bridge) supplied by the caller; it is awaited here and NOWHERE else —
 * commit and observe below are synchronous and never re-enter
 * asynchronous browser APIs.
 *
 * @param {(canonical: string) => Promise<string | null | undefined>} readFile
 * @param {string[]} canonicals
 * @returns {Promise<{files: Array<{canonical: string, content: string}>, missing: string[]}>}
 */
export async function acquireInputWave(readFile, canonicals) {
  if (typeof readFile !== "function") {
    throw new TypeError("acquireInputWave requires an async readFile adapter");
  }
  const files = [];
  const missing = [];
  for (const canonical of canonicals) {
    const content = await readFile(canonical);
    if (content == null) missing.push(canonical);
    else files.push({ canonical, content });
  }
  const failures = acquisitionWaveFailures(files, missing);
  if (failures.length > 0) {
    throw new Error(`incoherent acquisition wave: ${failures.join("; ")}`);
  }
  return { files, missing };
}

/**
 * Commit one acquired wave through the host's committed input-snapshot
 * route. The receipt binds the basis id, row counts, execution host
 * and engine version.
 *
 * @param {{commitInputSnapshot: (files: Array<{canonical: string, content: string}>, missing: string[]) => any}} host
 * @param {{files: Array<{canonical: string, content: string}>, missing: string[]}} wave
 */
export function commitInputSnapshot(host, wave) {
  const failures = acquisitionWaveFailures(wave.files, wave.missing);
  if (failures.length > 0) {
    throw new Error(`incoherent acquisition wave: ${failures.join("; ")}`);
  }
  return host.commitInputSnapshot(wave.files, wave.missing);
}

/**
 * Observe one canonical under a committed snapshot. A `needInputs`
 * status raises typed {@link NeedInputsError}; `absent` (the wave
 * probed the key and recorded it missing) and `file` return as-is —
 * distinct answers, never conflated with the acquisition demand.
 *
 * @param {{observeInputSnapshot: (basisId: string, canonical: string) => {status: string, content?: string}}} host
 * @param {string} basisId
 * @param {string} canonical
 */
export function observeInputSnapshot(host, basisId, canonical) {
  const observation = host.observeInputSnapshot(basisId, canonical);
  if (observation?.status === "needInputs") {
    throw new NeedInputsError([canonical]);
  }
  return observation;
}

/**
 * Release a committed snapshot the caller no longer observes. Every basis
 * stays addressable until released, so a host that commits repeated waves
 * releases the superseded basis after switching to the new one.
 *
 * @param {{releaseInputSnapshot: (basisId: string) => boolean}} host
 * @param {string} basisId
 * @returns {boolean} whether a snapshot was stored under `basisId`
 */
export function releaseInputSnapshot(host, basisId) {
  return host.releaseInputSnapshot(basisId) === true;
}

/** Canonicals for the next acquisition wave from a typed NeedInputs result. */
export function needInputsKeys(error) {
  if (error instanceof NeedInputsError) return [...error.keys];
  return [];
}
