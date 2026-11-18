/**
 * Externally processed block content: the envelope's stamps and hashes, the
 * wire form, the bundler-facing dependency registration, the refusal of stale
 * stamps, and the one compile shared by pre-compile and transform.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import type {
  HostBlockOverrideRefusalCode,
  HostPreprocessorRequest,
  HostUpdateResult,
} from "@verter/native";
import { copyCapturedBlockContentEcho } from "../preprocessor";
import { canonicalizeId, hashBlockContent } from "@verter/native";
import type { HostCanonicalId } from "../host-session/identity";
import { BundlerHostSession } from "../host-session/session";
import type { SessionBindingInput } from "../host-session/session";
import { unpluginFactory } from "../../index";
import { PipelineRefusalError } from "./admission";
import { CarrierPipeline, type CarrierDemandKey, type DemandContext } from "./carrier-demand";
import { createPipelineCounters } from "./counters";
import { applyProcessedContent, processBlock, toBlockOverride } from "./processed-content";

vi.mock("pug", () => ({
  render: (source: string) => {
    if (source.includes("explode")) throw new Error("boom");
    return `<p>${source}</p>`;
  },
}));

const SCSS_SFC = `<template><p class="a">x</p></template>
<style lang="scss">.a { .b { color: red; } }</style>`;

const INDENTED_SFC = `<template><p class="a">x</p></template>
<style lang="sass">
.a
  color: red
</style>`;

const IMPORTING_SFC = `<template><p class="a">x</p></template>
<style lang="scss">
@use "./vars";
.a { color: vars.$colour; }
</style>`;

const PUG_SFC = `<template lang="pug">explode</template>`;

let root: string;
const sessions: BundlerHostSession[] = [];

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "verter-processed-content-")).replace(/\\/g, "/");
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const session of sessions.splice(0)) session.close();
  rmSync(root, { recursive: true, force: true });
});

function open(overrides: Partial<SessionBindingInput> = {}): BundlerHostSession {
  const session = BundlerHostSession.bind({
    adapter: "rollup",
    root,
    mode: "dev",
    defaultTarget: "client",
    frameworkSelection: "vue",
    options: {},
    hostCssPipeline: false,
    hostStripsTypeScript: false,
    hostConfig: null,
    nuxtFileRoles: false,
    ...overrides,
  });
  sessions.push(session);
  return session;
}

function contextOf(
  session: BundlerHostSession,
  addWatchFile: ((path: string) => void) | null = null,
): DemandContext {
  return { invocation: session.nextInvocation(), resolve: null, warn: () => {}, addWatchFile };
}

function writeCarrier(name: string, source: string): string {
  const path = `${root}/${name}`;
  writeFileSync(path, source);
  return path;
}

function register(session: BundlerHostSession, path: string, source: string): HostUpdateResult {
  return session.host().upsert({ inputId: path, source });
}

function firstRequest(upsert: HostUpdateResult): HostPreprocessorRequest {
  const request = upsert.preprocessorRequests?.[0];
  if (!request) throw new Error("the upsert asked for no processed content");
  return request;
}

function keyOf(session: BundlerHostSession, path: string): CarrierDemandKey {
  return {
    carrier: canonicalizeId(path) as HostCanonicalId,
    rawPath: path,
    graph: session.graphFor({}),
  };
}

describe("processed content envelope", () => {
  it("returns the host's stamps unchanged", async () => {
    const session = open();
    const path = writeCarrier("Stamps.vue", SCSS_SFC);
    const request = firstRequest(register(session, path, SCSS_SFC));

    const envelope = await processBlock(request, path, session.binding);

    expect(envelope).not.toBeNull();
    expect(envelope!.echo).toEqual(copyCapturedBlockContentEcho(request));
    expect(envelope!.sourceSpaceToken).toBe(request.sourceSpaceToken);
    expect(envelope!.inputHash).toBe(request.contentHash);
    expect(envelope!.stage).toBe("style:scss");
    expect(envelope!.processorIdentity).toBe("sass");
    expect(envelope!.processorVersion).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("carries the code, the map and their hashes on the wire", async () => {
    const session = open();
    const path = writeCarrier("Wire.vue", SCSS_SFC);
    const request = firstRequest(register(session, path, SCSS_SFC));
    const envelope = (await processBlock(request, path, session.binding))!;

    const wire = toBlockOverride(envelope);

    expect(wire).toEqual({
      ...copyCapturedBlockContentEcho(request),
      sourceSpaceToken: request.sourceSpaceToken,
      code: envelope.code,
      codeHash: envelope.codeHash,
      sourceMap: envelope.sourceMap ?? undefined,
      sourceMapHash: envelope.sourceMapHash ?? undefined,
      dependencies: [...envelope.dependencies],
      diagnostics: [],
      processorIdentity: "sass",
      processorVersion: envelope.processorVersion,
      configFingerprint: envelope.configFingerprint,
      suppliedProvenance: "@verter/unplugin",
    });
    expect(envelope.codeHash).toBe(hashBlockContent(envelope.code));
    expect(envelope.codeHash).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(envelope.sourceMap).not.toBeNull();
    expect(envelope.sourceMapHash).toMatch(/^sha256:[0-9a-f]{64}$/);
  });

  it("fingerprints different tool options differently", async () => {
    const session = open();
    const scssPath = writeCarrier("Scss.vue", SCSS_SFC);
    const sassPath = writeCarrier("Indented.vue", INDENTED_SFC);
    const scss = (await processBlock(
      firstRequest(register(session, scssPath, SCSS_SFC)),
      scssPath,
      session.binding,
    ))!;
    const indented = (await processBlock(
      firstRequest(register(session, sassPath, INDENTED_SFC)),
      sassPath,
      session.binding,
    ))!;

    expect(indented.configFingerprint).not.toBe(scss.configFingerprint);
  });

  it("leaves style requests to a host that has a CSS pipeline", async () => {
    const session = open({ hostCssPipeline: true });
    const path = writeCarrier("Hosted.vue", SCSS_SFC);
    const upsert = register(session, path, SCSS_SFC);
    expect(upsert.preprocessorRequests?.map((request) => request.contentClass)).toContain("style");
    const apply = vi.spyOn(session.host(), "applyBlockOverrides");

    const envelopes = await applyProcessedContent(
      session.host(),
      path,
      upsert,
      session.binding,
      contextOf(session),
      createPipelineCounters(),
    );

    expect(envelopes).toEqual([]);
    expect(apply).not.toHaveBeenCalled();
  });

  it("registers a stylesheet's imported files with the bundler", async () => {
    const session = open();
    const path = writeCarrier("Imports.vue", IMPORTING_SFC);
    writeCarrier("_vars.scss", "$colour: red;\n");
    const watched: string[] = [];
    const counters = createPipelineCounters();

    await applyProcessedContent(
      session.host(),
      path,
      register(session, path, IMPORTING_SFC),
      session.binding,
      contextOf(session, (file) => watched.push(file)),
      counters,
    );

    expect(watched).toEqual([fileURLToPath(pathToFileURL(`${root}/_vars.scss`))]);
    expect(counters.preparedInputBytes).toBeGreaterThan(0);
  });

  it("builds the same CSS after an imported stylesheet changes as a fresh session", async () => {
    const path = writeCarrier("Edited.vue", IMPORTING_SFC);
    const vars = writeCarrier("_vars.scss", "$colour: red;\n");

    const session = open();
    const pipeline = new CarrierPipeline(session);
    const key = keyOf(session, path);
    const first = await pipeline.demand(
      key,
      { kind: "bundler", code: IMPORTING_SFC },
      contextOf(session),
    );
    expect(first.compiledStyles[0].code).toContain("red");

    writeFileSync(vars, "$colour: blue;\n");
    pipeline.invalidate(vars);
    const edited = await pipeline.demand(
      key,
      { kind: "bundler", code: IMPORTING_SFC },
      contextOf(session),
    );

    const fresh = open();
    const rebuilt = await new CarrierPipeline(fresh).demand(
      keyOf(fresh, path),
      { kind: "bundler", code: IMPORTING_SFC },
      contextOf(fresh),
    );
    expect(edited.compiledStyles[0].code).toContain("blue");
    expect(edited.compiledStyles[0].code).toBe(rebuilt.compiledStyles[0].code);
  });

  it("a stylesheet that imports another keeps a map naming both files", async () => {
    const session = open();
    const path = writeCarrier("Mapped.vue", IMPORTING_SFC);
    writeCarrier("_vars.scss", "$colour: red;\n");
    const pipeline = new CarrierPipeline(session);

    const record = await pipeline.demand(
      keyOf(session, path),
      { kind: "bundler", code: IMPORTING_SFC },
      contextOf(session),
    );

    const map = JSON.parse(record.compiledStyles[0].map ?? "null") as { sources: string[] };
    expect(map.sources.some((source) => source.endsWith("Mapped.vue"))).toBe(true);
    expect(map.sources.some((source) => source.endsWith("_vars.scss"))).toBe(true);
  });

  it("fails the build with the tool's message when a template preprocessor throws", async () => {
    const session = open();
    const path = writeCarrier("Broken.vue", PUG_SFC);
    const upsert = register(session, path, PUG_SFC);

    const failure = await applyProcessedContent(
      session.host(),
      path,
      upsert,
      session.binding,
      contextOf(session),
      createPipelineCounters(),
    ).then(
      () => null,
      (error: unknown) => error,
    );

    expect(failure).toBeInstanceOf(Error);
    expect((failure as Error).message).toContain("boom");
    expect((failure as Error).message).toContain(path);
    expect((failure as Error as Error & { cause?: unknown }).cause).toBeInstanceOf(Error);
  });

  it("refuses an override stamped for an older revision and publishes nothing", async () => {
    const session = open();
    const path = writeCarrier("Stale.vue", SCSS_SFC);
    const older = register(session, path, SCSS_SFC);
    register(session, path, SCSS_SFC.replace("color: red", "color: green"));

    const apply = vi.spyOn(session.host(), "applyBlockOverrides");
    const failure = await applyProcessedContent(
      session.host(),
      path,
      older,
      session.binding,
      contextOf(session),
      createPipelineCounters(),
    ).then(
      () => null,
      (error: unknown) => error,
    );

    expect(failure).toBeInstanceOf(PipelineRefusalError);
    expect((failure as PipelineRefusalError).refusal.kind).toBe("stale");
    // The refusal comes from the host's typed code, never from its message text.
    const thrown = apply.mock.results[0];
    expect(thrown?.type).toBe("throw");
    const code: HostBlockOverrideRefusalCode = (
      thrown?.value as { code: HostBlockOverrideRefusalCode }
    ).code;
    expect(code).toBe("BlockContentStale");
    expect(
      session.catalog.record(canonicalizeId(path) as HostCanonicalId, session.graphFor({})),
    ).toBeUndefined();
  });
});

describe("pre-compile admission", () => {
  it("shares one compile between pre-compile and the transform of the same bytes", async () => {
    const files = ["One.vue", "Two.vue"].map((name) => ({
      path: writeCarrier(name, SCSS_SFC.replace("x", name)),
      source: SCSS_SFC.replace("x", name),
    }));
    vi.spyOn(process, "cwd").mockReturnValue(root);
    vi.stubEnv("VERTER_TIMING", "1");
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    const plugin = unpluginFactory({ preCompile: true }, {
      framework: "rollup",
      versions: { unplugin: "0.0.0", rollup: "0.0.0" },
    } as never) as unknown as {
      buildStart(): Promise<void>;
      transform(code: string, id: string): Promise<unknown>;
      buildEnd(): Promise<void>;
      closeBundle(): Promise<void>;
    };

    await plugin.buildStart();
    for (const { path, source } of files) await plugin.transform(source, path);
    await plugin.buildEnd();
    await plugin.closeBundle();
    vi.unstubAllEnvs();

    const summary = log.mock.calls
      .map((call) => String(call[0]))
      .find((line) => line.includes("compiles="));
    expect(summary).toBeDefined();
    expect(summary).toContain(`compiles=${files.length} `);
  });
});
