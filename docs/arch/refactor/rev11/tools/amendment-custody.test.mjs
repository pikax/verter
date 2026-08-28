/** @ai-generated - Receipt-backed amendment custody and CLI regression proofs. */
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import * as lib from "./lib.mjs";

function exec(command, args, cwd) {
  return childProcess
    .execFileSync(command, args, {
      cwd,
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
      timeout: 120_000,
      killSignal: "SIGKILL",
    })
    .trim();
}

function git(cwd, ...args) {
  return exec("git", args, cwd);
}

function cli(env, args) {
  return exec(
    process.execPath,
    [env.programctl, ...args, "--runtime-root", env.runtimeRoot],
    env.repo,
  );
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function withTempDir(prefix, callback) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  try {
    return callback(directory);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

function prepare(temp) {
  const source = git(lib.PACKAGE_ROOT, "rev-parse", "--show-toplevel");
  const repo = path.join(temp, "repo");
  exec("git", ["clone", "--shared", source, repo], temp);
  git(repo, "config", "user.email", "trusted-local@example.invalid");
  git(repo, "config", "user.name", "Trusted Local Test");
  git(repo, "switch", "-C", "program/architecture-lock", "HEAD");
  const packageRoot = path.join(repo, "docs/arch/refactor/rev11");
  fs.rmSync(packageRoot, { recursive: true, force: true });
  fs.cpSync(lib.PACKAGE_ROOT, packageRoot, { recursive: true });
  git(repo, "add", "-A", "docs/arch/refactor/rev11");
  if (git(repo, "status", "--porcelain=v1", "--", "docs/arch/refactor/rev11"))
    git(repo, "commit", "-m", "test: install exact authority package");
  return {
    repo,
    packageRoot,
    runtimeRoot: path.join(temp, "external runtime"),
    programctl: path.join(packageRoot, "tools/programctl.mjs"),
  };
}

function installRatificationSlot(packageRoot) {
  const receipt = Buffer.from("maintainer ratification for the successor authority amendment\n");
  const receiptDigest = sha256(receipt);
  const receiptRelative = "authority/state/ratification-receipts/successor-amendment.txt";
  const receiptFile = path.join(packageRoot, receiptRelative);
  fs.mkdirSync(path.dirname(receiptFile), { recursive: true });
  fs.writeFileSync(receiptFile, receipt);
  const ledger = [
    "schema = 2",
    "",
    "[[slot]]",
    'purpose = "authority-amendment"',
    'ratified_by = "maintainer"',
    `receipt_path = "${receiptRelative}"`,
    `receipt_sha256 = "${receiptDigest}"`,
    "",
  ].join("\n");
  fs.writeFileSync(path.join(packageRoot, "authority/state/trusted-ratifications.toml"), ledger);
  return { ledger, receiptDigest, receiptFile };
}

function existingAmendmentSlot(packageRoot) {
  const ledger = lib.readToml(path.join(packageRoot, "authority/state/trusted-ratifications.toml"));
  return (ledger.slot || []).find((slot) => slot.purpose === "authority-amendment");
}

test("static custody admits only exact receipt-backed authority amendment slots", () => {
  withTempDir("rev11-amendment-slot-", (directory) => {
    fs.cpSync(lib.PACKAGE_ROOT, directory, { recursive: true });
    const { ledger, receiptFile } = installRatificationSlot(directory);
    const ledgerFile = path.join(directory, "authority/state/trusted-ratifications.toml");
    const validErrors = lib.validateAuthority(lib.loadAuthority(directory), {
      strict: true,
      checkGenerated: false,
      checkAmendments: false,
    });
    assert.doesNotMatch(
      validErrors.join("\n"),
      /authorization\/ratification slot|ratification receipt/i,
    );

    fs.writeFileSync(
      ledgerFile,
      ledger.replace('purpose = "authority-amendment"', 'purpose = "unknown-purpose"'),
    );
    assert.match(
      lib
        .validateAuthority(lib.loadAuthority(directory), {
          strict: true,
          checkGenerated: false,
          checkAmendments: false,
        })
        .join("\n"),
      /unanchored static authorization\/ratification slot/i,
    );

    fs.writeFileSync(ledgerFile, ledger);
    fs.writeFileSync(receiptFile, "tampered\n");
    assert.match(
      lib
        .validateAuthority(lib.loadAuthority(directory), {
          strict: true,
          checkGenerated: false,
          checkAmendments: false,
        })
        .join("\n"),
      /ratification receipt digest mismatch/i,
    );
  });
});

test("programctl creates an externally ratified authority amendment", { timeout: 240_000 }, () => {
  withTempDir("rev11-amendment-cli-", (temp) => {
    const env = prepare(temp);
    let slot = existingAmendmentSlot(env.packageRoot);
    if (!slot) {
      const { receiptDigest } = installRatificationSlot(env.packageRoot);
      slot = { ratified_by: "maintainer", receipt_sha256: receiptDigest };
    }
    const beforeRoot = path.join(temp, "before-authority");
    fs.cpSync(env.packageRoot, beforeRoot, { recursive: true });
    if (
      lib.readToml(path.join(beforeRoot, "authority/state/authority-lock.toml")).generation === 0
    ) {
      const beforeDigest = lib.computeAuthorityDigest(beforeRoot);
      const baselineLock = [
        "schema = 2",
        `baseline_authority_sha256 = "${beforeDigest}"`,
        `current_authority_sha256 = "${beforeDigest}"`,
        "generation = 0",
        'last_amendment_id = ""',
        'last_amendment_sha256 = ""',
        "",
      ].join("\n");
      fs.writeFileSync(path.join(beforeRoot, "authority/state/authority-lock.toml"), baselineLock);
      fs.writeFileSync(
        path.join(env.packageRoot, "authority/state/authority-lock.toml"),
        baselineLock,
      );
    }
    fs.writeFileSync(
      path.join(env.packageRoot, "sources/cli-amendment-test.md"),
      "# CLI amendment test\n",
    );
    const created = JSON.parse(
      cli(env, [
        "amendment-create",
        "AMD-CLI-TEST",
        "--before-root",
        beforeRoot,
        "--ratified-by",
        slot.ratified_by,
        "--ratification-receipt",
        slot.receipt_sha256,
      ]),
    );
    assert.equal(created.amendment, "AMD-CLI-TEST");
    assert.match(created.digest, /^[0-9a-f]{64}$/u);
    assert.match(cli(env, ["amendment-check"]), /PASS/);

    const orphanWriteLock = path.join(
      env.packageRoot,
      "authority/state/amendments/.amendment-write.lock",
    );
    fs.mkdirSync(orphanWriteLock);
    assert.throws(
      () => cli(env, ["amendment-check"]),
      /amendment write is incomplete or in progress/,
    );
  });
});
