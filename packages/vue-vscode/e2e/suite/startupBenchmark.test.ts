/**
 * @ai-generated - Startup benchmark coverage for cold-start timing markers.
 */
import { expect } from "chai";
import * as vscode from "vscode";
import {
  ensureFixtureWarm,
  openVueFile,
  parseStartupTiming,
  FIXTURE_NAME,
  getAppVuePath,
} from "../helpers";
import { getTimer } from "../timer";

const startupSuite = process.env.VERTER_E2E_STARTUP_BENCHMARK === "1" ? suite : suite.skip;

startupSuite(`Startup Benchmark [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  suiteSetup(async function () {
    await openVueFile(getAppVuePath());
    await ensureFixtureWarm();
  });

  test("captures typed-completion startup markers and timing report fields", async function () {
    const editor = vscode.window.activeTextEditor;
    expect(editor, "A Vue editor should be active").to.exist;
    expect(editor?.document.languageId, "Benchmark should target a Vue file").to.equal("vue");

    const timing = await waitForStartupTiming();
    const typedCompletion = timing as typeof timing & {
      firstTypedCompletionLabel?: string;
      firstTypedCompletionKind?: string;
    };

    expect(timing.activationStartMs, "activation_start marker should be present").to.be.a("number");
    expect(timing.typeProviderStartedMs, "type_provider_started marker should be present").to.be.a(
      "number",
    );
    expect(timing.lspReadyMs, "ready marker should be present").to.be.a("number");
    expect(
      timing.firstTypedCompletionMs,
      "first_typed_completion marker should be present",
    ).to.be.a("number");
    expect(
      timing.activationToTypeProviderStartedMs,
      "activationToTypeProviderStartedMs should be present",
    ).to.be.a("number");
    expect(
      timing.typeProviderStartedToFirstTypedCompletionMs,
      "typeProviderStartedToFirstTypedCompletionMs should be present",
    ).to.be.a("number");
    expect(
      timing.typeProviderStartedToReadyMs,
      "typeProviderStartedToReadyMs should be present",
    ).to.be.a("number");
    expect(timing.providerKind, "provider kind should be detected").to.equal("editor-tsserver");
    expect(
      typedCompletion.firstTypedCompletionLabel,
      "startup benchmark should target the props.title member probe",
    ).to.equal("title");
    expect(
      typedCompletion.firstTypedCompletionKind,
      "startup benchmark should record a provider-backed member completion kind",
    ).to.be.oneOf(["Property", "Field"]);

    expect(
      timing.firstTypedCompletionMs!,
      "typed completion should happen after activation starts",
    ).to.be.greaterThan(timing.activationStartMs!);
    expect(
      timing.firstTypedCompletionMs!,
      "typed completion should not happen before the provider starts",
    ).to.be.greaterThan(timing.typeProviderStartedMs!);

    getTimer().recordStartupTiming({
      activationStartMs: timing.activationStartMs!,
      typeProviderStartedMs: timing.typeProviderStartedMs!,
      lspReadyMs: timing.lspReadyMs!,
      firstTypedCompletionMs: timing.firstTypedCompletionMs!,
      firstDiagnosticMs: timing.firstDiagnosticMs ?? null,
      providerKind: timing.providerKind!,
    });

    const report = getTimer().getReport();
    expect(report.startup.activationStartMs).to.equal(timing.activationStartMs);
    expect(report.startup.typeProviderStartedMs).to.equal(timing.typeProviderStartedMs);
    expect(report.startup.lspReadyMs).to.equal(timing.lspReadyMs);
    expect(report.startup.firstTypedCompletionMs).to.equal(timing.firstTypedCompletionMs);
    expect(report.startup.activationToTypeProviderStartedMs).to.equal(
      timing.activationToTypeProviderStartedMs,
    );
    expect(report.startup.typeProviderStartedToFirstTypedCompletionMs).to.equal(
      timing.typeProviderStartedToFirstTypedCompletionMs,
    );
    expect(report.startup.typeProviderStartedToReadyMs).to.equal(
      timing.typeProviderStartedToReadyMs,
    );
    expect(report.startup.providerKind).to.equal("editor-tsserver");
    expect(report.startup.typeProvider).to.not.equal("verter-only");
  });
});

async function waitForStartupTiming(timeoutMs = 20_000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const timing = parseStartupTiming();
    if (timing.firstTypedCompletionMs !== undefined) {
      return timing;
    }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }

  return parseStartupTiming();
}
