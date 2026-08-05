import { describe, expect, it } from "vitest";
import type {
  ArtifactBlockToken,
  BlockContentBasisToken,
  BlockContentCorrelationToken,
  BlockContentHashToken,
  BlockContentOwnerRevisionToken,
  BlockContentSourceSpaceToken,
  FrameworkArtifactToken,
  HostPreprocessorRequest,
} from "@verter/native";
import { copyCapturedBlockContentEcho } from "./preprocessor";

describe("captured block-content echo", () => {
  it("copies every sealed echo field and no mutable payload field", () => {
    const request = {
      contentClass: "style",
      lang: "scss",
      content: "$color: red",
      availability: "processedContentRequired",
      correlationToken: "correlation" as BlockContentCorrelationToken,
      blockToken: "block" as ArtifactBlockToken,
      ownerRevision: "revision" as BlockContentOwnerRevisionToken,
      artifactToken: "artifact" as FrameworkArtifactToken,
      expectedLanguage: "css",
      priorBasisToken: "prior-basis" as BlockContentBasisToken,
      basisToken: "basis" as BlockContentBasisToken,
      sourceSpaceToken: "source-space" as BlockContentSourceSpaceToken,
      contentHash: "content-hash" as BlockContentHashToken,
    } satisfies HostPreprocessorRequest;

    expect(copyCapturedBlockContentEcho(request)).toStrictEqual({
      correlationToken: request.correlationToken,
      blockToken: request.blockToken,
      ownerRevision: request.ownerRevision,
      artifactToken: request.artifactToken,
      expectedLanguage: request.expectedLanguage,
      priorBasisToken: request.priorBasisToken,
      basisToken: request.basisToken,
    });
  });
});
