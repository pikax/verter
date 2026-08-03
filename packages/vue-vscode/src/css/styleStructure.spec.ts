import { describe, expect, it } from "vitest";
import type { DocumentStructureResponseV1 } from "@verter/language-shared";
import { findStyleBlockAt, styleBlocksFromStructure } from "./styleStructure";

function available(source: string): DocumentStructureResponseV1 {
  const opening = source.indexOf("<style");
  const contentStart = source.indexOf(">", opening) + 1;
  const contentEnd = source.indexOf("</style>", contentStart);
  const fullEnd = contentEnd + "</style>".length;
  const langStart = source.indexOf("lang=", opening);
  const langEnd = langStart < 0 ? -1 : source.indexOf(">", langStart);
  const range = (start: number, end: number) => ({ sourceSpaceToken: "space", start, end });
  return {
    kind: "available",
    requestToken: "request",
    clientOpenEpoch: "open",
    expectedClientVersion: 1,
    structure: {
      schemaVersion: 1,
      documentRevisionToken: "revision",
      artifactToken: "artifact",
      markupNodes: [],
      blocks: [
        {
          kind: "section",
          markupRootTokens: [],
          section: {
            blockToken: "style-token",
            role: {
              kind: "style",
              dialect: source.includes("scss") ? "Scss" : "Css",
              scoped: source.includes("scoped"),
              module: "None",
            },
            openingRange: range(opening, contentStart),
            openingNameRange: range(opening + 1, opening + 6),
            contentRange: range(contentStart, contentEnd),
            closingRange: range(contentEnd, fullEnd),
            closingNameRange: range(contentEnd + 2, contentEnd + 7),
            fullRange: range(opening, fullEnd),
            attributeInsertionAnchor: range(contentStart - 1, contentStart - 1),
            attributes:
              langStart < 0
                ? []
                : [
                    {
                      attributeToken: "lang-token",
                      kind: "named",
                      name: {
                        spelling: "lang",
                        normalized: "lang",
                        range: range(langStart, langStart + 4),
                      },
                      value: source.includes("scss") ? "scss" : "css",
                      fullRange: range(langStart, langEnd),
                    },
                  ],
          },
        },
      ],
    },
  };
}

describe("styleBlocksFromStructure", () => {
  it("uses sealed structure geometry, dialect, and token identity", () => {
    const source = '<template />\n<style scoped lang="scss">\n.foo {}\n</style>';
    const blocks = styleBlocksFromStructure(source, available(source));
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ blockToken: "style-token", lang: "scss", scoped: true });
    expect(source.slice(blocks[0].contentStartOffset, blocks[0].contentEndOffset)).toBe(
      "\n.foo {}\n",
    );
    expect(findStyleBlockAt(blocks, source, 2, 2)?.blockToken).toBe("style-token");
  });

  it("marks external-src style sections typed external (no inline slice consumers)", () => {
    // R2-B-03: the parser-owned `src` attribute means the block's content is
    // an EXTERNAL file — the inline slice is framework-ignored and must not
    // be treated as available content.
    const source = '<template />\n<style src="./theme.css"></style>';
    const response = available(source);
    if (response.kind !== "available") throw new Error("fixture");
    const section = response.structure.blocks[0];
    if (section.kind !== "section") throw new Error("fixture");
    const srcStart = source.indexOf("src=");
    section.section.attributes.push({
      attributeToken: "src-token",
      kind: "named",
      name: {
        spelling: "src",
        normalized: "src",
        range: { sourceSpaceToken: "space", start: srcStart, end: srcStart + 3 },
      },
      value: "./theme.css",
      fullRange: { sourceSpaceToken: "space", start: srcStart, end: srcStart + 17 },
    });
    const blocks = styleBlocksFromStructure(source, response);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].externalSrc).toBe(true);
  });

  it("keeps inline style sections typed inline", () => {
    const source = "<template />\n<style>\n.foo {}\n</style>";
    const blocks = styleBlocksFromStructure(source, available(source));
    expect(blocks).toHaveLength(1);
    expect(blocks[0].externalSrc).toBe(false);
  });

  it("fails closed for typed unavailable and stale responses", () => {
    const base = { requestToken: "r", clientOpenEpoch: "o", expectedClientVersion: 1 };
    expect(
      styleBlocksFromStructure("<style/>", {
        kind: "unavailable",
        ...base,
        reason: "structureNotReady",
      }),
    ).toEqual([]);
    expect(styleBlocksFromStructure("<style/>", { kind: "staleClientDocument", ...base })).toEqual(
      [],
    );
  });
});
