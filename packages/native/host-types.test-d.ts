import type {
  ArtifactBlockToken,
  BlockContentArtifactToken,
  BlockContentBasisToken,
  BlockContentCorrelationToken,
  BlockContentHashToken,
  BlockContentOwnerRevisionToken,
  BlockContentSourceSpaceToken,
  FrameworkArtifactToken,
  HostBlockContentCapturedEcho,
  HostBlockContentCapturedEchoFields,
  HostBlockOverrideEntry,
  HostExternalSourceRequest,
  HostPreprocessorRequest,
} from "./index";

type Equal<Left, Right> =
  (<Value>() => Value extends Left ? 1 : 2) extends <Value>() => Value extends Right ? 1 : 2
    ? true
    : false;
type Expect<Value extends true> = Value;

declare const correlationToken: BlockContentCorrelationToken;
declare const blockToken: ArtifactBlockToken;
declare const ownerRevision: BlockContentOwnerRevisionToken;
declare const artifactToken: FrameworkArtifactToken;
declare const priorBasisToken: BlockContentBasisToken;
declare const basisToken: BlockContentBasisToken;
declare const sourceSpaceToken: BlockContentSourceSpaceToken;
declare const contentArtifactToken: BlockContentArtifactToken;
declare const contentHash: BlockContentHashToken;
declare const request: HostPreprocessorRequest;

const capturedEchoFields: HostBlockContentCapturedEchoFields = {
  correlationToken,
  blockToken,
  ownerRevision,
  artifactToken,
  expectedLanguage: "scss",
  priorBasisToken,
  basisToken,
};

const capturedEcho: HostBlockContentCapturedEcho = {
  request: {
    canonicalId: "/src/App.vue",
    correlationToken,
    blockToken,
    ownerRevision,
    artifactToken,
    expectedLanguage: "scss",
    priorBasisToken,
  },
  basisToken,
};

const override: HostBlockOverrideEntry = {
  ...capturedEchoFields,
  sourceSpaceToken,
  code: ".app {}",
  codeHash: contentHash,
};

const externalRequest: HostExternalSourceRequest = {
  ownerCanonicalId: "/src/App.vue",
  blockKind: "style",
  specifier: "./app.scss",
  resolvedCanonicalId: "/src/app.scss",
  blockToken,
  ownerRevision,
  artifactToken,
  carrierSourceSpaceToken: sourceSpaceToken,
};

const requestEcho: HostBlockContentCapturedEchoFields = request;

type ExpectedCapturedEchoFieldKeys =
  | "correlationToken"
  | "blockToken"
  | "ownerRevision"
  | "artifactToken"
  | "expectedLanguage"
  | "priorBasisToken"
  | "basisToken";
type _CapturedEchoFieldsAreExact = Expect<
  Equal<keyof HostBlockContentCapturedEchoFields, ExpectedCapturedEchoFieldKeys>
>;

// Raw wire strings are not trusted token values inside typed callers.
// @ts-expect-error raw strings must cross the host/protocol boundary first
const rawBlockToken: ArtifactBlockToken = "block";

// Nominal token classes cannot be accidentally substituted for each other.
// @ts-expect-error an artifact token is not a block token
const artifactAsBlock: ArtifactBlockToken = artifactToken;
// @ts-expect-error an owner revision is not an artifact token
const revisionAsArtifact: FrameworkArtifactToken = ownerRevision;
// @ts-expect-error a source-space token is not a content basis
const sourceSpaceAsBasis: BlockContentBasisToken = sourceSpaceToken;
// @ts-expect-error a content-artifact token is not a content hash
const contentArtifactAsHash: BlockContentHashToken = contentArtifactToken;
// @ts-expect-error a content hash is not a correlation token
const hashAsCorrelation: BlockContentCorrelationToken = contentHash;

void [
  capturedEchoFields,
  capturedEcho,
  override,
  externalRequest,
  requestEcho,
  rawBlockToken,
  artifactAsBlock,
  revisionAsArtifact,
  sourceSpaceAsBasis,
  contentArtifactAsHash,
  hashAsCorrelation,
];

// ---------------------------------------------------------------------------
// Host compile request: exhaustive discrimination, closed objects, closed
// vocabularies, required slots.
//
// Every `@ts-expect-error` below is itself checked: TypeScript reports an
// unused directive if the line it guards stops being an error, so each one
// fails the moment the type stops refusing what it names.
// ---------------------------------------------------------------------------

import type {
  HostAnalysisProduct,
  HostCompileRequest,
  HostDeclarationsProduct,
  HostIdeCompanionProduct,
  HostPublicApiProduct,
  HostRequestedProduct,
  HostSvelteCompatibility,
  HostSvelteCompileOptions,
  HostSvelteCompileRequest,
  HostSvelteCustomElementDescriptor,
  HostSvelteCustomElementProp,
  HostVueAssetUrlTransform,
  HostVueBackend,
  HostVueCompileOptions,
  HostVueCompileRequest,
  HostVueCssModules,
} from "./index";

type FrameworkTagsAreExactlyTheTwoArms = Expect<
  Equal<HostCompileRequest["framework"], "vue" | "svelte">
>;
type RequestUnionIsExactlyTheTwoArms = Expect<
  Equal<HostCompileRequest, HostVueCompileRequest | HostSvelteCompileRequest>
>;
type ProductKindsAreExactlyTheSixProducts = Expect<
  Equal<
    HostRequestedProduct["kind"],
    "runtimeClient" | "runtimeServer" | "ideCompanion" | "publicApi" | "declarations" | "analysis"
  >
>;

/** Narrowing on `framework` reaches each arm's own option type, and the
 * union is exhausted — a third arm would leave `request` inhabited here. */
function optionsFor(request: HostCompileRequest): HostVueCompileOptions | HostSvelteCompileOptions {
  switch (request.framework) {
    case "vue":
      return request.options;
    case "svelte":
      return request.options;
    default: {
      const unreachable: never = request;
      return unreachable;
    }
  }
}

/** Narrowing on `kind` exhausts the product union. */
function productTag(product: HostRequestedProduct): string {
  switch (product.kind) {
    case "runtimeClient":
    case "runtimeServer":
      return product.runtimeSourceMap ? "mapped" : "plain";
    case "ideCompanion":
      return product.strictSlots ? "strict" : "loose";
    case "publicApi":
    case "declarations":
      return product.kind;
    case "analysis":
      return product.wantScriptBindings ? "bindings" : "template";
    default: {
      const unreachable: never = product;
      return unreachable;
    }
  }
}

const vueRequest: HostVueCompileRequest = {
  framework: "vue",
  identity: { filename: "Comp.vue", isProduction: false, forceJs: false },
  products: [
    { kind: "runtimeClient", runtimeSourceMap: true },
    { kind: "publicApi" },
    {
      kind: "ideCompanion",
      wantSourceMap: true,
      embedAmbientTypes: false,
      conditionalRootNarrowing: false,
      strictSlots: true,
      ideChunkBoundaries: false,
    },
  ],
  options: {
    backend: "vapor",
    ssr: false,
    isCustomElement: ["my-el"],
    babelParserPlugins: [],
    delimiters: ["<%", "%>"],
    cssModules: { scopeBehaviour: "global", localsConvention: "camelCaseOnly" },
    transformAssetUrls: { enabled: { tags: { img: ["src"] } } },
  },
};

const svelteRequest: HostSvelteCompileRequest = {
  framework: "svelte",
  identity: { isProduction: true, forceJs: false },
  products: [{ kind: "analysis", wantScriptBindings: true, wantTemplateData: false }],
  options: {
    dev: false,
    namespace: "mathMl",
    runes: "infer",
    compatibility: {},
    customElementDescriptor: { tag: "my-el", props: { value: { propType: "number" } } },
  },
};

// `compatibility` is a closed object with no wire slot, and it is closed
// against an object that was typed BEFORE it reached the slot as well as
// against a fresh literal. A projection that only forbids the fresh literal
// lets a caller type-check against a shape the decoder refuses.
declare const builtCompatibility: { componentApi: boolean };
// @ts-expect-error `componentApi` has no wire slot
const preTypedCompatibilityOption: HostSvelteCompileOptions = { compatibility: builtCompatibility };
// @ts-expect-error `componentApi` has no wire slot
const preTypedCompatibility: HostSvelteCompatibility = builtCompatibility;
const literalCompatibilityOption: HostSvelteCompileOptions = {
  // @ts-expect-error `componentApi` has no wire slot
  compatibility: { componentApi: true },
};
// @ts-expect-error `componentApi` has no wire slot
const literalCompatibility: HostSvelteCompatibility = { componentApi: true };

// A Svelte option has no slot in the Vue arm, and vice versa.
const crossFrameworkVueOption: HostVueCompileOptions = {
  backend: "inferred",
  ssr: false,
  isCustomElement: [],
  babelParserPlugins: [],
  // @ts-expect-error `runes` is a Svelte option
  runes: "infer",
};
const crossFrameworkSvelteOption: HostSvelteCompileOptions = {
  // @ts-expect-error `backend` is a Vue option
  backend: "vdom",
};

// Unknown keys are refused at each nesting level of the request.
const unknownIdentityKey: HostVueCompileRequest["identity"] = {
  isProduction: false,
  forceJs: false,
  // @ts-expect-error the identity carries no `sourceMap`
  sourceMap: true,
};
const unknownNestedOptionKey: HostVueCssModules = {
  scopeBehaviour: "local",
  // @ts-expect-error css modules carry no `bogus`
  bogus: true,
};
const unknownAssetUrlKey: HostVueAssetUrlTransform = {
  // @ts-expect-error the enabled asset-url options carry no `bogus`
  enabled: { tags: {}, bogus: true },
};
const unknownDescriptorKey: HostSvelteCustomElementDescriptor = {
  props: {},
  // @ts-expect-error the descriptor carries no `bogus`
  bogus: true,
};

// A product carries only its own options.
const foreignProductOption: HostAnalysisProduct = {
  kind: "analysis",
  wantScriptBindings: true,
  wantTemplateData: true,
  // @ts-expect-error `strictSlots` belongs to the ide product
  strictSlots: true,
};
const optionOnAnOptionlessProduct: HostPublicApiProduct = {
  kind: "publicApi",
  // @ts-expect-error a public-api product carries no options
  inline: true,
};
const misspelledProductTag: HostDeclarationsProduct = {
  // @ts-expect-error `declaration` is not a product tag
  kind: "declaration",
};

// Closed vocabularies refuse a spelling outside them.
const spellingOutsideTheBackendVocabulary: HostVueCompileOptions = {
  // @ts-expect-error `runtime` is not a backend
  backend: "runtime",
  ssr: false,
  isCustomElement: [],
  babelParserPlugins: [],
};
const spellingOutsideTheNamespaceVocabulary: HostSvelteCompileOptions = {
  // @ts-expect-error `xml` is not a namespace
  namespace: "xml",
};

// Required slots are required: an omitted one is a type error, never a
// substituted value.
// @ts-expect-error `backend`, `ssr`, `isCustomElement` and `babelParserPlugins` are required
const missingRequiredVueOptions: HostVueCompileOptions = {};
// @ts-expect-error `isProduction` and `forceJs` are required
const missingRequiredIdentity: HostVueCompileRequest["identity"] = {};
// @ts-expect-error `runtimeSourceMap` is required
const missingRequiredProductOption: HostRequestedProduct = { kind: "runtimeClient" };

// The four shape classes the generated declarations project, pinned as
// identities rather than as assignability: a widened, narrowed or
// re-spelled slot fails here even where an example value would still
// type-check.

// Closed string union: the vocabulary is exactly the decoder's variants.
type BackendVocabularyIsClosed = Expect<Equal<HostVueBackend, "inferred" | "vdom" | "vapor">>;

// Optional slot: omitted, `undefined` and `null` all decode as absent.
type AnOptionalSlotAdmitsAbsenceInEveryForm = Expect<
  Equal<HostVueCompileOptions["comments"], boolean | null | undefined>
>;

// Required slot: no absence, no null.
type ARequiredSlotAdmitsNeither = Expect<Equal<HostVueCompileOptions["ssr"], boolean>>;

// Closed object: the option set is exactly the decoder's field set, so an
// added or removed Rust field shows up here as a changed key set.
type IdeProductKeysAreExactlyTheDecodersFields = Expect<
  Equal<
    keyof HostIdeCompanionProduct,
    | "kind"
    | "wantSourceMap"
    | "embedAmbientTypes"
    | "conditionalRootNarrowing"
    | "strictSlots"
    | "typesModuleName"
    | "ideChunkBoundaries"
  >
>;

// The custom-element prop type is a FORWARDED spelling, not a wire
// vocabulary: membership is decided once, at canonical request
// construction, which admits each variant's lowercase and capitalised
// spelling. A closed union here would refuse spellings the runtime
// accepts, so the slot is pinned open.
type PropTypeIsAForwardedSpelling = Expect<
  Equal<HostSvelteCustomElementProp["propType"], string | null | undefined>
>;

const capitalisedPropTypeSpelling: HostSvelteCustomElementDescriptor = {
  props: { value: { propType: "Number" }, label: { propType: "string" } },
};

void [capitalisedPropTypeSpelling];

export type {
  ARequiredSlotAdmitsNeither,
  AnOptionalSlotAdmitsAbsenceInEveryForm,
  BackendVocabularyIsClosed,
  FrameworkTagsAreExactlyTheTwoArms,
  IdeProductKeysAreExactlyTheDecodersFields,
  ProductKindsAreExactlyTheSixProducts,
  PropTypeIsAForwardedSpelling,
  RequestUnionIsExactlyTheTwoArms,
};
export {
  crossFrameworkSvelteOption,
  crossFrameworkVueOption,
  foreignProductOption,
  misspelledProductTag,
  missingRequiredIdentity,
  missingRequiredProductOption,
  missingRequiredVueOptions,
  optionsFor,
  optionOnAnOptionlessProduct,
  productTag,
  spellingOutsideTheBackendVocabulary,
  spellingOutsideTheNamespaceVocabulary,
  svelteRequest,
  unknownAssetUrlKey,
  unknownDescriptorKey,
  unknownIdentityKey,
  unknownNestedOptionKey,
  vueRequest,
};
