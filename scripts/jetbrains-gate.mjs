#!/usr/bin/env node
// jetbrains-product gate runner (JBT1).
//
// Runs the pinned extensions/jetbrains Gradle build's JVM tests, the IntelliJ
// plugin verifier and installable packaging, and fails closed when the JDK, the
// pinned IntelliJ Platform SDK (via the Gradle build itself), the packaged
// build output or the test evidence is missing. A missing precondition NEVER
// passes: there is no pass-with-no-tests and no pass-without-artifacts path.
//
// The gate also pins the supported-IDE declaration to the live build inputs:
// tests/jetbrains-product/JBT1/products/ide-support-range.v1.json must agree
// with extensions/jetbrains/gradle.properties (product code, SDK version,
// build number, since/until range, plugin id), and the packaged descriptor
// inside the built distribution must carry the same range. Any drift is a
// failure, not a warning.
//
// Usage:
//   node scripts/jetbrains-gate.mjs [--root <repo>] [--java-home <jdk>]
//
// The JDK is resolved from --java-home, then JAVA_HOME. The IntelliJ Platform
// SDK is resolved by the Gradle build (extensions/jetbrains is pinned to
// WebStorm 2026.2.3); a missing or unresolvable SDK fails the Gradle build and
// therefore this gate.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { inflateRawSync } from "node:zlib";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const defaultRepoRoot = path.dirname(scriptDir);

export const GRADLE_PROJECT_DIR = "extensions/jetbrains";
export const GRADLE_PROPERTIES_PATH = path.join(GRADLE_PROJECT_DIR, "gradle.properties");
export const DECLARATION_PATH = path.join(
  "tests",
  "jetbrains-product",
  "JBT1",
  "products",
  "ide-support-range.v1.json",
);

export const REQUIRED_GRADLE_PROPERTIES = [
  "platformType",
  "platformVersion",
  "platformBuild",
  "pluginSinceBuild",
  "pluginUntilBuild",
  "pluginId",
  "pluginVersion",
];

// The Gradle invocation is assembled by this script, not the caller; every
// argument must match this allowlist shape so the Windows single-string shell
// invocation below can never smuggle a metacharacter.
export const GRADLE_ARG_RE = /^[A-Za-z0-9:_\-.=]+$/;

// ---------------------------------------------------------------------------
// Inputs: gradle.properties + the supported-IDE declaration product.
// ---------------------------------------------------------------------------

export function parseGradleProperties(text) {
  const properties = {};
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === "" || line.startsWith("#") || line.startsWith("!")) continue;
    const eq = line.indexOf("=");
    if (eq === -1) continue;
    const key = line.slice(0, eq).trim();
    const value = line.slice(eq + 1).trim();
    if (key !== "") properties[key] = value;
  }
  return properties;
}

export function validatePins({ properties, declaration }) {
  const errors = [];
  for (const key of REQUIRED_GRADLE_PROPERTIES) {
    if (!properties[key]) errors.push(`gradle.properties is missing required pin '${key}'`);
  }
  if (errors.length > 0) return { ok: false, errors };

  const edition = declaration?.supportedEditions?.[0];
  if (!edition) {
    errors.push("declaration product has no supportedEditions entry");
    return { ok: false, errors };
  }

  const expected = [
    ["productCode", edition.productCode, properties.platformType],
    ["sdkVersion", edition.sdkVersion, properties.platformVersion],
    ["buildNumber", edition.buildNumber, properties.platformBuild],
    ["sinceBuild", edition.sinceBuild, properties.pluginSinceBuild],
    ["untilBuild", edition.untilBuild, properties.pluginUntilBuild],
  ];
  for (const [field, declared, pinned] of expected) {
    if (declared !== pinned) {
      errors.push(
        `supported-IDE declaration drift: declaration ${field}='${declared}' vs gradle.properties pin '${pinned}'`,
      );
    }
  }
  if (declaration.pluginId !== properties.pluginId) {
    errors.push(
      `plugin id drift: declaration '${declaration.pluginId}' vs gradle.properties '${properties.pluginId}'`,
    );
  }
  // The declaration's unsupported-editions population is what keeps "not
  // claimed" falsifiable; an empty list means the declaration stopped
  // delimiting its own claim.
  if (
    !Array.isArray(declaration.unsupportedEditionsNotClaimed) ||
    declaration.unsupportedEditionsNotClaimed.length === 0
  ) {
    errors.push(
      "declaration product must list unsupportedEditionsNotClaimed (unsupported editions are not claimed)",
    );
  }
  return { ok: errors.length === 0, errors };
}

// ---------------------------------------------------------------------------
// JDK resolution (fail closed: no JDK, no gate).
// ---------------------------------------------------------------------------

export function resolveJdkHome({ javaHomeArg, env = process.env }) {
  const candidates = [javaHomeArg, env.JAVA_HOME].filter(Boolean);
  for (const home of candidates) {
    const javaBin = path.join(home, "bin", process.platform === "win32" ? "java.exe" : "java");
    if (existsSync(javaBin)) return { home, javaBin };
  }
  return {
    home: null,
    javaBin: null,
    reason:
      "no usable JDK found: pass --java-home or set JAVA_HOME (a JDK 21 toolchain is required by extensions/jetbrains)",
  };
}

// ---------------------------------------------------------------------------
// Test evidence: JUnit XML produced by the Gradle `test` task.
// ---------------------------------------------------------------------------

export function collectTestEvidence(testResultsDir) {
  let files;
  try {
    files = readdirSync(testResultsDir).filter((name) => /^TEST-.*\.xml$/.test(name));
  } catch {
    return {
      ok: false,
      reason: `no JUnit XML test results under ${testResultsDir} — the test task produced no evidence`,
    };
  }
  if (files.length === 0) {
    return {
      ok: false,
      reason: `zero TEST-*.xml files under ${testResultsDir} — pass-with-no-tests is a gate failure`,
    };
  }
  let tests = 0;
  let failures = 0;
  let errors = 0;
  let skipped = 0;
  for (const name of files) {
    const xml = readFileSync(path.join(testResultsDir, name), "utf8");
    const suite = xml.match(/<testsuite\b[^>]*>/)?.[0] ?? "";
    const attr = (key) =>
      Number.parseInt(suite.match(new RegExp(`\\b${key}="(-?\\d+)"`))?.[1] ?? "0", 10);
    tests += attr("tests");
    failures += attr("failures");
    errors += attr("errors");
    skipped += attr("skipped");
  }
  if (tests <= 0) {
    return {
      ok: false,
      reason: `JUnit XML present but reports 0 executed tests under ${testResultsDir}`,
    };
  }
  if (failures > 0 || errors > 0) {
    return { ok: false, reason: `JVM test failures: ${failures} failures, ${errors} errors` };
  }
  return { ok: true, files: files.length, tests, failures, errors, skipped };
}

// ---------------------------------------------------------------------------
// Minimal ZIP reader (local-file + central-directory walk, stored + deflate).
// Node has no built-in unzip; the packaged descriptor must be read without a
// dependency and without a platform-specific `unzip` binary.
// ---------------------------------------------------------------------------

const EOCD_SIGNATURE = 0x06054b50;
const CENTRAL_SIGNATURE = 0x02014b50;
const LOCAL_SIGNATURE = 0x04034b50;

export function listZipEntries(buf) {
  // Scan back from EOF for the End Of Central Directory record (max 64 KiB
  // comment). Zip64 archives are rejected explicitly: buildPlugin zips of a
  // plugin skeleton are nowhere near the 4 GiB boundary.
  const eocdMin = Math.max(0, buf.length - 0xffff - 22);
  let eocd = -1;
  for (let i = buf.length - 22; i >= eocdMin; i--) {
    if (buf.readUInt32LE(i) === EOCD_SIGNATURE) {
      eocd = i;
      break;
    }
  }
  if (eocd === -1)
    throw new Error("zip: end-of-central-directory record not found (not a zip file?)");
  if (buf.readUInt32LE(eocd + 16) === 0xffffffff) {
    throw new Error("zip: zip64 archives are not supported by the gate's descriptor reader");
  }
  const entryCount = buf.readUInt16LE(eocd + 10);
  let offset = buf.readUInt32LE(eocd + 16);

  const entries = [];
  for (let i = 0; i < entryCount; i++) {
    if (buf.readUInt32LE(offset) !== CENTRAL_SIGNATURE) {
      throw new Error(`zip: corrupt central directory at entry ${i}`);
    }
    const method = buf.readUInt16LE(offset + 10);
    const compressedSize = buf.readUInt32LE(offset + 20);
    const nameLength = buf.readUInt16LE(offset + 28);
    const extraLength = buf.readUInt16LE(offset + 30);
    const commentLength = buf.readUInt16LE(offset + 32);
    const localHeaderOffset = buf.readUInt32LE(offset + 42);
    const name = buf.subarray(offset + 46, offset + 46 + nameLength).toString("utf8");
    entries.push({ name, method, compressedSize, localHeaderOffset });
    offset += 46 + nameLength + extraLength + commentLength;
  }
  return entries;
}

export function findZipEntries(buf, entrySuffix) {
  return listZipEntries(buf).filter((entry) => entry.name.endsWith(entrySuffix));
}

export function readZipEntryBytes(buf, entry) {
  const o = entry.localHeaderOffset;
  if (buf.readUInt32LE(o) !== LOCAL_SIGNATURE) {
    throw new Error(`zip: corrupt local header for '${entry.name}'`);
  }
  const nameLength = buf.readUInt16LE(o + 26);
  const extraLength = buf.readUInt16LE(o + 28);
  const dataStart = o + 30 + nameLength + extraLength;
  const data = buf.subarray(dataStart, dataStart + entry.compressedSize);
  if (entry.method === 0) return data;
  if (entry.method === 8) return inflateRawSync(data);
  throw new Error(`zip: unsupported compression method ${entry.method} for '${entry.name}'`);
}

export function readZipTextEntry(buf, entry) {
  return readZipEntryBytes(buf, entry).toString("utf8");
}

// The buildPlugin distribution layout is `<plugin>/lib/<plugin>.jar` with the
// descriptor INSIDE the nested jar; a direct `<plugin>/META-INF/plugin.xml` is
// accepted too (older/simplified layouts) so the check follows the artifact,
// not one assumed shape.
export function readPackagedDescriptor(zipPath) {
  const buf = readFileSync(zipPath);
  const direct = findZipEntries(buf, "META-INF/plugin.xml");
  if (direct.length > 0) return readZipTextEntry(buf, direct[0]);

  const jars = findZipEntries(buf, ".jar").filter((entry) => entry.name.includes("/lib/"));
  for (const jar of jars) {
    const jarBytes = readZipEntryBytes(buf, jar);
    const inner = findZipEntries(jarBytes, "META-INF/plugin.xml");
    if (inner.length > 0) return readZipTextEntry(jarBytes, inner[0]);
  }
  throw new Error(
    `zip: no META-INF/plugin.xml (direct or inside a lib/*.jar) in ${zipPath} — not an installable plugin layout`,
  );
}

export function verifyPackagedPlugin({ zipPath, pluginId, sinceBuild, untilBuild }) {
  const errors = [];
  let descriptor;
  try {
    descriptor = readPackagedDescriptor(zipPath);
  } catch (err) {
    return { ok: false, errors: [err.message] };
  }
  const attr = (name) => descriptor.match(new RegExp(`\\b${name}="([^"]*)"`))?.[1];
  const idElement = descriptor.match(/<id>\s*([^<]+?)\s*<\/id>/)?.[1];
  const descriptorId = idElement ?? attr("id");
  if (descriptorId !== pluginId) {
    errors.push(`packaged plugin id '${descriptorId}' does not match declared '${pluginId}'`);
  }
  if (attr("since-build") !== sinceBuild) {
    errors.push(
      `packaged since-build '${attr("since-build")}' does not match declared '${sinceBuild}'`,
    );
  }
  if (attr("until-build") !== untilBuild) {
    errors.push(
      `packaged until-build '${attr("until-build")}' does not match declared '${untilBuild}'`,
    );
  }
  return { ok: errors.length === 0, errors };
}

// Unpacks a distribution zip into `installDir` (fresh each run: the previous
// rehearsal content is removed first so a stale tree can never stand in for
// this run's artifact). Directory entries create directories; file entries
// are written with their parents created.
export function extractDistribution(zipPath, installDir) {
  let buf;
  let entries;
  try {
    buf = readFileSync(zipPath);
    entries = listZipEntries(buf);
  } catch (err) {
    return { ok: false, reason: `install rehearsal: cannot read ${zipPath}: ${err.message}` };
  }
  try {
    rmSync(installDir, { recursive: true, force: true });
    let files = 0;
    for (const entry of entries) {
      // Zip entries are supposed to use "/". Backslash is still a separator
      // on win32 `path.join`, so reject `..` segments, `\`, and drive-letter
      // prefixes so a `..\..\evil` name cannot escape the rehearsal directory.
      const parts = entry.name.split(/[/\\]/).filter((part) => part !== "");
      if (
        entry.name.startsWith("/") ||
        entry.name.startsWith("\\") ||
        entry.name.includes("\\") ||
        /^[A-Za-z]:[\\/]/.test(entry.name) ||
        parts.includes("..")
      ) {
        return { ok: false, reason: `install rehearsal: unsafe zip entry name '${entry.name}'` };
      }
      if (entry.name.endsWith("/")) {
        mkdirSync(path.join(installDir, ...parts), { recursive: true });
        continue;
      }
      const target = path.join(installDir, ...parts);
      mkdirSync(path.dirname(target), { recursive: true });
      writeFileSync(target, readZipEntryBytes(buf, entry));
      files++;
    }
    if (files === 0) {
      return { ok: false, reason: `install rehearsal: ${zipPath} contained no files to unpack` };
    }
    return { ok: true, installDir, files };
  } catch (err) {
    return {
      ok: false,
      reason: `install rehearsal: failed to unpack ${zipPath} into ${installDir}: ${err.message}`,
    };
  }
}

// ---------------------------------------------------------------------------
// The gate itself. All process/filesystem seams are injectable so the
// discriminating failure paths are unit-testable without a JVM.
// ---------------------------------------------------------------------------

export function defaultGradleInvocation(repoRoot, args) {
  for (const arg of args) {
    if (!GRADLE_ARG_RE.test(arg)) {
      throw new Error(`jetbrains-gate: refusing to pass non-allowlisted gradle argument '${arg}'`);
    }
  }
  const wrapper = path.join(
    repoRoot,
    GRADLE_PROJECT_DIR,
    process.platform === "win32" ? "gradlew.bat" : "gradlew",
  );
  if (process.platform === "win32") {
    // A .bat shim cannot be spawned directly without a shell; all arguments
    // are constant allowlisted tokens (see GRADLE_ARG_RE), so a single quoted
    // command string is safe — only the wrapper path needs quoting.
    return { command: `"${wrapper}" ${args.join(" ")}`, args: [], shell: true };
  }
  return { command: wrapper, args, shell: false };
}

export function runGate({
  repoRoot = defaultRepoRoot,
  javaHomeArg,
  env = process.env,
  gradleTasks = ["test", "verifyPlugin", "buildPlugin"],
  extraGradleArgs = ["--console=plain"],
  spawnFn = spawnSync,
  log = (line) => process.stdout.write(`${line}\n`),
} = {}) {
  const failures = [];

  const propertiesPath = path.join(repoRoot, GRADLE_PROPERTIES_PATH);
  if (!existsSync(propertiesPath)) {
    return {
      ok: false,
      failures: [`missing ${GRADLE_PROPERTIES_PATH} — the pinned plugin build does not exist`],
    };
  }
  const properties = parseGradleProperties(readFileSync(propertiesPath, "utf8"));

  const declarationPath = path.join(repoRoot, DECLARATION_PATH);
  if (!existsSync(declarationPath)) {
    return {
      ok: false,
      failures: [
        `missing ${DECLARATION_PATH} — the supported-IDE range declaration does not exist`,
      ],
    };
  }
  let declaration;
  try {
    declaration = JSON.parse(readFileSync(declarationPath, "utf8"));
  } catch (err) {
    return { ok: false, failures: [`supported-IDE declaration is not valid JSON: ${err.message}`] };
  }

  const pins = validatePins({ properties, declaration });
  if (!pins.ok) return { ok: false, failures: pins.errors };

  const jdk = resolveJdkHome({ javaHomeArg, env });
  if (!jdk.home) return { ok: false, failures: [jdk.reason] };

  const { command, args, shell } = defaultGradleInvocation(repoRoot, [
    ...gradleTasks,
    ...extraGradleArgs,
  ]);
  log(
    `jetbrains-gate: ${command}${args.length ? ` ${args.join(" ")}` : ""} (JAVA_HOME=${jdk.home})`,
  );
  const gradleEnv = { ...env, JAVA_HOME: jdk.home };
  const result = spawnFn(command, args, {
    cwd: path.join(repoRoot, GRADLE_PROJECT_DIR),
    env: gradleEnv,
    shell,
    stdio: "inherit",
    encoding: "utf8",
  });
  if (result.error) {
    return { ok: false, failures: [`gradle wrapper failed to start: ${result.error.message}`] };
  }
  if (result.status !== 0) {
    // A missing/unresolvable pinned SDK, a JVM test failure and a verifier
    // incompatibility all land here: the Gradle build failed, so the gate
    // fails. JBT1-AC1.
    return {
      ok: false,
      failures: [
        `gradle exited ${result.status}: JVM tests, plugin verification or packaging failed (a missing JDK/SDK or build failure is a gate failure, never a skip)`,
      ],
    };
  }

  const projectDir = path.join(repoRoot, GRADLE_PROJECT_DIR);

  const testEvidence = collectTestEvidence(path.join(projectDir, "build", "test-results", "test"));
  if (!testEvidence.ok) return { ok: false, failures: [testEvidence.reason] };
  log(
    `jetbrains-gate: JVM tests ${testEvidence.tests} executed, ${testEvidence.failures} failures, ${testEvidence.errors} errors (${testEvidence.files} suites)`,
  );

  let distributions = [];
  try {
    distributions = readdirSync(path.join(projectDir, "build", "distributions"))
      .filter((name) => name.endsWith(".zip"))
      .map((name) => path.join(projectDir, "build", "distributions", name));
  } catch {
    // handled below
  }
  if (distributions.length === 0) {
    return {
      ok: false,
      failures: [
        "no *.zip under extensions/jetbrains/build/distributions — the build produced no installable output (JBT1-AC1)",
      ],
    };
  }

  for (const dist of distributions) {
    const verification = verifyPackagedPlugin({
      zipPath: dist,
      pluginId: properties.pluginId,
      sinceBuild: properties.pluginSinceBuild,
      untilBuild: properties.pluginUntilBuild,
    });
    if (!verification.ok) {
      failures.push(...verification.errors.map((err) => `${path.basename(dist)}: ${err}`));
    } else {
      log(
        `jetbrains-gate: packaged ${path.basename(dist)} matches the declared range ${properties.pluginSinceBuild}..${properties.pluginUntilBuild}`,
      );
    }
  }
  if (failures.length > 0) return { ok: false, failures };

  // JBT1-AC2 install rehearsal: unpack the packaged distribution into a
  // plugins directory OUTSIDE the repository, proving the zip extracts as a
  // real plugin layout. A full real-IDE boot with the plugin installed is the
  // JBT1H real-IDE harness's evidence, not this gate's.
  const installRoot =
    env.JETBRAINS_GATE_INSTALL_DIR && path.isAbsolute(env.JETBRAINS_GATE_INSTALL_DIR)
      ? env.JETBRAINS_GATE_INSTALL_DIR
      : path.join(repoRoot, "..", ".jetbrains-gate-install");
  const installDir = path.join(installRoot, "plugins");
  const extraction = extractDistribution(distributions[0], installDir);
  if (!extraction.ok) return { ok: false, failures: [extraction.reason] };
  log(
    `jetbrains-gate: install rehearsal unpacked ${path.basename(distributions[0])} into ${installDir} (outside the repository)`,
  );

  // Verifier evidence: the verifyPlugin task already failed the build on any
  // incompatibility (failureLevel ALL, pinned build); the gate additionally
  // requires the verifier's report output to exist, so a silently-skipped
  // verifier cannot pass.
  const verifierReports = path.join(projectDir, "build", "reports", "pluginVerifier");
  let verifierReportFiles = [];
  try {
    verifierReportFiles = readdirSync(verifierReports);
  } catch {
    // handled below
  }
  if (verifierReportFiles.length === 0) {
    return {
      ok: false,
      failures: [
        `no plugin verifier reports under ${verifierReports} — verification did not run (JBT1-AC3)`,
      ],
    };
  }
  log(`jetbrains-gate: plugin verifier reports present for ${verifierReportFiles.join(", ")}`);

  return {
    ok: true,
    summary: {
      tests: testEvidence.tests,
      distributions: distributions.map((dist) => path.basename(dist)),
      verifierTargets: verifierReportFiles,
      installRehearsalDir: installDir,
    },
  };
}

function main() {
  const argv = process.argv.slice(2);
  const args = { root: defaultRepoRoot, javaHome: undefined };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--root") args.root = path.resolve(argv[++i]);
    else if (argv[i] === "--java-home") args.javaHome = path.resolve(argv[++i]);
    else {
      process.stderr.write(`jetbrains-gate: unknown argument '${argv[i]}'\n`);
      process.exit(2);
    }
  }
  const outcome = runGate({ repoRoot: args.root, javaHomeArg: args.javaHome });
  if (outcome.ok) {
    process.stdout.write(`jetbrains-gate: PASS ${JSON.stringify(outcome.summary)}\n`);
    return;
  }
  for (const failure of outcome.failures)
    process.stderr.write(`jetbrains-gate: FAIL — ${failure}\n`);
  process.exit(1);
}

if (
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
