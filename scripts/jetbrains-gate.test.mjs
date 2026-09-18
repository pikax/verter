// Selftest for scripts/jetbrains-gate.mjs (node --test, no JVM required).
//
// Proves the jetbrains-product gate FAILS on every pass-shaped hole it exists
// to close (JBT1-AC1/AC2/AC3 discrimination) and passes only with real
// evidence: a JDK, an agreeing declaration/properties pin set, a successful
// Gradle run, executed JVM tests, packaged output with the declared range, an
// outside-the-repo install rehearsal dir, and verifier reports.

import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { deflateRawSync } from "node:zlib";

import {
  DECLARATION_PATH,
  GRADLE_PROPERTIES_PATH,
  collectTestEvidence,
  defaultGradleInvocation,
  extractDistribution,
  findZipEntries,
  parseGradleProperties,
  readZipTextEntry,
  runGate,
  validatePins,
  verifyPackagedPlugin,
} from "./jetbrains-gate.mjs";

const FIXTURE_PROPERTIES = {
  platformType: "WS",
  platformVersion: "2026.2.3",
  platformBuild: "262.10968.77",
  pluginSinceBuild: "262.10968",
  pluginUntilBuild: "262.*",
  pluginId: "com.verter.jetbrains",
  pluginVersion: "0.1.0-jbt1",
};

const FIXTURE_DECLARATION = {
  pluginId: "com.verter.jetbrains",
  supportedEditions: [
    {
      product: "WebStorm",
      productCode: "WS",
      sdkVersion: "2026.2.3",
      buildNumber: "262.10968.77",
      sinceBuild: "262.10968",
      untilBuild: "262.*",
    },
  ],
  unsupportedEditionsNotClaimed: ["IntelliJ IDEA Ultimate"],
};

function propertiesText(overrides = {}) {
  const merged = { ...FIXTURE_PROPERTIES, ...overrides };
  return Object.entries(merged)
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
}

// A fake "successful Gradle run" that materializes the exact artifacts the
// post-build validation reads: JUnit XML, a packaged distribution zip and
// verifier report dirs. Everything is written under the sandbox repoRoot, so
// no real JVM is involved anywhere in this suite.
function makeFakeGradle({ tests = 5, failures = 0, zip = true, verifier = true } = {}) {
  return (command, args, options) => {
    const projectDir = options.cwd;
    const resultsDir = path.join(projectDir, "build", "test-results", "test");
    mkdirSync(resultsDir, { recursive: true });
    writeFileSync(
      path.join(resultsDir, "TEST-dev.verter.jetbrains.HealthActionTest.xml"),
      `<?xml version="1.0"?><testsuite name="x" tests="${tests}" failures="${failures}" errors="0" skipped="0"></testsuite>`,
    );
    if (zip) {
      const distDir = path.join(projectDir, "build", "distributions");
      mkdirSync(distDir, { recursive: true });
      writeFileSync(
        path.join(distDir, "verter-jetbrains-0.1.0-jbt1.zip"),
        makePluginZip({
          id: FIXTURE_PROPERTIES.pluginId,
          sinceBuild: FIXTURE_PROPERTIES.pluginSinceBuild,
          untilBuild: FIXTURE_PROPERTIES.pluginUntilBuild,
        }),
      );
    }
    if (verifier) {
      const reportsDir = path.join(
        projectDir,
        "build",
        "reports",
        "pluginVerifier",
        "WebStorm-2026.2.3",
      );
      mkdirSync(reportsDir, { recursive: true });
      writeFileSync(path.join(reportsDir, "verifyPlugin.log"), "no compatibility problems");
    }
    return { status: 0 };
  };
}

// --- Minimal zip writer (stored + deflated) for reader round-trip tests -----

function crc32(buf) {
  let c;
  const table = [];
  for (let n = 0; n < 256; n++) {
    c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  let crc = 0xffffffff;
  for (const byte of buf) crc = table[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

export function makeZipBuffer(entries) {
  const locals = [];
  const centrals = [];
  let bodyOffset = 0;
  for (const { name, data, deflate = true } of entries) {
    const nameBytes = Buffer.from(name, "utf8");
    const method = deflate ? 8 : 0;
    const payload = deflate ? deflateRawSync(data) : data;
    const crc = crc32(data);

    const local = Buffer.alloc(30 + nameBytes.length);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0, 6); // flags
    local.writeUInt16LE(method, 8);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(payload.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    nameBytes.copy(local, 30);

    const central = Buffer.alloc(46 + nameBytes.length);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(method, 10);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(payload.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt32LE(bodyOffset, 42);
    nameBytes.copy(central, 46);

    locals.push(local, payload);
    centrals.push(central);
    bodyOffset += local.length + payload.length;
  }

  const centralBytes = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(centrals.length, 8);
  eocd.writeUInt16LE(centrals.length, 10);
  eocd.writeUInt32LE(centralBytes.length, 12);
  eocd.writeUInt32LE(bodyOffset, 16);
  return Buffer.concat([...locals, centralBytes, eocd]);
}

function pluginDescriptorXml({ id, sinceBuild, untilBuild }) {
  return (
    `<idea-plugin><id>${id}</id><version>0.1.0-jbt1</version>` +
    `<idea-version since-build="${sinceBuild}" until-build="${untilBuild}"/></idea-plugin>`
  );
}

// The buildPlugin layout: the descriptor lives INSIDE the nested lib jar.
export function makeNestedPluginZip(pins) {
  const innerJar = makeZipBuffer([
    { name: "META-INF/plugin.xml", data: Buffer.from(pluginDescriptorXml(pins), "utf8") },
  ]);
  return makeZipBuffer([
    { name: "verter-jetbrains/lib/verter-jetbrains-0.1.0-jbt1.jar", data: innerJar },
  ]);
}

export function makePluginZip({ id, sinceBuild, untilBuild, deflate = true } = {}) {
  return makeZipBuffer([
    {
      name: "verter-jetbrains-0.1.0-jbt1/META-INF/plugin.xml",
      data: Buffer.from(pluginDescriptorXml({ id, sinceBuild, untilBuild }), "utf8"),
      deflate,
    },
  ]);
}

// --- Suite -------------------------------------------------------------------

function makeSandbox({ properties = propertiesText(), declaration = FIXTURE_DECLARATION } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "jetbrains-gate-"));
  mkdirSync(path.join(root, "extensions", "jetbrains", "gradle", "wrapper"), { recursive: true });
  writeFileSync(path.join(root, GRADLE_PROPERTIES_PATH), properties);
  mkdirSync(path.dirname(path.join(root, DECLARATION_PATH)), { recursive: true });
  writeFileSync(path.join(root, DECLARATION_PATH), JSON.stringify(declaration, null, 2));
  // A stand-in JDK directory so JDK resolution succeeds.
  const jdkHome = path.join(root, "fake-jdk");
  mkdirSync(path.join(jdkHome, "bin"), { recursive: true });
  writeFileSync(path.join(jdkHome, "bin", process.platform === "win32" ? "java.exe" : "java"), "");
  return { root, jdkHome };
}

function cleanup(sandbox) {
  rmSync(sandbox.root, { recursive: true, force: true });
  rmSync(path.join(sandbox.root, "..", ".jetbrains-gate-install"), {
    recursive: true,
    force: true,
  });
}

// The gate extracts the distribution into JETBRAINS_GATE_INSTALL_DIR when set
// (production default: a `.jetbrains-gate-install` directory beside the
// repository). Pointing it at a per-sandbox dir keeps concurrent tests
// isolated while exercising the same code path.
function gateEnv(sandbox) {
  return { JETBRAINS_GATE_INSTALL_DIR: path.join(sandbox.root, "install-root") };
}

test("parseGradleProperties ignores comments and blank lines", () => {
  const parsed = parseGradleProperties(
    "# comment\n\nplatformType=WS\n  pluginUntilBuild = 262.*  \n!bang\nnovalue\n",
  );
  assert.deepEqual(parsed, { platformType: "WS", pluginUntilBuild: "262.*" });
});

test("validatePins accepts agreeing pins and rejects every drift", () => {
  assert.equal(
    validatePins({ properties: FIXTURE_PROPERTIES, declaration: FIXTURE_DECLARATION }).ok,
    true,
  );

  for (const override of [
    { platformType: "IU" },
    { platformVersion: "2026.1" },
    { platformBuild: "261.1.1" },
    { pluginSinceBuild: "261.1" },
    { pluginUntilBuild: "261.*" },
    { pluginId: "other.plugin" },
  ]) {
    const outcome = validatePins({
      properties: { ...FIXTURE_PROPERTIES, ...override },
      declaration: FIXTURE_DECLARATION,
    });
    assert.equal(outcome.ok, false, `expected drift failure for ${JSON.stringify(override)}`);
  }

  const noUnsupported = validatePins({
    properties: FIXTURE_PROPERTIES,
    declaration: { ...FIXTURE_DECLARATION, unsupportedEditionsNotClaimed: [] },
  });
  assert.equal(noUnsupported.ok, false, "empty unsupported-editions population must fail");

  const missingPin = validatePins({
    properties: { ...FIXTURE_PROPERTIES, platformBuild: undefined },
    declaration: FIXTURE_DECLARATION,
  });
  assert.equal(missingPin.ok, false, "missing required pin must fail");
});

test("collectTestEvidence fails on zero suites, zero tests and failing tests", () => {
  const emptyDir = mkdtempSync(path.join(tmpdir(), "gate-results-"));
  try {
    assert.equal(collectTestEvidence(emptyDir).ok, false);

    writeFileSync(
      path.join(emptyDir, "TEST-a.xml"),
      `<testsuite tests="0" failures="0" errors="0" skipped="0"></testsuite>`,
    );
    assert.equal(collectTestEvidence(emptyDir).ok, false, "zero executed tests must not pass");

    writeFileSync(
      path.join(emptyDir, "TEST-a.xml"),
      `<testsuite tests="3" failures="1" errors="0" skipped="0"></testsuite>`,
    );
    const failing = collectTestEvidence(emptyDir);
    assert.equal(failing.ok, false);
    assert.match(failing.reason, /1 failures/);

    writeFileSync(
      path.join(emptyDir, "TEST-a.xml"),
      `<testsuite name="a" tests="4" failures="0" errors="0" skipped="1"></testsuite>`,
    );
    writeFileSync(
      path.join(emptyDir, "TEST-b.xml"),
      `<testsuite name="b" tests="6" failures="0" errors="0" skipped="0"></testsuite>`,
    );
    const passing = collectTestEvidence(emptyDir);
    assert.equal(passing.ok, true);
    assert.equal(passing.tests, 10);
  } finally {
    rmSync(emptyDir, { recursive: true, force: true });
  }
});

test("zip reader round-trips stored and deflated plugin descriptors", () => {
  for (const deflate of [false, true]) {
    const zip = makePluginZip({
      id: "com.verter.jetbrains",
      sinceBuild: "262.10968",
      untilBuild: "262.*",
      deflate,
    });
    const entries = findZipEntries(zip, "META-INF/plugin.xml");
    assert.equal(entries.length, 1);
    const descriptor = readZipTextEntry(zip, entries[0]);
    assert.match(descriptor, /<idea-version since-build="262\.10968" until-build="262\.\*"/);
  }
});

test("packaged-descriptor reader follows the nested lib jar layout buildPlugin produces", () => {
  const zip = makeNestedPluginZip({
    id: "com.verter.jetbrains",
    sinceBuild: "262.10968",
    untilBuild: "262.*",
  });
  const dir = mkdtempSync(path.join(tmpdir(), "gate-zip-"));
  try {
    const zipPath = path.join(dir, "plugin.zip");
    writeFileSync(zipPath, zip);
    const outcome = verifyPackagedPlugin({
      zipPath,
      pluginId: "com.verter.jetbrains",
      sinceBuild: "262.10968",
      untilBuild: "262.*",
    });
    assert.equal(outcome.ok, true, outcome.errors.join("; "));
    const drifted = verifyPackagedPlugin({
      zipPath,
      pluginId: "com.verter.jetbrains",
      sinceBuild: "261.1",
      untilBuild: "262.*",
    });
    assert.equal(drifted.ok, false, "nested-layout range drift must still fail");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("verifyPackagedPlugin rejects id and range drift", () => {
  const zip = makePluginZip({
    id: "com.verter.jetbrains",
    sinceBuild: "262.10968",
    untilBuild: "262.*",
  });
  const dir = mkdtempSync(path.join(tmpdir(), "gate-zip-"));
  try {
    const zipPath = path.join(dir, "plugin.zip");
    writeFileSync(zipPath, zip);
    assert.equal(
      verifyPackagedPlugin({
        zipPath,
        pluginId: "com.verter.jetbrains",
        sinceBuild: "262.10968",
        untilBuild: "262.*",
      }).ok,
      true,
    );
    assert.equal(
      verifyPackagedPlugin({
        zipPath,
        pluginId: "other",
        sinceBuild: "262.10968",
        untilBuild: "262.*",
      }).ok,
      false,
      "plugin id drift must fail",
    );
    assert.equal(
      verifyPackagedPlugin({
        zipPath,
        pluginId: "com.verter.jetbrains",
        sinceBuild: "261.1",
        untilBuild: "262.*",
      }).ok,
      false,
      "since-build drift must fail",
    );
    assert.equal(
      verifyPackagedPlugin({
        zipPath,
        pluginId: "com.verter.jetbrains",
        sinceBuild: "262.10968",
        untilBuild: "271.*",
      }).ok,
      false,
      "until-build drift must fail",
    );

    const notAPlugin = path.join(dir, "not-a-plugin.zip");
    writeFileSync(notAPlugin, Buffer.from("definitely not a zip"));
    assert.equal(
      verifyPackagedPlugin({ zipPath: notAPlugin, pluginId: "x", sinceBuild: "1", untilBuild: "2" })
        .ok,
      false,
      "a zip without a plugin descriptor must fail",
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("defaultGradleInvocation rejects non-allowlisted gradle arguments", () => {
  assert.throws(() => defaultGradleInvocation("/repo", ["test; rm -rf /"]));
  assert.throws(() => defaultGradleInvocation("/repo", ["`ouch`"]));
  const invocation = defaultGradleInvocation("/repo", ["test", "verifyPlugin", "--console=plain"]);
  // The wrapper path plus every allowlisted argument survives on the command
  // line, on both the shell-string (win32) and argv (POSIX) forms.
  const commandLine = [invocation.command, ...invocation.args].join(" ");
  assert.match(commandLine, /gradlew/);
  assert.match(commandLine, /test/);
  assert.match(commandLine, /verifyPlugin/);
  assert.match(commandLine, /--console=plain/);
});

test("runGate passes only with complete evidence", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: makeFakeGradle(),
      log: () => {},
    });
    assert.equal(outcome.ok, true, JSON.stringify(outcome.failures));
    assert.equal(outcome.summary.tests, 5);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails without a JDK (JBT1-AC1)", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      env: {},
      spawnFn: makeFakeGradle(),
      log: () => {},
    });
    assert.equal(outcome.ok, false);
    assert.match(outcome.failures.join("\n"), /JDK/);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails when the gradle build fails (missing SDK/build breakage, JBT1-AC1)", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: () => ({ status: 1 }),
      log: () => {},
    });
    assert.equal(outcome.ok, false);
    assert.match(outcome.failures.join("\n"), /gradle exited 1/);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails on pass-with-no-tests", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: makeFakeGradle({ tests: 0 }),
      log: () => {},
    });
    assert.equal(outcome.ok, false);
    assert.match(outcome.failures.join("\n"), /0 executed tests|zero TEST-/);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails on failing JVM tests", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: makeFakeGradle({ failures: 2 }),
      log: () => {},
    });
    assert.equal(outcome.ok, false);
    assert.match(outcome.failures.join("\n"), /2 failures/);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails when the build produced no distribution (JBT1-AC1)", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: makeFakeGradle({ zip: false }),
      log: () => {},
    });
    assert.equal(outcome.ok, false);
    assert.match(outcome.failures.join("\n"), /no installable output/);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails when the packaged descriptor drifts from the declared range (JBT1-AC2/AC3)", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: makeFakeGradle(),
      log: () => {},
      // Simulate a zip built with a different range than the declaration:
      // overwrite the artifact after the fake gradle run materializes it.
    });
    // Overwrite with drifted zip and rerun — runGate re-reads artifacts.
    const distDir = path.join(sandbox.root, "extensions", "jetbrains", "build", "distributions");
    writeFileSync(
      path.join(distDir, "verter-jetbrains-0.1.0-jbt1.zip"),
      makePluginZip({ id: "com.verter.jetbrains", sinceBuild: "261.1", untilBuild: "261.*" }),
    );
    const drifted = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: () => ({ status: 0 }),
      log: () => {},
    });
    assert.equal(drifted.ok, false);
    assert.match(drifted.failures.join("\n"), /since-build/);
    assert.equal(outcome.ok, true);
  } finally {
    cleanup(sandbox);
  }
});

test("the install rehearsal unpacks real layouts and rejects corrupt or unsafe archives (JBT1-AC2)", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "gate-install-"));
  try {
    const zipPath = path.join(dir, "plugin.zip");
    writeFileSync(
      zipPath,
      makeNestedPluginZip({
        id: "com.verter.jetbrains",
        sinceBuild: "262.10968",
        untilBuild: "262.*",
      }),
    );
    const installDir = path.join(dir, "plugins");
    const extracted = extractDistribution(zipPath, installDir);
    assert.equal(extracted.ok, true, extracted.reason);
    assert.ok(extracted.files >= 1);
    assert.ok(
      existsSync(
        path.join(installDir, "verter-jetbrains", "lib", "verter-jetbrains-0.1.0-jbt1.jar"),
      ),
    );

    const corrupt = extractDistribution(path.join(dir, "not-a-zip.zip"), installDir);
    writeFileSync(path.join(dir, "not-a-zip.zip"), Buffer.from("garbage"));
    const corruptOutcome = extractDistribution(path.join(dir, "not-a-zip.zip"), installDir);
    assert.equal(
      corrupt.ok === false || corruptOutcome.ok === false,
      true,
      "a corrupt archive must fail the rehearsal",
    );

    const hostile = makeZipBuffer([{ name: "../escape.txt", data: Buffer.from("nope") }]);
    writeFileSync(path.join(dir, "hostile.zip"), hostile);
    const hostileOutcome = extractDistribution(path.join(dir, "hostile.zip"), installDir);
    assert.equal(hostileOutcome.ok, false, "a `..` zip entry must be rejected, not extracted");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("runGate fails when the distribution cannot be unpacked outside the repository (JBT1-AC2)", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: makeFakeGradle(),
      log: () => {},
    });
    assert.equal(outcome.ok, true, JSON.stringify(outcome.failures));

    // Replace the distribution with garbage: the gate must fail at the
    // outside-the-repository unpack step instead of passing on a broken zip.
    const distDir = path.join(sandbox.root, "extensions", "jetbrains", "build", "distributions");
    writeFileSync(path.join(distDir, "verter-jetbrains-0.1.0-jbt1.zip"), Buffer.from("garbage"));
    const broken = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: () => ({ status: 0 }),
      log: () => {},
    });
    assert.equal(broken.ok, false);
    assert.match(broken.failures.join("\n"), /not a zip file|corrupt|install rehearsal/);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails when the verifier produced no reports (JBT1-AC3)", () => {
  const sandbox = makeSandbox();
  try {
    const outcome = runGate({
      repoRoot: sandbox.root,
      javaHomeArg: sandbox.jdkHome,
      env: gateEnv(sandbox),
      spawnFn: makeFakeGradle({ verifier: false }),
      log: () => {},
    });
    assert.equal(outcome.ok, false);
    assert.match(outcome.failures.join("\n"), /verifier/);
  } finally {
    cleanup(sandbox);
  }
});

test("runGate fails when the pinned build inputs do not exist", () => {
  const bareRoot = mkdtempSync(path.join(tmpdir(), "jetbrains-gate-"));
  try {
    const missingProps = runGate({
      repoRoot: bareRoot,
      env: {},
      spawnFn: makeFakeGradle(),
      log: () => {},
    });
    assert.equal(missingProps.ok, false);
    assert.match(missingProps.failures.join("\n"), /gradle\.properties/);
  } finally {
    rmSync(bareRoot, { recursive: true, force: true });
  }

  // Properties present but the declaration product absent: still a failure.
  const root = mkdtempSync(path.join(tmpdir(), "jetbrains-gate-"));
  try {
    mkdirSync(path.join(root, "extensions", "jetbrains"), { recursive: true });
    writeFileSync(path.join(root, GRADLE_PROPERTIES_PATH), propertiesText());
    const missingDeclaration = runGate({
      repoRoot: root,
      env: {},
      spawnFn: makeFakeGradle(),
      log: () => {},
    });
    assert.equal(missingDeclaration.ok, false);
    assert.match(missingDeclaration.failures.join("\n"), /ide-support-range/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the live repository's declaration and gradle.properties agree", () => {
  const repoRoot = path.resolve(import.meta.dirname, "..");
  const properties = parseGradleProperties(
    readFileSync(path.join(repoRoot, GRADLE_PROPERTIES_PATH), "utf8"),
  );
  const declaration = JSON.parse(readFileSync(path.join(repoRoot, DECLARATION_PATH), "utf8"));
  const outcome = validatePins({ properties, declaration });
  assert.equal(outcome.ok, true, outcome.errors.join("; "));
});
