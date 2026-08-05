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
