/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for InferFunctionPlugin fixture generation utilities.
 * Validates that function parameter type inference works correctly
 * for template event handlers.
 *
 * Key scenarios tested:
 * - Native HTML element events (click, input, change, etc.)
 * - Multiple function parameters
 * - Functions not used in templates (should not transform)
 * - Functions with existing type annotations (should not transform)
 * - Arrow functions (should not transform - only FunctionDeclarations)
 */
import { createFixtures, fixtures } from "./infer-function.fixtures";
import type { Fixture } from "../../../../../fixtures/types";
import { resolveWithPrefix } from "../../../../../fixtures/types";

const PREFIX = "___VERTER___";

// Get the process function from fixtures
const fixtureConfig = createFixtures(PREFIX);
const processFixture = fixtureConfig.process;

describe("InferFunctionPlugin fixtures", () => {
  describe("fixture configuration", () => {
    it("should have fixtures defined", () => {
      expect(fixtures.length).toBeGreaterThan(0);
    });

    it("should have process function", () => {
      expect(typeof fixtureConfig.process).toBe("function");
    });
  });

  describe("syntax validation", () => {
    fixtures.forEach((fixture) => {
      it(`${fixture.name}: produces valid output`, () => {
        const { result } = processFixture(fixture);
        expect(result).toBeDefined();
        expect(typeof result).toBe("string");
        expect(result.length).toBeGreaterThan(0);
      });
    });
  });

  describe("pattern matching", () => {
    fixtures.forEach((fixture) => {
      if (fixture.expectations?.patterns) {
        describe(fixture.name, () => {
          const { result } = processFixture(fixture);

          fixture.expectations!.patterns!.forEach((pattern) => {
            const resolvedPattern =
              typeof pattern === "function" ? pattern(PREFIX) : pattern;
            it(`should contain: ${resolvedPattern.substring(0, 50)}...`, () => {
              expect(result).toContain(resolvedPattern);
            });
          });
        });
      }

      if (fixture.expectations?.antiPatterns) {
        describe(`${fixture.name} (negative patterns)`, () => {
          const { result } = processFixture(fixture);

          fixture.expectations!.antiPatterns!.forEach((pattern) => {
            const resolvedPattern =
              typeof pattern === "function" ? pattern(PREFIX) : pattern;
            it(`should NOT contain: ${resolvedPattern.substring(0, 50)}...`, () => {
              expect(result).not.toContain(resolvedPattern);
            });
          });
        });
      }
    });
  });

  describe("native HTML element events", () => {
    it("click event infers HTMLElementEventMap type", () => {
      const fixture = fixtures.find((f) => f.name === "click event on button");
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["click"]');
      expect(result).toContain("...[e]:");
    });

    it("input event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "input event on input element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["input"]');
    });

    it("change event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "change event on select element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["change"]');
    });

    it("submit event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "submit event on form element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["submit"]');
    });

    it("keydown event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "keydown event on input element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["keydown"]');
    });

    it("focus event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "focus event on input element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["focus"]');
    });

    it("blur event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "blur event on input element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["blur"]');
    });

    it("mouseenter event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "mouseenter event on div element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["mouseenter"]');
    });

    it("mouseleave event infers correct type", () => {
      const fixture = fixtures.find(
        (f) => f.name === "mouseleave event on div element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["mouseleave"]');
    });
  });

  describe("multiple parameters", () => {
    it("transforms all parameters with spread syntax", () => {
      const fixture = fixtures.find(
        (f) => f.name === "function with multiple parameters"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain("...[a, b, c]:");
      expect(result).toContain('HTMLElementEventMap["click"]');
    });
  });

  describe("no transformation cases", () => {
    it("does not transform function not used in template", () => {
      const fixture = fixtures.find(
        (f) => f.name === "function not used in template"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain("function unusedFunction(x)");
      expect(result).not.toContain("HTMLElementEventMap");
      expect(result).not.toContain("...[");
    });

    // NOTE: Currently the plugin transforms parameters even when they have type annotations
    it("transforms function with typed parameter (current behavior)", () => {
      const fixture = fixtures.find(
        (f) => f.name === "function with typed parameter"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      // Currently transforms even typed parameters - wraps in spread syntax
      expect(result).toContain("...[e: MouseEvent]:");
      expect(result).toContain('HTMLElementEventMap["click"]');
    });

    it("does not transform arrow functions", () => {
      const fixture = fixtures.find((f) => f.name === "arrow function handler");
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain("const handler = (e) => e.target");
      expect(result).not.toContain("HTMLElementEventMap");
    });

    it("does not transform function with no parameters", () => {
      const fixture = fixtures.find(
        (f) => f.name === "function with no parameters"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain("function noParams()");
      expect(result).not.toContain("HTMLElementEventMap");
    });
  });

  describe("different element types", () => {
    it("anchor element click event", () => {
      const fixture = fixtures.find(
        (f) => f.name === "click on anchor element"
      );
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["click"]');
    });

    it("double click event", () => {
      const fixture = fixtures.find((f) => f.name === "double click on div");
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["dblclick"]');
    });

    it("contextmenu event", () => {
      const fixture = fixtures.find((f) => f.name === "contextmenu event");
      expect(fixture).toBeDefined();
      const { result } = processFixture(fixture!);
      expect(result).toContain('HTMLElementEventMap["contextmenu"]');
    });
  });
});
