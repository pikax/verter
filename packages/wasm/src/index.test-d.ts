import type {
  ArtifactBlockToken,
  BlockContentCorrelationToken,
  BlockContentSourceSpaceToken,
  FrameworkArtifactToken,
  HostBlockContentCapturedEchoFields,
  WasmStampedBlockResult,
} from "./handoff-types";
import type {
  HostBlockOverrideRequest,
  HostCompileProfile,
  HostVirtualQuery,
} from "./request-types";
import type {
  HostBlockOverrideRequest as NativeHostBlockOverrideRequest,
  HostCompileProfile as NativeHostCompileProfile,
  HostVirtualQuery as NativeHostVirtualQuery,
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

// `@verter/wasm` declares the three request DTOs itself so its exported
// declaration graph never reaches the native compile profile. A caller must
// still be able to hand the same object to either binding, and nothing else
// holds the two copies to one shape — so it is held here, where a
// divergence in a field, its optionality or its type is a compile error.
// Naming the native types in a `.test-d.ts` exports nothing and ships
// nothing, so the declaration graph stays clean.
type _ProfileIsExactlyTheNativeProfile = Expect<
  Equal<HostCompileProfile, NativeHostCompileProfile>
>;
type _BlockOverrideRequestIsExactlyTheNativeOne = Expect<
  Equal<HostBlockOverrideRequest, NativeHostBlockOverrideRequest>
>;
type _VirtualQueryIsExactlyTheNativeOne = Expect<
  Equal<HostVirtualQuery, NativeHostVirtualQuery>
>;
