export type {
  ArtifactBlockToken,
  FrameworkArtifactToken,
  BlockContentOwnerRevisionToken,
  BlockContentBasisToken,
  BlockContentCorrelationToken,
  BlockContentSourceSpaceToken,
  BlockContentArtifactToken,
  BlockContentHashToken,
  HostBlockContentPreCaptureEcho,
  HostBlockContentCapturedEcho,
  HostBlockContentCapturedEchoFields,
} from "@verter/native/host-types";

import type {
  HostBlockContentCapturedEchoFields,
  HostBlockOverrideEntry,
} from "@verter/native/host-types";

/** Exact flattened captured echo plus its selected input source-space stamp. */
export type WasmStampedBlockResult = HostBlockContentCapturedEchoFields &
  Pick<HostBlockOverrideEntry, "sourceSpaceToken">;
