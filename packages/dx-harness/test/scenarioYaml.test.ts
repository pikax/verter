import { describe, expect, it } from "vitest";

import { YamlParseError, parseScenarioYaml } from "../src/scenario/load.js";

/**
 * The scenario YAML loader parses with a small, dependency-free YAML SUBSET (no
 * third-party `yaml`), so the package stays hermetic and adds zero lockfile drift.
 * These tests pin the supported subset AND the rejection of everything outside it —
 * a parser that silently accepted malformed input would let an ill-formed scenario
 * reach (and slip past) the validator with a wrong shape.
 */
describe("parseScenarioYaml — supported subset", () => {
  it("parses a block mapping of typed scalars", () => {
    expect(parseScenarioYaml("id: hover-scenario\nfixture: template-events")).toEqual({
      id: "hover-scenario",
      fixture: "template-events",
    });
  });

  it("coerces booleans, null, integers, and floats — but keeps version-ish strings as strings", () => {
    const parsed = parseScenarioYaml(
      ["flag: true", "off: false", "nil: null", "tilde: ~", "i: 42", "f: 1.5", "neg: -3"].join(
        "\n",
      ),
    );
    expect(parsed).toEqual({
      flag: true,
      off: false,
      nil: null,
      tilde: null,
      i: 42,
      f: 1.5,
      neg: -3,
    });
    // A plain scalar that merely contains digits/dots/dashes is a STRING, never a number.
    expect(parseScenarioYaml("entryFile: App.vue")).toEqual({ entryFile: "App.vue" });
    expect(parseScenarioYaml("fixture: minimal-member-access")).toEqual({
      fixture: "minimal-member-access",
    });
  });

  it("parses a nested block mapping (deeper indent)", () => {
    const parsed = parseScenarioYaml(
      ["thresholds:", "  steadyStateCompileDelta: 0", "  latency:", "    p95Ms: 200"].join("\n"),
    );
    expect(parsed).toEqual({ thresholds: { steadyStateCompileDelta: 0, latency: { p95Ms: 200 } } });
  });

  it("parses a flow sequence (inline) and an empty flow sequence", () => {
    expect(parseScenarioYaml("requiredDrivers: [rawLsp, tsgo]")).toEqual({
      requiredDrivers: ["rawLsp", "tsgo"],
    });
    expect(parseScenarioYaml("capabilityRequirements: []")).toEqual({
      capabilityRequirements: [],
    });
  });

  it("parses a block sequence of scalars under a key", () => {
    expect(parseScenarioYaml(["anchors:", "  - a", "  - b", "  - c"].join("\n"))).toEqual({
      anchors: ["a", "b", "c"],
    });
  });

  it("parses a top-level block sequence of mappings (the scenario-file shape)", () => {
    const parsed = parseScenarioYaml(
      ["- id: p1", "  method: hover", "- id: p2", "  method: definition"].join("\n"),
    );
    expect(parsed).toEqual([
      { id: "p1", method: "hover" },
      { id: "p2", method: "definition" },
    ]);
  });

  it("parses a sequence item whose map carries nested block + flow values", () => {
    const parsed = parseScenarioYaml(
      [
        "- id: scenario-1",
        "  probes:",
        "    - id: probe-1",
        "      requiredDrivers: [rawLsp, tsgo]",
        "  thresholds:",
        "    flakeWindows: 2",
      ].join("\n"),
    );
    expect(parsed).toEqual([
      {
        id: "scenario-1",
        probes: [{ id: "probe-1", requiredDrivers: ["rawLsp", "tsgo"] }],
        thresholds: { flakeWindows: 2 },
      },
    ]);
  });

  it("parses double-quoted scalars with escapes (so `text` edit steps can carry newlines/quotes)", () => {
    expect(parseScenarioYaml('text: "import { Foo }\\nfrom \\"x\\""')).toEqual({
      text: 'import { Foo }\nfrom "x"',
    });
  });

  it("parses single-quoted scalars literally (doubled quote escapes one quote)", () => {
    expect(parseScenarioYaml("value: 'a''b'")).toEqual({ value: "a'b" });
    // A `#` inside quotes is literal text, never a comment.
    expect(parseScenarioYaml('value: "a # b"')).toEqual({ value: "a # b" });
  });

  it("strips full-line and trailing comments outside quotes", () => {
    const parsed = parseScenarioYaml(
      ["# leading comment", "id: foo # trailing comment", "fixture: bar"].join("\n"),
    );
    expect(parsed).toEqual({ id: "foo", fixture: "bar" });
  });

  it("treats CRLF and LF identically", () => {
    const lf = ["- id: p1", "  method: hover"].join("\n");
    const crlf = lf.replace(/\n/g, "\r\n");
    expect(parseScenarioYaml(crlf)).toEqual(parseScenarioYaml(lf));
  });

  it("returns null for an empty / comment-only document", () => {
    expect(parseScenarioYaml("")).toBeNull();
    expect(parseScenarioYaml("# only a comment\n")).toBeNull();
  });
});

describe("parseScenarioYaml — rejects malformed input (no silent rubber-stamp)", () => {
  it("throws YamlParseError on TAB indentation", () => {
    expect(() => parseScenarioYaml("a:\n\tb: 1")).toThrow(YamlParseError);
  });

  it("throws on an unterminated double-quoted scalar", () => {
    expect(() => parseScenarioYaml('a: "oops')).toThrow(YamlParseError);
  });

  it("throws on an unterminated single-quoted scalar", () => {
    expect(() => parseScenarioYaml("a: 'oops")).toThrow(YamlParseError);
  });

  it("throws on an unterminated flow sequence", () => {
    expect(() => parseScenarioYaml("a: [1, 2")).toThrow(YamlParseError);
  });

  it("throws on an unknown escape sequence", () => {
    expect(() => parseScenarioYaml('a: "\\q"')).toThrow(YamlParseError);
  });

  it("throws on a non-empty flow mapping (outside the supported subset)", () => {
    expect(() => parseScenarioYaml("a: {x: 1}")).toThrow(YamlParseError);
  });

  it("throws on a duplicate key within one mapping", () => {
    expect(() => parseScenarioYaml("a: 1\na: 2")).toThrow(YamlParseError);
  });

  // The supported subset nests by EXACTLY two spaces per level. A child block
  // indented by any other amount is a fault, never a silent accept — otherwise a
  // mis-indented document would parse to a different shape than it reads as.
  it("throws on a child over-indented past the two-space step (4 spaces under 0)", () => {
    expect(() => parseScenarioYaml("a:\n    b: 1")).toThrow(YamlParseError);
  });

  it("throws on a child indented three spaces (not the two-space step)", () => {
    expect(() => parseScenarioYaml("a:\n   b: 1")).toThrow(YamlParseError);
  });

  it("throws on a nested child whose step is inconsistent with its parent's", () => {
    // `thresholds`→`latency` steps by 2, but `latency`→`p95Ms` jumps by 3.
    expect(() => parseScenarioYaml("thresholds:\n  latency:\n     p95Ms: 1")).toThrow(
      YamlParseError,
    );
  });

  it("throws on an over-indented first child of a deeper mapping value", () => {
    // `b`'s value block lands three spaces deeper than `b` (a two-space step is required).
    expect(() => parseScenarioYaml("root:\n  a:\n    x: 1\n  b:\n   y: 2")).toThrow(YamlParseError);
  });

  it("throws on inconsistent sibling indentation within one mapping block", () => {
    // `c` dedents to a column matching no open level — it is neither `b`'s sibling
    // (indent 2) nor an ancestor's, so it is stray, not silently re-homed.
    expect(() => parseScenarioYaml("a:\n  b: 1\n c: 2")).toThrow(YamlParseError);
  });

  it("throws on a dedent that lands between two open levels", () => {
    // `d` at indent 3 matches none of the open levels (0, 2, 4).
    expect(() => parseScenarioYaml("a:\n  b:\n    c: 1\n   d: 2")).toThrow(YamlParseError);
  });

  it("throws on inconsistent dash indentation within one sequence block", () => {
    expect(() => parseScenarioYaml("items:\n  - a\n   - b")).toThrow(YamlParseError);
  });

  it("throws on a mapping line that has no `key:` separator", () => {
    expect(() => parseScenarioYaml("id: ok\njust-a-bare-scalar")).toThrow(YamlParseError);
  });

  it("reports a 1-based line number in the error message", () => {
    try {
      parseScenarioYaml("a: 1\nb: 2\n\tc: 3");
      expect.unreachable("should have thrown");
    } catch (err) {
      expect(err).toBeInstanceOf(YamlParseError);
      expect((err as YamlParseError).message).toMatch(/line 3/);
    }
  });
});
