import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import ts from "typescript";
import { afterEach, describe, expect, it } from "vitest";
import { VERTER_TYPES_STUB } from "./verterTypesStub";

const roots: string[] = [];

const VUE_STUB = `
export type ShallowUnwrapRef<T> = T;
export interface ComponentObjectPropsOptions {}
export type EmitsOptions = Record<string, unknown>;
export type ComponentTypeEmits = Record<string, unknown>;
export interface PropType<T> {}
export interface ComponentOptionsMixin {}
export interface ComputedOptions {}
export interface MethodOptions {}
export interface ComponentOptionsBase<A, B, C, D, E, F, G, H> {}
export const Comment: unique symbol;
export const Fragment: unique symbol;
export interface HTMLAttributes { class?: string }
export interface NativeElements { div: HTMLAttributes; [name: string]: unknown }
export interface Directive<T = any, V = any, M extends string = string, A = any> {}
export interface GlobalDirectives {}
declare global { namespace JSX { interface Element {} } }
`;

const GENERATED_IMPORT_CONTRACT = `
import type {
  Prettify,
  ExtractComponentProps,
  ExtractLeafElement,
  ExtractRenderComponent,
} from "@verter/types";
import {
  shallowUnwrapRef,
  enhanceElementWithProps,
  extractRenderComponent,
  instantiateComponent,
  extractArgumentsFromRenderSlot,
  runCustomDirective,
  retrieveSetupDirectives,
  strictRenderSlot,
  checkRequiredSlots,
} from "@verter/types";

declare const Component: new () => { $props: { label: string }; $el: HTMLDivElement };
type Props = Prettify<ExtractComponentProps<typeof Component>>;
const valid: Props = { label: "ok" };
// @ts-expect-error the virtual stub must preserve the component's public prop type
const invalid: Props = { label: 1 };
declare const leaf: ExtractLeafElement<typeof Component>;
const element: HTMLDivElement = leaf;
declare const rendered: ExtractRenderComponent<"div">;
void valid;
void invalid;
void element;
void rendered;
void shallowUnwrapRef;
void enhanceElementWithProps;
void extractRenderComponent;
void instantiateComponent;
void extractArgumentsFromRenderSlot;
void runCustomDirective;
void retrieveSetupDirectives;
void strictRenderSlot;
void checkRequiredSlots;
`;

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function typecheckVirtualPackage(stub: string): readonly ts.Diagnostic[] {
  const root = path.join(
    tmpdir(),
    `verter-types-stub-contract-${process.pid}-${roots.length}-${Date.now()}`,
  );
  roots.push(root);
  const vue = path.join(root, "node_modules", "vue");
  const verterTypes = path.join(root, "node_modules", "@verter", "types");
  mkdirSync(vue, { recursive: true });
  mkdirSync(verterTypes, { recursive: true });
  writeFileSync(path.join(vue, "index.d.ts"), VUE_STUB);
  writeFileSync(path.join(vue, "package.json"), '{"name":"vue","types":"index.d.ts"}');
  writeFileSync(path.join(verterTypes, "index.d.ts"), stub);
  writeFileSync(
    path.join(verterTypes, "package.json"),
    '{"name":"@verter/types","types":"index.d.ts"}',
  );
  const consumer = path.join(root, "contract.ts");
  writeFileSync(consumer, GENERATED_IMPORT_CONTRACT);

  const program = ts.createProgram([consumer], {
    lib: ["lib.es2022.d.ts", "lib.dom.d.ts"],
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    noEmit: true,
    skipLibCheck: false,
    strict: true,
    target: ts.ScriptTarget.ES2022,
  });
  return ts.getPreEmitDiagnostics(program);
}

describe("VERTER_TYPES_STUB", () => {
  it("type-checks the complete generated IDE import contract with precise component props", () => {
    const diagnostics = typecheckVirtualPackage(VERTER_TYPES_STUB);
    expect(
      diagnostics.map(
        (diagnostic) =>
          `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
      ),
    ).toEqual([]);
  });

  it("keeps the managed-tsgo fallback on the same typed import contract", () => {
    const rustStub = readFileSync(
      path.resolve(__dirname, "../../../../crates/verter_lsp/src/verter_types_stub.d.ts"),
      "utf8",
    );
    const diagnostics = typecheckVirtualPackage(rustStub);
    expect(
      diagnostics.map(
        (diagnostic) =>
          `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
      ),
    ).toEqual([]);
  });
});
