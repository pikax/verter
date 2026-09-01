import { expect } from "chai";
import * as vscode from "vscode";

import { attestE2eTypeProviderLog } from "../../src/e2eProviderAttestation";
import {
  ensureFixtureWarm,
  FIXTURE_NAME,
  isLspReady,
  readTestLog,
  TYPE_PROVIDER,
} from "../helpers";

suite(`Projectless provider-off contract [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    await ensureFixtureWarm();
  });

  test("projectless.extension-active", function () {
    const extension = vscode.extensions.getExtension("verter.verter-vscode");
    expect(extension, "Verter extension must be installed").to.exist;
    expect(extension!.isActive, "Verter extension must activate for a projectless carrier").to.be
      .true;
  });

  test("projectless.lsp-ready", function () {
    expect(isLspReady(), "projectless provider-off route must reach LSP ready").to.be.true;
    expect(readTestLog()).to.include("Verter ready");
  });

  test("projectless.provider-none", function () {
    expect(TYPE_PROVIDER, "projectless contract must run only on the explicit off route").to.equal(
      "off",
    );
    const attestation = attestE2eTypeProviderLog(readTestLog(), "off");
    expect(attestation.publicKind).to.equal("none");
    expect(attestation.route).to.equal("off");
  });

  test("projectless.no-server-crash", function () {
    const log = readTestLog();
    expect(log).to.not.include("panicked at");
    expect(log).to.not.include("thread 'main' panicked");
  });
});
