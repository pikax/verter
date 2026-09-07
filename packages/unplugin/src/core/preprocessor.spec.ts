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
import { copyCapturedBlockContentEcho, preprocessStyle } from "./preprocessor";

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

describe("style preprocessing failures outside Vite", () => {
  it("rethrows a Sass compile failure carrying the compiler error as the cause", async () => {
    // A caller that inspects `error.cause` — a bundler adapter deciding
    // whether the failure came from Sass or from Verter — must reach the
    // originating Sass error, not just the wrapped message.
    const failure = await preprocessStyle(
      "scss",
      '@error "authored scss is broken";',
      "/test/Broken.vue",
      null,
    ).then(
      () => null,
      (error: unknown) => error,
    );

    expect(failure).toBeInstanceOf(Error);
    const message = (failure as Error).message;
    expect(message).toContain('Failed to preprocess style lang="scss" in /test/Broken.vue');
    expect(message).toContain("authored scss is broken");
    const cause = (failure as Error & { cause?: unknown }).cause;
    expect(cause).toBeInstanceOf(Error);
    expect(cause).not.toBe(failure);
    expect((cause as Error).message).toContain("authored scss is broken");
  });

  it("returns null rather than throwing when no preprocessor owns the lang", async () => {
    // The absence of a preprocessor is not a build failure: the block is
    // reported unavailable so the host refuses it with its own message.
    expect(await preprocessStyle("stylus", "a\n  color red", "/test/Styl.vue", null)).toBeNull();
  });
});
