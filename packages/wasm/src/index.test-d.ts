import type {
  ArtifactBlockToken,
  BlockContentCorrelationToken,
  BlockContentSourceSpaceToken,
  FrameworkArtifactToken,
  HostBlockContentCapturedEchoFields,
  WasmStampedBlockResult,
} from "./handoff-types";
import type {
  BrowserHostCompileRequest,
  BrowserHostRequestedProduct,
  BrowserHostSvelteCompileRequest,
  BrowserHostVueCompileRequest,
} from "./compile-request-types";
import type {
  HostAnalysisProductOptions as NativeHostAnalysisProductOptions,
  HostCompileRequest as NativeHostCompileRequest,
  HostRequestedProduct as NativeHostRequestedProduct,
  HostRuntimeProductOptions as NativeHostRuntimeProductOptions,
  HostSvelteCompileRequest as NativeHostSvelteCompileRequest,
  HostVueCompileRequest as NativeHostVueCompileRequest,
} from "@verter/native/host-types";

type Equal<Left, Right> =
  (<Value>() => Value extends Left ? 1 : 2) extends <Value>() => Value extends Right ? 1 : 2
    ? true
    : false;
type Expect<Value extends true> = Value;

type _WasmEchoIsTheExactSharedEcho = Expect<
  Equal<
    Pick<WasmStampedBlockResult, keyof HostBlockContentCapturedEchoFields>,
    HostBlockContentCapturedEchoFields
  >
>;
type _WasmSourceSpaceKeepsItsBrand = Expect<
  Equal<WasmStampedBlockResult["sourceSpaceToken"], BlockContentSourceSpaceToken>
>;

declare const blockToken: ArtifactBlockToken;
declare const artifactToken: FrameworkArtifactToken;
declare const correlationToken: BlockContentCorrelationToken;

// @ts-expect-error WASM must preserve the shared nominal token classes.
const artifactAsBlock: ArtifactBlockToken = artifactToken;
// @ts-expect-error A correlation token cannot be substituted for a block token.
const correlationAsBlock: ArtifactBlockToken = correlationToken;

void [blockToken, artifactAsBlock, correlationAsBlock];

// The typed compile request is the SAME schema the native binding decodes,
// tagged differently: this binding's decoder puts the arm name in the key
// (`{ vue: … }`), the native one puts it in a `framework` / `kind` field.
// Only the two tagged wrappers are declared locally; every leaf shape is
// imported from the generated projection. What is unheld by that import is
// the ARM SET and the wrapper BODIES — an arm added, removed or renamed on
// the shared schema would type-check here and refuse at run time — so both
// are pinned to the generated union below.
//
// The arms are mutually exclusive, so each one also declares every OTHER
// arm's key as an optional `never`. The tag is therefore the arm's REQUIRED
// key, not any key it mentions — reading `keyof` here would report the whole
// arm set for every arm and stop discriminating a mis-tagged arm.
type RequiredKeys<Value> = {
  [Key in keyof Value]-?: Record<string, never> extends Pick<Value, Key> ? never : Key;
}[keyof Value];
type ExternalTag<Arm> = Arm extends string ? Arm : RequiredKeys<Arm>;

type _ProductArmsAreTheGeneratedOnes = Expect<
  Equal<ExternalTag<BrowserHostRequestedProduct>, NativeHostRequestedProduct["kind"]>
>;
type _FrameworkArmsAreTheGeneratedOnes = Expect<
  Equal<ExternalTag<BrowserHostCompileRequest>, NativeHostCompileRequest["framework"]>
>;

// The arms are EXCLUSIVE. A plain union would accept an object naming two
// arms at once — TypeScript's excess-property check does not fire through a
// union — and the decoder refuses exactly that payload, so a well-typed call
// would be a guaranteed run-time throw. Both directions are asserted: the
// multi-arm shapes must be rejected AND the single-arm shapes must still be
// accepted, else a type that rejects everything would pass the negative legs.
type _BothFrameworkArmsAreRejected = Expect<
  Equal<
    {
      vue: BrowserHostVueCompileRequest;
      svelte: BrowserHostSvelteCompileRequest;
    } extends BrowserHostCompileRequest
      ? true
      : false,
    false
  >
>;
type _OneFrameworkArmIsAccepted = Expect<
  Equal<
    { vue: BrowserHostVueCompileRequest } extends BrowserHostCompileRequest ? true : false,
    true
  >
>;
// Default consumer configs treat an optional property as accepting an
// explicit `undefined`. The callable route treats those sibling tags as
// absent, keeping the published declaration and runtime behavior aligned.
type _UndefinedFrameworkSiblingIsAccepted = Expect<
  Equal<
    {
      vue: BrowserHostVueCompileRequest;
      svelte: undefined;
    } extends BrowserHostCompileRequest
      ? true
      : false,
    true
  >
>;
type _TwoProductTagsAreRejected = Expect<
  Equal<
    {
      runtimeClient: NativeHostRuntimeProductOptions;
      analysis: NativeHostAnalysisProductOptions;
    } extends BrowserHostRequestedProduct
      ? true
      : false,
    false
  >
>;
type _OneProductTagIsAccepted = Expect<
  Equal<
    { runtimeClient: NativeHostRuntimeProductOptions } extends BrowserHostRequestedProduct
      ? true
      : false,
    true
  >
>;
type _UndefinedProductSiblingIsAccepted = Expect<
  Equal<
    {
      runtimeClient: NativeHostRuntimeProductOptions;
      analysis: undefined;
    } extends BrowserHostRequestedProduct
      ? true
      : false,
    true
  >
>;
// `products` is excluded on both sides because its ELEMENT type is exactly
// what differs: the generated arm holds internally-tagged products, this one
// holds the externally-tagged union pinned below. Every other field — the
// identity and the framework's whole option surface — must be the generated
// one verbatim.
type _VueArmBodyIsTheGeneratedOne = Expect<
  Equal<
    Omit<BrowserHostVueCompileRequest, "products">,
    Omit<NativeHostVueCompileRequest, "framework" | "products">
  >
>;
type _SvelteArmBodyIsTheGeneratedOne = Expect<
  Equal<
    Omit<BrowserHostSvelteCompileRequest, "products">,
    Omit<NativeHostSvelteCompileRequest, "framework" | "products">
  >
>;
// …and the excluded slot is the local union, not something else entirely.
type _VueArmProductsAreTheLocalUnion = Expect<
  Equal<BrowserHostVueCompileRequest["products"], BrowserHostRequestedProduct[]>
>;
type _SvelteArmProductsAreTheLocalUnion = Expect<
  Equal<BrowserHostSvelteCompileRequest["products"], BrowserHostRequestedProduct[]>
>;

// Each externally-tagged product arm carries exactly the generated arm's
// options — the arm-set check above says the tags line up, not that the
// payload behind a tag does. EVERY option-carrying arm is listed: the two
// runtime arms share one option type locally, so a generated schema that
// gave the server arm its own shape would otherwise type-check here and
// refuse at run time.
type ArmOptions<Tag extends string> =
  Extract<BrowserHostRequestedProduct, Record<Tag, unknown>> extends Record<Tag, infer Options>
    ? Options
    : never;
type GeneratedArmOptions<Tag extends string> = Omit<
  Extract<NativeHostRequestedProduct, { kind: Tag }>,
  "kind"
>;

type _RuntimeClientArmCarriesTheGeneratedOptions = Expect<
  Equal<ArmOptions<"runtimeClient">, GeneratedArmOptions<"runtimeClient">>
>;
type _RuntimeServerArmCarriesTheGeneratedOptions = Expect<
  Equal<ArmOptions<"runtimeServer">, GeneratedArmOptions<"runtimeServer">>
>;
type _IdeCompanionArmCarriesTheGeneratedOptions = Expect<
  Equal<ArmOptions<"ideCompanion">, GeneratedArmOptions<"ideCompanion">>
>;
type _AnalysisArmCarriesTheGeneratedOptions = Expect<
  Equal<ArmOptions<"analysis">, GeneratedArmOptions<"analysis">>
>;
