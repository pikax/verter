import type {
  ArtifactBlockToken,
  BlockContentCorrelationToken,
  BlockContentSourceSpaceToken,
  FrameworkArtifactToken,
  HostBlockContentCapturedEchoFields,
  WasmStampedBlockResult,
} from "./handoff-types";

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
