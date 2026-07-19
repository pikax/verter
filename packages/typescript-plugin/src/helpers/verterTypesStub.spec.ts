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
export interface GlobalComponents {}
export interface GlobalDirectives {}
declare global { namespace JSX { interface Element {} } }
`;

// A Vue ≤3.4-shaped type surface: NO \`GlobalComponents\` export at all. The
// shipped empty \`declare module "vue" { interface GlobalComponents {} }\`
// augmentation must INTRODUCE the surface (so the stub's own
// \`import { GlobalComponents } from "vue"\` resolves) and user augmentations
// must still merge into it. (\`GlobalDirectives\` stays exported here: the
// pre-3.5 directives surface is a separate, undispositioned gap — this leg
// discriminates the GlobalComponents mechanism only.)
const VUE_STUB_WITHOUT_GLOBAL_COMPONENTS = VUE_STUB.replace(
  "export interface GlobalComponents {}\n",
  "",
);

// The official \`vue/jsx-runtime\` JSX namespace shape (mirrors the real
// package: IntrinsicElements carries the \`[name: string]: any\` escape hatch).
// \`GlobalComponentKebabType\`'s fail-open arm reads it.
const VUE_JSX_RUNTIME_STUB = `
import type { HTMLAttributes } from "../index";
export namespace JSX {
  export interface Element {}
  export interface ElementClass { $props: {} }
  export interface ElementAttributesProperty { $props: {} }
  export interface IntrinsicElements { div: HTMLAttributes; [name: string]: any }
  export interface IntrinsicAttributes {}
}
`;

const GENERATED_IMPORT_CONTRACT = `
import type {
  Prettify,
  ExtractComponentProps,
  ExtractLeafElement,
  ExtractRenderComponent,
  GlobalComponentType,
  GlobalComponentKebabType,
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
  globalComponentsNav,
} from "@verter/types";

declare const Component: new () => { $props: { label: string }; $el: HTMLDivElement };
type Props = Prettify<ExtractComponentProps<typeof Component>>;
const valid: Props = { label: "ok" };
// @ts-expect-error the virtual stub must preserve the component's public prop type
const invalid: Props = { label: 1 };
declare const leaf: ExtractLeafElement<typeof Component>;
const element: HTMLDivElement = leaf;
declare const rendered: ExtractRenderComponent<"div">;
// The NAV-PROBE shape the generated TSX emits per fallback const, against a
// registered augmentation member (the unregistered case lives in dropped
// synthetic text in the real carrier).
declare module "vue" {
  interface GlobalComponents {
    SomeName: new () => { $props: {} };
  }
}
void globalComponentsNav().SomeName;
// An unregistered global name resolves fail-closed to \`unknown\`, never \`any\`.
declare const globalMiss: GlobalComponentType<"NotRegistered">;
const failClosed: unknown = globalMiss;
// @ts-expect-error unknown (not any) — a fail-open \`any\` here would compile
const failOpen: { $props: {} } = globalMiss;
// The kebab-authored fallback type: a REGISTERED member (Pascal key) resolves
// its component type.
declare const kebabHit: GlobalComponentKebabType<"SomeName", "some-name">;
const kebabResolved: new () => { $props: {} } = kebabHit;
// An UNREGISTERED kebab tag is fail-OPEN: it types as a function component
// over the intrinsic surface. This assignment DISCRIMINATES against the
// fail-closed revert — \`unknown\` is not assignable to a function type.
declare const kebabMiss: GlobalComponentKebabType<"NotRegisteredEither", "x-web-comp">;
const kebabFailOpen: (props: any) => any = kebabMiss;
// @ts-expect-error … and it is a FUNCTION type, not \`any\` (an \`any\` here would compile)
const kebabNotAny: number = kebabMiss;
void kebabResolved;
void kebabFailOpen;
void kebabNotAny;
void valid;
void invalid;
void element;
void rendered;
void failClosed;
void failOpen;
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

// A consumer with NO user augmentation at all: on a ≤3.4-shaped vue, ONLY the
// stub's shipped empty augmentation can introduce the `GlobalComponents`
// surface — this is the contract that discriminates its presence.
const NO_AUGMENTATION_CONTRACT = `
import type { GlobalComponentType, GlobalComponentKebabType } from "@verter/types";
declare const globalMiss: GlobalComponentType<"NotRegistered">;
const failClosed: unknown = globalMiss;
// @ts-expect-error unknown (not any) — a fail-open \`any\` here would compile
const failOpen: { $props: {} } = globalMiss;
declare const kebabMiss: GlobalComponentKebabType<"NotRegistered", "x-web-comp">;
const kebabFailOpen: (props: any) => any = kebabMiss;
void failClosed;
void failOpen;
void kebabFailOpen;
`;

function typecheckVirtualPackage(
  stub: string,
  vueStub: string = VUE_STUB,
  consumerContract: string = GENERATED_IMPORT_CONTRACT,
): readonly ts.Diagnostic[] {
  const root = path.join(
    tmpdir(),
    `verter-types-stub-contract-${process.pid}-${roots.length}-${Date.now()}`,
  );
  roots.push(root);
  const vue = path.join(root, "node_modules", "vue");
  const vueJsxRuntime = path.join(vue, "jsx-runtime");
  const verterTypes = path.join(root, "node_modules", "@verter", "types");
  mkdirSync(vueJsxRuntime, { recursive: true });
  mkdirSync(verterTypes, { recursive: true });
  writeFileSync(path.join(vue, "index.d.ts"), vueStub);
  writeFileSync(path.join(vueJsxRuntime, "index.d.ts"), VUE_JSX_RUNTIME_STUB);
  writeFileSync(path.join(vue, "package.json"), '{"name":"vue","types":"index.d.ts"}');
  writeFileSync(path.join(verterTypes, "index.d.ts"), stub);
  writeFileSync(
    path.join(verterTypes, "package.json"),
    '{"name":"@verter/types","types":"index.d.ts"}',
  );
  const consumer = path.join(root, "contract.ts");
  writeFileSync(consumer, consumerContract);

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

  // ── Vue ≤3.4 discriminator ────────────────────────────────────────────────
  //
  // The claim under test: the shipped empty `declare module "vue" { interface
  // GlobalComponents {} }` augmentation INTRODUCES the surface on a Vue whose
  // types export no `GlobalComponents` at all (≤3.4-shaped), and user
  // augmentations still MERGE into it (registered member types, unregistered
  // stays fail-closed `unknown`, kebab miss stays fail-open). The previous
  // coverage only ran against a 3.5-shaped vue stub that PREDEFINED the
  // interface, so the introduce-on-absence half was unproven.
  it("introduces the GlobalComponents surface on a Vue without the export (≤3.4) and merges augmentations", () => {
    const diagnostics = typecheckVirtualPackage(
      VERTER_TYPES_STUB,
      VUE_STUB_WITHOUT_GLOBAL_COMPONENTS,
    );
    expect(
      diagnostics.map(
        (diagnostic) =>
          `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
      ),
    ).toEqual([]);
  });

  it("introduces the surface on ≤3.4 through the managed-tsgo fallback stub too", () => {
    const rustStub = readFileSync(
      path.resolve(__dirname, "../../../../crates/verter_lsp/src/verter_types_stub.d.ts"),
      "utf8",
    );
    const diagnostics = typecheckVirtualPackage(rustStub, VUE_STUB_WITHOUT_GLOBAL_COMPONENTS);
    expect(
      diagnostics.map(
        (diagnostic) =>
          `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
      ),
    ).toEqual([]);
  });

  // A project with NO user augmentation on ≤3.4: the shipped empty
  // augmentation ALONE introduces the surface (fail-closed `unknown` /
  // fail-open kebab both compile).
  it("introduces the surface on ≤3.4 with no user augmentation at all", () => {
    const diagnostics = typecheckVirtualPackage(
      VERTER_TYPES_STUB,
      VUE_STUB_WITHOUT_GLOBAL_COMPONENTS,
      NO_AUGMENTATION_CONTRACT,
    );
    expect(
      diagnostics.map(
        (diagnostic) =>
          `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
      ),
    ).toEqual([]);
  });

  // Negative control proving the ≤3.4 leg DISCRIMINATES: with the shipped
  // empty augmentation stripped from the stub AND no user augmentation in the
  // consumer, the contract must FAIL against the GlobalComponents-less vue
  // (nothing introduces the interface, so the stub's own import of
  // `GlobalComponents` cannot resolve). A green here would mean the leg
  // proves nothing. (The full GENERATED_IMPORT_CONTRACT cannot serve as this
  // control: its own user `declare module "vue"` augmentation would introduce
  // the surface and mask the stub's absence.)
  it("the ≤3.4 leg fails without the shipped empty augmentation (discrimination control)", () => {
    const withoutAugmentation = VERTER_TYPES_STUB.replace(
      /declare module "vue" \{[\s\S]*?interface GlobalComponents \{\}\n\}/,
      "",
    );
    // Prove the strip actually applied before trusting the run.
    expect(VERTER_TYPES_STUB).toContain("interface GlobalComponents {}");
    expect(withoutAugmentation).not.toContain("interface GlobalComponents {}");
    const diagnostics = typecheckVirtualPackage(
      withoutAugmentation,
      VUE_STUB_WITHOUT_GLOBAL_COMPONENTS,
      NO_AUGMENTATION_CONTRACT,
    );
    expect(diagnostics.length).toBeGreaterThan(0);
  });
});
