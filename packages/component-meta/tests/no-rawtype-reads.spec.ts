/**
 * Static check (W7.3): `prop.rawType` / `event.rawType` / `slot.rawType` /
 * `expose.rawType` / `binding.rawType` reads inside semantic-decision
 * functions are forbidden in the compat checker after the typed-IR-only
 * cutover. The only allowed reads are display-passthrough copies into
 * `PropertyMeta.type` text, marked with the exact-line annotation
 * `// rawType-allowlist: display-passthrough`.
 *
 * This test parses `packages/component-meta/src/compat/checker.ts`,
 * walks the TypeScript AST for every `PropertyAccessExpression` whose
 * property name is `rawType`, identifies the smallest enclosing
 * function declaration / expression / method / arrow, and asserts that
 * either:
 *  - the enclosing function's name does NOT match the forbidden pattern,
 *  - or the read carries the allowlist annotation on the same or
 *    immediately-preceding source line.
 *
 * Pre-W7.3 the file had ~57 `rawType` reads scattered across `buildCompat*`,
 * `looksLike*`, `normalize*`, `prefer*`, `shouldPrefer*`, `applyRawType*`,
 * `repairOpaque*`, and other semantic-decision helpers. The W7.3 cutover
 * deletes the dead helpers and migrates the surviving gates to descriptor
 * kind-tag matching; the allowlist captures only the display-passthrough
 * site(s) — by W8.2 the allowlist reaches zero.
 *
 * Discriminating property: the test FAILS against the pre-W7.3 tree
 * because the forbidden-pattern enclosing functions contained dozens of
 * unannotated reads. The test PASSES against the post-W7.3 tree because
 * every surviving read is either outside the forbidden pattern or
 * carries the exact-line annotation.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import ts from "typescript";

const __dirname = dirname(fileURLToPath(import.meta.url));
const CHECKER_PATH = resolve(__dirname, "../src/compat/checker.ts");

/** Function-name patterns whose bodies MUST NOT read `*.rawType`. */
const FORBIDDEN_FUNCTION_PATTERN =
  /^(buildCompat|looksLike|extract|normalize|split|strip|prefer|shouldPrefer|compat[A-Za-z0-9_$]+ToString|repairOpaque|applyRawType)/;

/** Container identifiers whose `.rawType` member reads are tracked. */
const TRACKED_OBJECT_NAMES = new Set(["prop", "event", "slot", "expose", "binding", "exposed"]);

/** Exact-line annotation marking the display-passthrough exception. */
const ALLOWLIST_ANNOTATION = "rawType-allowlist: display-passthrough";

interface RawTypeReadSite {
  /** 1-based line number. */
  line: number;
  /** Name of the enclosing function (or "<top-level>"). */
  enclosingFunctionName: string;
  /** Identifier on the left of `.rawType` (e.g. "prop", "binding"). */
  objectName: string;
  /** Raw source text of the line, for diagnostic display. */
  lineText: string;
  /** Whether the read is annotated as a display-passthrough exception. */
  annotated: boolean;
}

function getEnclosingFunctionName(node: ts.Node, source: ts.SourceFile): string {
  let current: ts.Node | undefined = node.parent;
  while (current) {
    if (ts.isFunctionDeclaration(current) && current.name) {
      return current.name.text;
    }
    if (ts.isMethodDeclaration(current) && current.name) {
      return ts.isIdentifier(current.name) ? current.name.text : current.name.getText(source);
    }
    if (
      ts.isVariableDeclaration(current) &&
      current.name &&
      ts.isIdentifier(current.name) &&
      current.initializer &&
      (ts.isFunctionExpression(current.initializer) || ts.isArrowFunction(current.initializer))
    ) {
      return current.name.text;
    }
    current = current.parent;
  }
  return "<top-level>";
}

function lineHasAnnotation(lines: readonly string[], lineNumber1Based: number): boolean {
  // Check current line and the line immediately above (for trailing-comment
  // and preceding-comment annotation styles).
  const here = lines[lineNumber1Based - 1] ?? "";
  const previous = lines[lineNumber1Based - 2] ?? "";
  return here.includes(ALLOWLIST_ANNOTATION) || previous.includes(ALLOWLIST_ANNOTATION);
}

function collectRawTypeReads(source: ts.SourceFile, lines: readonly string[]): RawTypeReadSite[] {
  const sites: RawTypeReadSite[] = [];

  const visit = (node: ts.Node): void => {
    if (
      ts.isPropertyAccessExpression(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === "rawType" &&
      ts.isIdentifier(node.expression) &&
      TRACKED_OBJECT_NAMES.has(node.expression.text)
    ) {
      const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
      const lineNumber1Based = line + 1;
      const enclosingFunctionName = getEnclosingFunctionName(node, source);
      sites.push({
        line: lineNumber1Based,
        enclosingFunctionName,
        objectName: node.expression.text,
        lineText: (lines[line] ?? "").trim(),
        annotated: lineHasAnnotation(lines, lineNumber1Based),
      });
    }
    ts.forEachChild(node, visit);
  };

  visit(source);
  return sites;
}

describe("W7.3: no `*.rawType` reads inside semantic-decision functions in compat/checker.ts", () => {
  const sourceText = readFileSync(CHECKER_PATH, "utf8");
  const lines = sourceText.split(/\r?\n/);
  const source = ts.createSourceFile(
    CHECKER_PATH,
    sourceText,
    ts.ScriptTarget.Latest,
    /* setParentNodes */ true,
    ts.ScriptKind.TS,
  );

  const sites = collectRawTypeReads(source, lines);

  it("the AST walker finds at least one rawType read site (sanity check)", () => {
    // If this fails the AST walker itself is broken — every healthy tree has
    // at least the display-passthrough read inside `preferredCompatPropTypeText`.
    expect(sites.length).toBeGreaterThan(0);
  });

  it("every rawType read inside a forbidden-pattern function is allowlisted", () => {
    const violations = sites.filter(
      (site) => FORBIDDEN_FUNCTION_PATTERN.test(site.enclosingFunctionName) && !site.annotated,
    );

    if (violations.length > 0) {
      const formatted = violations
        .map(
          (v) =>
            `  line ${v.line} in ${v.enclosingFunctionName}: ${v.objectName}.rawType — ${v.lineText}`,
        )
        .join("\n");
      throw new Error(
        `Found ${violations.length} unallowlisted rawType reads inside semantic-decision functions:\n${formatted}\n\n` +
          `Either:\n` +
          `  • migrate the gate to descriptor kind-tag matching (preferred), or\n` +
          `  • if this is a true display-passthrough copy into PropertyMeta.type,\n` +
          `    add an exact-line comment: // ${ALLOWLIST_ANNOTATION}`,
      );
    }

    expect(violations).toEqual([]);
  });

  it("the allowlist captures only display-passthrough sites — final form (W7.3 floor)", () => {
    // Post-W7.3 the allowlist must hold only display-passthrough reads. Each
    // allowlisted entry is paired here with the function that owns it; the
    // list is the cap for W7.3 and shrinks to zero by W8.2 once the compat
    // layer drops `prop.rawType` entirely.
    const annotatedInsideForbidden = sites.filter(
      (site) => FORBIDDEN_FUNCTION_PATTERN.test(site.enclosingFunctionName) && site.annotated,
    );
    const allowlistedFunctions = new Set(
      annotatedInsideForbidden.map((s) => s.enclosingFunctionName),
    );

    // The display-passthrough lives inside `preferredCompatPropTypeText`.
    // Any other forbidden-pattern function in the allowlist would be a
    // regression — re-open the function and migrate to typed kind matching.
    expect([...allowlistedFunctions].sort()).toEqual(["preferredCompatPropTypeText"]);
  });
});

/**
 * Discriminating runtime test: with a `PropMeta` whose `rawType` is a decoy
 * that would break semantics if used to drive a projection, the compat layer
 * must produce kind-tag-driven output identical to the typed descriptor's
 * intent, NOT the decoy rawType.
 *
 * Pre-W7.3 the `buildCompatNumberishPropMeta` / `buildCompatBooleanishPropMeta`
 * / `buildCompatReferrerPolicyPropMeta` / etc. gates accepted projection when
 * `rawType.trim() === "Numberish"` (etc.) — so a decoy `rawType: "Numberish"`
 * with a `prop.type` that did NOT carry a `Numberish` ref would still
 * project. Post-W7.3 only the descriptor's kind-tag drives the gate.
 */
import { mapPropMeta } from "../src/compat/checker.js";
import { primitive, ref, union, literal } from "@verter/type-ir";
import type { TypeDescriptor } from "@verter/type-ir";
import type { PropMeta } from "../src/types.js";

function makeProp(overrides: Partial<PropMeta> & { type: TypeDescriptor }): PropMeta {
  return {
    name: overrides.name ?? "value",
    type: overrides.type,
    required: overrides.required ?? false,
    hasDefault: overrides.hasDefault ?? false,
    rawType: overrides.rawType,
    tags: overrides.tags ?? [],
    description: overrides.description,
    default: overrides.default,
  };
}

describe("W7.3: kind-tag wins over decoy rawType in buildCompat* projections", () => {
  it("buildCompatNumberishPropMeta DECLINES when descriptor lacks Numberish ref, even if rawType decoy says 'Numberish'", () => {
    const prop = makeProp({
      type: primitive("string"),
      // Pre-W7.3 decoy: rawType text says "Numberish" so the OLD gate
      // `strippedRawType === "Numberish"` returned true and projected the
      // Numberish compat shape (enum schema with ["number", "string"]).
      // Post-W7.3 the gate is descriptor-only — the projection declines and
      // the schema must NOT be the Numberish enum.
      rawType: "Numberish",
      required: true,
    });
    const result = mapPropMeta(prop);
    // Post-cutover: descriptor IS `primitive("string")`, NOT Numberish.
    // The compat projection MUST decline. The display-passthrough rawType
    // is fine for `.type` text (that's the W7.3 contract), but the SCHEMA
    // must come from descriptor (`"string"`), NOT the Numberish enum.
    expect(JSON.stringify(result.schema)).not.toBe(
      JSON.stringify({
        kind: "enum",
        type: "Numberish",
        schema: ["number", "string"],
      }),
    );
    // Pre-W7.3 the schema would have been the Numberish enum (3 entries).
    // Post-W7.3 the schema is the descriptor's `"string"` (primitive).
    expect(result.schema).toBe("string");
  });

  it("buildCompatReferrerPolicyPropMeta DECLINES when descriptor lacks HTMLAttributeReferrerPolicy ref, even if rawType decoy says so", () => {
    const prop = makeProp({
      type: primitive("string"),
      // Decoy: pre-W7.3 the gate accepted rawType text. Post-W7.3 descriptor wins.
      rawType: "HTMLAttributeReferrerPolicy",
      required: true,
    });
    const result = mapPropMeta(prop);
    // The .type field receives the display-passthrough rawType ("HTMLAttribute..."),
    // but the SCHEMA must NOT be the referrer-policy enum projection.
    expect(JSON.stringify(result.schema)).not.toContain('"no-referrer"');
    expect(JSON.stringify(result.schema)).not.toContain('"origin"');
    expect(result.schema).toBe("string");
  });

  it("buildCompatNuxtLinkToPropMeta DECLINES when descriptor lacks NuxtLinkProps['to'] indexed-access, even if rawType decoy matches the text", () => {
    const prop = makeProp({
      type: primitive("number"),
      // Decoy: pre-W7.3 the gate accepted rawType text equality.
      rawType: 'NuxtLinkProps["to"]',
      required: true,
    });
    const result = mapPropMeta(prop);
    // Post-cutover the projection declines — descriptor is `number`, not the
    // structural indexedAccess / RouteLocationRaw ref. Final type must NOT
    // be the NuxtLinkProps `string | St | vt` projection.
    expect(result.type).not.toContain("St");
    expect(result.type).not.toContain("vt");
  });

  it("buildCompatHtmlButtonTypePropMeta DECLINES when descriptor is not the structural button-type union, even if rawType decoy says it is", () => {
    const prop = makeProp({
      name: "type",
      type: primitive("string"),
      // Decoy: pre-W7.3 the gate accepted rawType `ButtonHTMLAttributes["type"]`.
      rawType: 'ButtonHTMLAttributes["type"]',
      required: true,
    });
    const result = mapPropMeta(prop);
    // Post-cutover: descriptor is plain `string`, NOT the structural button
    // union. The result must NOT be the button-type compat projection.
    expect(result.type).not.toBe('"button" | "submit" | "reset"');
  });

  it("buildCompatStringBrandUnionPropMeta DECLINES when descriptor lacks the `string & {}` branded arm, even if rawType decoy contains `(string & {})`", () => {
    const prop = makeProp({
      name: "rel",
      // Descriptor: bare string primitive, NO branded `intersection(string,{})` arm.
      type: primitive("string"),
      // Decoy: rawType text contains `(string & {})`. Pre-W7.3 this satisfied
      // the gate and the projection ran. Post-W7.3 the gate is descriptor-only.
      rawType: '"a" | "b" | (string & {})',
      required: true,
    });
    const result = mapPropMeta(prop);
    // Post-cutover: the brand-union projection declines. The SCHEMA must
    // not be the projection's structured enum (which would contain entries
    // for the decoy literals). The descriptor schema is plain `"string"`.
    expect(result.schema).toBe("string");
  });

  it("buildCompatBooleanishPropMeta DECLINES when descriptor lacks the Booleanish ref, even if rawType decoy says 'Booleanish'", () => {
    const prop = makeProp({
      type: ref("CustomEnum"),
      // Decoy: pre-W7.3 the gate stripped undefined and compared to "Booleanish".
      rawType: "Booleanish",
      required: true,
    });
    const result = mapPropMeta(prop);
    // Post-cutover: descriptor is `ref("CustomEnum")`, NOT Booleanish. The
    // result must NOT be the Booleanish compat projection.
    expect(result.type).not.toBe("Booleanish");
    expect(JSON.stringify(result.schema)).not.toContain('"true"');
  });

  it("buildCompatPrefetchOnPropMeta DECLINES when descriptor lacks the literal `visibility`/`interaction` union arms, even if rawType decoy includes `Partial<{ visibility: boolean; interaction: boolean; }>`", () => {
    const prop = makeProp({
      type: primitive("string"),
      // Decoy: pre-W7.3 the gate was `rawType.includes("Partial<{")` etc.
      rawType:
        '"visibility" | "interaction" | Partial<{ visibility: boolean; interaction: boolean; }>',
      required: true,
    });
    const result = mapPropMeta(prop);
    // Post-cutover: descriptor is `primitive("string")`, NOT the structural
    // literal-union form. The prefetch projection declines and the result
    // must NOT carry the projection's specific schema entries.
    expect(JSON.stringify(result.schema)).not.toContain("Partial<{ visibility");
  });

  it("buildCompatAnyPropMeta KIND-tag-DRIVEN: projects 'any' when descriptor is `any`, regardless of rawType decoy", () => {
    const prop = makeProp({
      // Descriptor structurally IS `any` union with undefined.
      type: union([primitive("any"), primitive("undefined")]),
      // Decoy: rawType text says `string | undefined` (no `any`).
      rawType: "string | undefined",
      required: false,
    });
    const result = mapPropMeta(prop);
    // Post-cutover: descriptor wins → `any` projection.
    expect(result.type).toBe("any");
    expect(result.schema).toBe("any");
  });

  it("preferredCompatPropTypeText flows rawType ONLY when descriptor is not bare-ref / indexed-access — decoy rawType cannot override structural ref", () => {
    const prop = makeProp({
      // Descriptor: bare Ref(Foo). The structural form is meaningful — descriptor wins.
      type: ref("Foo"),
      // Decoy: rawType is wildly different text.
      rawType: "DECOY_THAT_SHOULD_NOT_APPEAR_IN_OUTPUT",
      required: true,
    });
    const result = mapPropMeta(prop);
    // Post-cutover: descriptor is bare ref → descriptor text wins.
    expect(result.type).toBe("Foo");
    expect(result.type).not.toContain("DECOY");
  });

  it("preferredCompatPropTypeText flows rawType when descriptor is expanded (object/primitive/etc.)", () => {
    const prop = makeProp({
      // Descriptor: expanded object — descriptor renders structurally as `{ id: number; }`.
      type: {
        kind: "object",
        properties: [{ name: "id", type: primitive("number"), optional: false }],
      },
      // Source-level alias the user wrote.
      rawType: 'Pick<FullUser, "id">',
      required: true,
    });
    const result = mapPropMeta(prop);
    // Display passthrough: rawType carries the alias name.
    expect(result.type).toBe('Pick<FullUser, "id">');
  });
});

/** Compile-time placeholder import to keep `literal` referenced if the test
 *  body shrinks; vitest's TypeScript loader treats unused imports as an
 *  error in some configurations. */
void literal;
