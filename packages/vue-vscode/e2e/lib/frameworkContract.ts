import { strict as assert } from "node:assert";
import * as path from "node:path";
import * as vscode from "vscode";

import { ensureTypeProviderSynced, sleep, waitForDiagnosticsSettled } from "../helpers";
import type {
  ContractAnchor,
  FrameworkContractDescriptor,
  LocalCarrierCase,
} from "../frameworks/types";
import { frameworkContractId, type FrameworkContractCapability } from "./frameworkContractManifest";
import { VIRTUAL_CARRIER_PATTERN } from "./virtualCarrier";

function workspaceRoot(): string {
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  assert.ok(root, "framework contract requires one workspace root");
  return root;
}

function absoluteFile(relative: string): string {
  return path.normalize(path.join(workspaceRoot(), relative));
}

async function openWorkspaceFile(relative: string): Promise<vscode.TextDocument> {
  const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(absoluteFile(relative)));
  await vscode.window.showTextDocument(doc);
  return doc;
}

function anchorOffset(doc: vscode.TextDocument, anchor: ContractAnchor): number {
  assert.equal(
    path.normalize(doc.uri.fsPath),
    absoluteFile(anchor.file),
    `anchor opened wrong file`,
  );
  const occurrence = anchor.occurrence ?? 0;
  let offset = -1;
  for (let index = 0; index <= occurrence; index++) {
    offset = doc.getText().indexOf(anchor.token, offset + 1);
    assert.notEqual(offset, -1, `missing anchor ${anchor.file}#${anchor.token}[${occurrence}]`);
  }
  return offset;
}

function anchorPosition(doc: vscode.TextDocument, anchor: ContractAnchor): vscode.Position {
  return doc.positionAt(anchorOffset(doc, anchor) + Math.min(1, anchor.token.length));
}

function locationPath(location: vscode.Location): string {
  return path.normalize(location.uri.fsPath);
}

function assertNoVirtualLocations(locations: readonly vscode.Location[], feature: string): void {
  const leaked = locations.filter((location) => VIRTUAL_CARRIER_PATTERN.test(location.uri.fsPath));
  assert.deepEqual(
    leaked.map((location) => location.uri.fsPath),
    [],
    `${feature} exposed generated carrier paths`,
  );
}

function toLocations(
  values: readonly (vscode.Location | vscode.LocationLink)[] | undefined,
): vscode.Location[] {
  return (values ?? []).map((value) =>
    "targetUri" in value
      ? new vscode.Location(value.targetUri, value.targetSelectionRange ?? value.targetRange)
      : value,
  );
}

async function poll<T>(
  label: string,
  request: () => Promise<T>,
  ready: (value: T) => boolean,
  timeoutMs = 10_000,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  let latest = await request();
  while (Date.now() < deadline) {
    if (ready(latest)) return latest;
    await sleep(150);
    latest = await request();
  }
  throw new Error(`${label} did not become semantically ready within ${timeoutMs}ms`);
}

async function definitionsAt(anchor: ContractAnchor): Promise<vscode.Location[]> {
  const doc = await openWorkspaceFile(anchor.file);
  const position = anchorPosition(doc, anchor);
  let locations: vscode.Location[];
  try {
    locations = await poll(
      `definition ${anchor.file}#${anchor.token}`,
      async () =>
        toLocations(
          await vscode.commands.executeCommand<Array<vscode.Location | vscode.LocationLink>>(
            "vscode.executeDefinitionProvider",
            doc.uri,
            position,
          ),
        ),
      (result) => result.length > 0,
    );
  } catch (error) {
    const diagnostics = vscode.languages
      .getDiagnostics(doc.uri)
      .map((diagnostic) => `${String(diagnostic.code)}:${diagnostic.message}`);
    const hovers =
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        doc.uri,
        position,
      )) ?? [];
    const hoverText = hovers
      .flatMap((hover) => hover.contents)
      .map((content) => (typeof content === "string" ? content : content.value))
      .join("\n");
    throw new Error(
      `${String(error)}; diagnostics=${JSON.stringify(diagnostics)}; hover=${JSON.stringify(hoverText)}`,
    );
  }
  assertNoVirtualLocations(locations, "definition");
  return locations;
}

async function assertDefinitionTargetsAnchor(
  source: ContractAnchor,
  target: ContractAnchor,
): Promise<void> {
  const locations = await definitionsAt(source);
  const targetDoc = await vscode.workspace.openTextDocument(
    vscode.Uri.file(absoluteFile(target.file)),
  );
  const expectedOffset = anchorOffset(targetDoc, target);
  assert.ok(
    locations.some((location) => {
      if (locationPath(location) !== absoluteFile(target.file)) return false;
      const start = targetDoc.offsetAt(location.range.start);
      const end = targetDoc.offsetAt(location.range.end);
      return start <= expectedOffset && expectedOffset <= Math.max(start, end);
    }),
    `definition from ${source.file}#${source.token} did not reach ${target.file}#${target.token}; ` +
      `got ${locations.map((location) => `${location.uri.fsPath}:${location.range.start.line + 1}`).join(", ")}`,
  );
}

async function assertDefinitionTargetsFile(
  source: ContractAnchor,
  targetFile: string,
): Promise<void> {
  const locations = await definitionsAt(source);
  assert.ok(
    locations.some((location) => locationPath(location) === absoluteFile(targetFile)),
    `definition from ${source.file}#${source.token} did not reach authored ${targetFile}; ` +
      `got ${locations.map((location) => location.uri.fsPath).join(", ")}`,
  );
}

function expectedAnchorKeys(anchors: readonly ContractAnchor[]): string[] {
  return anchors
    .map((anchor) => `${absoluteFile(anchor.file)}:${anchor.occurrence ?? 0}:${anchor.token}`)
    .sort();
}

async function locationKeys(
  locations: readonly vscode.Location[],
  token: string,
): Promise<string[]> {
  const seen = new Map<string, number>();
  const keys: string[] = [];
  for (const location of locations) {
    const file = locationPath(location);
    const doc = await vscode.workspace.openTextDocument(location.uri);
    const start = doc.offsetAt(location.range.start);
    const text = doc.getText();
    assert.equal(
      text.slice(start, start + token.length),
      token,
      `reference did not select ${token}`,
    );
    const before = text.slice(0, start);
    const occurrence = before.split(token).length - 1;
    const key = `${file}:${occurrence}:${token}`;
    assert.ok(!seen.has(key), `duplicate reference ${key}`);
    seen.set(key, 1);
    keys.push(key);
  }
  return keys.sort();
}

async function assertReferences(local: LocalCarrierCase): Promise<void> {
  const doc = await openWorkspaceFile(local.declaration.file);
  const refs = await poll(
    `references ${local.file}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.Location[]>(
        "vscode.executeReferenceProvider",
        doc.uri,
        anchorPosition(doc, local.declaration),
      )) ?? [],
    (result) => result.length >= local.allReferences.length,
  );
  assertNoVirtualLocations(refs, "references");
  assert.deepEqual(
    await locationKeys(refs, local.declaration.token),
    expectedAnchorKeys(local.allReferences),
  );
}

function workspaceEditLocations(edit: vscode.WorkspaceEdit): Array<{
  uri: vscode.Uri;
  edit: vscode.TextEdit;
}> {
  return edit.entries().flatMap(([uri, edits]) => edits.map((entry) => ({ uri, edit: entry })));
}

async function assertAndApplyRename(
  local: LocalCarrierCase,
  origin: ContractAnchor,
  newName: string,
): Promise<void> {
  assert.equal(newName.length, origin.token.length, "contract rename keeps offsets stable");
  const originDoc = await openWorkspaceFile(origin.file);
  const result = await vscode.commands.executeCommand<vscode.WorkspaceEdit>(
    "vscode.executeDocumentRenameProvider",
    originDoc.uri,
    anchorPosition(originDoc, origin),
    newName,
  );
  assert.ok(result, `rename from ${origin.file}#${origin.token} returned no edit`);
  const edits = workspaceEditLocations(result);
  assert.equal(
    edits.length,
    local.allReferences.length,
    "rename must cover the exact semantic set",
  );
  assertNoVirtualLocations(
    edits.map(({ uri, edit }) => new vscode.Location(uri, edit.range)),
    "rename",
  );

  const actualKeys: string[] = [];
  for (const { uri, edit } of edits) {
    assert.equal(edit.newText, newName, "rename edit carries the requested spelling");
    const doc = await vscode.workspace.openTextDocument(uri);
    const start = doc.offsetAt(edit.range.start);
    const occurrence = doc.getText().slice(0, start).split(origin.token).length - 1;
    actualKeys.push(`${path.normalize(uri.fsPath)}:${occurrence}:${origin.token}`);
  }
  assert.deepEqual(actualKeys.sort(), expectedAnchorKeys(local.allReferences));

  const originals = new Map<string, string>();
  for (const { uri } of edits) {
    if (!originals.has(uri.toString())) {
      originals.set(uri.toString(), (await vscode.workspace.openTextDocument(uri)).getText());
    }
  }
  try {
    assert.equal(await vscode.workspace.applyEdit(result), true, "VS Code applies the rename edit");
    const renamedMarkup: ContractAnchor = { ...local.markupUse, token: newName };
    const renamedDeclaration: ContractAnchor = { ...local.declaration, token: newName };
    await assertDefinitionTargetsAnchor(renamedMarkup, renamedDeclaration);
  } finally {
    const restore = new vscode.WorkspaceEdit();
    for (const [uriText, original] of originals) {
      const uri = vscode.Uri.parse(uriText);
      const doc = await vscode.workspace.openTextDocument(uri);
      restore.replace(
        uri,
        new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)),
        original,
      );
    }
    assert.equal(
      await vscode.workspace.applyEdit(restore),
      true,
      "rename fixture restoration applies",
    );
    for (const [uriText, original] of originals) {
      assert.equal(
        (await vscode.workspace.openTextDocument(vscode.Uri.parse(uriText))).getText(),
        original,
      );
    }
  }
}

async function assertCleanDiagnostics(local: LocalCarrierCase): Promise<void> {
  const doc = await openWorkspaceFile(local.file);
  await assertDefinitionTargetsAnchor(local.markupUse, local.declaration);
  const diagnostics = await waitForDiagnosticsSettled(doc.uri, {
    timeoutMs: 10_000,
    stableMs: 600,
  });
  const errors = diagnostics.filter(
    (diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error,
  );
  assert.deepEqual(
    errors.map((diagnostic) => `${String(diagnostic.code)}:${diagnostic.message}`),
    [],
    `${local.file} must be clean after provider sync (including no TS7026)`,
  );
}

async function assertCleanFileDiagnostics(file: string): Promise<void> {
  const doc = await openWorkspaceFile(file);
  const diagnostics = await waitForDiagnosticsSettled(doc.uri, {
    timeoutMs: 10_000,
    stableMs: 600,
  });
  const errors = diagnostics.filter(
    (diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error,
  );
  assert.deepEqual(
    errors.map((diagnostic) => `${String(diagnostic.code)}:${diagnostic.message}`),
    [],
    `${file} must consume the component's public type without diagnostics`,
  );
}

async function assertTypedHover(local: LocalCarrierCase): Promise<void> {
  const doc = await openWorkspaceFile(local.markupUse.file);
  const hovers = await poll(
    `hover ${local.file}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        doc.uri,
        anchorPosition(doc, local.markupUse),
      )) ?? [],
    (result) => result.length > 0,
  );
  const text = hovers
    .flatMap((hover) => hover.contents)
    .map((content) => (typeof content === "string" ? content : content.value))
    .join("\n");
  for (const needle of local.hoverNeedles)
    assert.ok(text.includes(needle), `hover missing ${needle}: ${text}`);
  assert.ok(!/:\s*any\b/.test(text), `hover degraded to any: ${text}`);
}

async function assertTypedComponentHover(
  anchor: ContractAnchor,
  requiredSurface: readonly string[],
): Promise<void> {
  const doc = await openWorkspaceFile(anchor.file);
  const hovers = await poll(
    `component hover ${anchor.file}#${anchor.token}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        doc.uri,
        anchorPosition(doc, anchor),
      )) ?? [],
    (result) => result.length > 0,
  );
  const text = hovers
    .flatMap((hover) => hover.contents)
    .map((content) => (typeof content === "string" ? content : content.value))
    .join("\n");
  for (const needle of requiredSurface) {
    assert.ok(text.includes(needle), `component hover missing ${needle}: ${text}`);
  }
  assert.ok(!/\bany\b/.test(text), `component hover exposed an unsafe any carrier: ${text}`);
  assert.ok(!/__Verter\w*/.test(text), `component hover leaked generated symbols: ${text}`);
  assert.ok(!/\bunknown\b/.test(text), `component hover lost its concrete prop surface: ${text}`);
}

async function assertCtrlClick(local: LocalCarrierCase): Promise<void> {
  const doc = await openWorkspaceFile(local.markupUse.file);
  const editor = await vscode.window.showTextDocument(doc);
  const sourcePosition = anchorPosition(doc, local.markupUse);
  const targetOffset = anchorOffset(doc, local.declaration);
  editor.selection = new vscode.Selection(sourcePosition, sourcePosition);
  const config = vscode.workspace.getConfiguration("editor");
  const previous = config.get<string>("gotoLocation.multipleDefinitions");
  await config.update(
    "gotoLocation.multipleDefinitions",
    "goto",
    vscode.ConfigurationTarget.Workspace,
  );
  try {
    await vscode.commands.executeCommand("editor.action.revealDefinition");
    await poll(
      `CTRL+click ${local.file}`,
      async () => vscode.window.activeTextEditor,
      (active) =>
        active !== undefined &&
        path.normalize(active.document.uri.fsPath) === absoluteFile(local.declaration.file) &&
        active.document.offsetAt(active.selection.active) === targetOffset,
      10_000,
    );
  } finally {
    await config.update(
      "gotoLocation.multipleDefinitions",
      previous,
      vscode.ConfigurationTarget.Workspace,
    );
  }
}

export function registerFrameworkContract(descriptor: FrameworkContractDescriptor): void {
  const id = (capability: FrameworkContractCapability) =>
    frameworkContractId(descriptor.framework, capability);

  suite(`${descriptor.framework} semantic capability contract`, function () {
    suiteSetup(async function () {
      this.timeout(60_000);
      await ensureTypeProviderSynced();
      const entry = await openWorkspaceFile(descriptor.entry);
      assert.equal(entry.languageId, descriptor.languageId);
      await assertDefinitionTargetsAnchor(descriptor.ts.markupUse, descriptor.ts.declaration);
    });

    test(id("ts.clean-diagnostics"), () => assertCleanDiagnostics(descriptor.ts));
    test(id("js.clean-diagnostics"), () => assertCleanDiagnostics(descriptor.js));
    test(id("ts.definition.markup-to-script"), () =>
      assertDefinitionTargetsAnchor(descriptor.ts.markupUse, descriptor.ts.declaration),
    );
    test(id("js.definition.markup-to-script"), () =>
      assertDefinitionTargetsAnchor(descriptor.js.markupUse, descriptor.js.declaration),
    );
    test(id("ts.references.script-and-markup"), () => assertReferences(descriptor.ts));
    test(id("js.references.script-and-markup"), () => assertReferences(descriptor.js));
    test(id("ts.rename.from-script"), () =>
      assertAndApplyRename(descriptor.ts, descriptor.ts.declaration, "typedDatum"),
    );
    test(id("ts.rename.from-markup"), () =>
      assertAndApplyRename(descriptor.ts, descriptor.ts.markupUse, "typedDatum"),
    );
    test(id("js.rename.from-script"), () =>
      assertAndApplyRename(descriptor.js, descriptor.js.declaration, "jsDatum"),
    );
    test(id("js.rename.from-markup"), () =>
      assertAndApplyRename(descriptor.js, descriptor.js.markupUse, "jsDatum"),
    );
    test(id("ts.hover.typed-markup"), () => assertTypedHover(descriptor.ts));
    test(id("js.hover.typed-markup"), () => assertTypedHover(descriptor.js));
    test(id("import.direct.sfc-tag-to-child"), () =>
      assertDefinitionTargetsFile(descriptor.directParentTag, descriptor.directChildFile),
    );
    test(id("import.direct.plain-ts-to-child"), () =>
      assertDefinitionTargetsFile(descriptor.directConsumerUse, descriptor.directChildFile),
    );
    test(id("import.direct.sfc-tag-hover.typed"), () =>
      assertTypedComponentHover(descriptor.directParentTag, descriptor.directComponentHoverNeedles),
    );
    test(id("import.direct.plain-ts-hover.typed"), () =>
      assertTypedComponentHover(
        descriptor.directConsumerUse,
        descriptor.directComponentHoverNeedles,
      ),
    );
    test(id("import.deep-barrel.sfc-tag-to-child"), () =>
      assertDefinitionTargetsFile(descriptor.barrelParentTag, descriptor.barrelChildFile),
    );
    test(id("import.deep-barrel.plain-ts-to-child"), () =>
      assertDefinitionTargetsFile(descriptor.barrelConsumerUse, descriptor.barrelChildFile),
    );
    test(id("import.deep-barrel.sfc-tag-hover.typed"), () =>
      assertTypedComponentHover(descriptor.barrelParentTag, descriptor.barrelComponentHoverNeedles),
    );
    test(id("import.deep-barrel.plain-ts-hover.typed"), () =>
      assertTypedComponentHover(
        descriptor.barrelConsumerUse,
        descriptor.barrelComponentHoverNeedles,
      ),
    );
    test(id("import.deep-barrel.public-type.clean-diagnostics"), () =>
      assertCleanFileDiagnostics(descriptor.publicTypeConsumer),
    );
    test(id("ctrl-click.markup-to-script"), () => assertCtrlClick(descriptor.ts));
  });
}
