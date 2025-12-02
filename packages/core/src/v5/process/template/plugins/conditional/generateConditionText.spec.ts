/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for generateConditionText function.
 * - Tests condition text generation for v-if/v-else-if/v-else chains
 * - Validates correct sibling negation handling
 * - Catches the bug where multiple v-else-if causes duplicate narrowing
 */
import { MagicString } from "@vue/compiler-sfc";
import { generateConditionText } from "./conditional";
import { TemplateCondition, TemplateTypes } from "../../../../parser/template/types";
import { DirectiveNode, NodeTypes } from "@vue/compiler-core";

describe("generateConditionText", () => {
  /**
   * Helper to create a mock TemplateCondition for testing.
   * The MagicString must already contain the condition text at the specified offset.
   */
  function createConditionAtOffset(
    startOffset: number,
    endOffset: number,
    siblings: TemplateCondition[] = []
  ): TemplateCondition {
    return {
      type: TemplateTypes.Condition,
      node: {
        type: NodeTypes.DIRECTIVE,
        name: "if",
        loc: {
          start: { offset: startOffset, line: 1, column: startOffset },
          end: { offset: endOffset, line: 1, column: endOffset },
          source: "",
        },
      } as DirectiveNode,
      bindings: [],
      element: {} as any,
      parent: {} as any,
      context: {},
      siblings,
    };
  }

  /**
   * Helper to set up a chain of conditions simulating v-if/v-else-if/v-else.
   * Creates a MagicString with all condition texts concatenated, and returns
   * conditions with proper offsets and sibling references.
   */
  function createConditionChain(
    conditionTexts: string[]
  ): { conditions: TemplateCondition[]; s: MagicString } {
    // Build the source string with all conditions
    const source = conditionTexts.join("");
    const s = new MagicString(source);
    
    const conditions: TemplateCondition[] = [];
    let offset = 0;

    for (let i = 0; i < conditionTexts.length; i++) {
      const text = conditionTexts[i];
      const startOffset = offset;
      const endOffset = offset + text.length;
      
      // Each condition's siblings are all previous conditions
      const siblings = conditions.slice();
      const condition = createConditionAtOffset(startOffset, endOffset, siblings);
      conditions.push(condition);
      
      offset = endOffset;
    }

    return { conditions, s };
  }

  describe("single condition (v-if only)", () => {
    it("should return the condition text for a single v-if", () => {
      const source = "(isVisible)";
      const s = new MagicString(source);
      const condition = createConditionAtOffset(0, source.length, []);

      const result = generateConditionText([condition], s);

      expect(result).toBe("(isVisible)");
    });

    it("should handle condition without parentheses", () => {
      const source = "isVisible";
      const s = new MagicString(source);
      const condition = createConditionAtOffset(0, source.length, []);

      const result = generateConditionText([condition], s);

      expect(result).toBe("isVisible");
    });

    it("should handle typeof conditions", () => {
      const source = "(typeof test === 'string')";
      const s = new MagicString(source);
      const condition = createConditionAtOffset(0, source.length, []);

      const result = generateConditionText([condition], s);

      expect(result).toBe("(typeof test === 'string')");
    });
  });

  describe("v-if with v-else-if", () => {
    it("should negate the v-if condition for v-else-if", () => {
      const { conditions, s } = createConditionChain([
        "(isTypeA)",
        "(isTypeB)",
      ]);

      // When generating text for the v-else-if condition (index 1),
      // we pass only that condition but it has siblings
      const result = generateConditionText([conditions[1]], s);

      // Should be: !(isTypeA) && (isTypeB)
      // The v-else-if is true when: NOT the previous condition AND this condition
      expect(result).toBe("!((isTypeA)) && (isTypeB)");
    });

    it("should handle typeof narrowing correctly", () => {
      const { conditions, s } = createConditionChain([
        "(typeof test === 'object')",
        "(typeof test === 'string')",
      ]);

      const result = generateConditionText([conditions[1]], s);

      expect(result).toBe("!((typeof test === 'object')) && (typeof test === 'string')");
    });
  });

  describe("v-if with v-else", () => {
    it("should only negate the v-if for v-else", () => {
      const { conditions, s } = createConditionChain([
        "(isTypeA)",
        "", // v-else has no condition expression
      ]);

      const result = generateConditionText([conditions[1]], s);

      // v-else should just negate the v-if
      expect(result).toBe("!((isTypeA))");
    });
  });

  describe("v-if with multiple v-else-if", () => {
    it("should correctly chain two v-else-if conditions", () => {
      const { conditions, s } = createConditionChain([
        "(isTypeA)",
        "(isTypeB)",
        "(isTypeC)",
      ]);

      // For the third condition (second v-else-if)
      const result = generateConditionText([conditions[2]], s);

      // Bug: Currently this produces duplicate conditions
      // Expected: !(isTypeA) && !(isTypeB) && (isTypeC)
      // Or equivalently: !((isTypeA) || (isTypeB)) && (isTypeC)
      
      // The condition should mean: 
      // "NOT first condition AND NOT second condition AND this condition"
      expect(result).toContain("(isTypeC)");
      
      // Should NOT contain duplicate (isTypeA) checks
      const matchesA = result.match(/isTypeA/g) || [];
      expect(matchesA.length).toBe(1);
      
      // Should NOT contain duplicate (isTypeB) checks  
      const matchesB = result.match(/isTypeB/g) || [];
      expect(matchesB.length).toBe(1);
    });

    it("should correctly handle three v-else-if conditions", () => {
      const { conditions, s } = createConditionChain([
        "(typeof test === 'string')",
        "(typeof test === 'number')",
        "(typeof test === 'boolean')",
        "(typeof test === 'object')",
      ]);

      // For the fourth condition (third v-else-if)
      const result = generateConditionText([conditions[3]], s);

      // Should contain each condition exactly once
      const matchesString = result.match(/typeof test === 'string'/g) || [];
      const matchesNumber = result.match(/typeof test === 'number'/g) || [];
      const matchesBoolean = result.match(/typeof test === 'boolean'/g) || [];
      const matchesObject = result.match(/typeof test === 'object'/g) || [];

      expect(matchesString.length).toBe(1);
      expect(matchesNumber.length).toBe(1);
      expect(matchesBoolean.length).toBe(1);
      expect(matchesObject.length).toBe(1);
    });
  });

  describe("v-if with multiple v-else-if and v-else", () => {
    it("should correctly handle v-if, two v-else-if, and v-else", () => {
      const { conditions, s } = createConditionChain([
        "(isTypeA)",
        "(isTypeB)",
        "(isTypeC)",
        "", // v-else
      ]);

      // For the v-else (fourth condition)
      const result = generateConditionText([conditions[3]], s);

      // v-else should negate all previous conditions
      // Expected: !(isTypeA) && !(isTypeB) && !(isTypeC)
      
      // Should contain each condition exactly once
      const matchesA = result.match(/isTypeA/g) || [];
      const matchesB = result.match(/isTypeB/g) || [];
      const matchesC = result.match(/isTypeC/g) || [];

      expect(matchesA.length).toBe(1);
      expect(matchesB.length).toBe(1);
      expect(matchesC.length).toBe(1);
    });
  });

  describe("nested conditions", () => {
    it("should handle nested v-if inside v-if", () => {
      const source = "(isOuter)(isInner)";
      const s = new MagicString(source);
      
      // Outer v-if
      const outerCondition = createConditionAtOffset(0, 9, []);
      
      // Inner v-if (no siblings, but has parent context)
      const innerCondition = createConditionAtOffset(9, 18, []);

      // When inside a nested v-if, we pass both conditions
      const result = generateConditionText([outerCondition, innerCondition], s);

      expect(result).toBe("(isOuter) && (isInner)");
    });

    it("should handle nested v-else-if inside v-if", () => {
      const source = "(isOuter)(isInnerA)(isInnerB)";
      const s = new MagicString(source);
      
      // Outer v-if
      const outerCondition = createConditionAtOffset(0, 9, []);
      
      // Inner v-if
      const innerIf = createConditionAtOffset(9, 19, []);
      
      // Inner v-else-if (has innerIf as sibling)
      const innerElseIf = createConditionAtOffset(19, 29, [innerIf]);

      // When inside nested v-else-if, we pass outer + inner else-if
      const result = generateConditionText([outerCondition, innerElseIf], s);

      // Should be: (isOuter) && !(isInnerA) && (isInnerB)
      expect(result).toContain("(isOuter)");
      expect(result).toContain("(isInnerA)");
      expect(result).toContain("(isInnerB)");
    });
  });

  describe("compound conditions with && and ||", () => {
    // @ai-generated - Tests compound conditions are wrapped in parentheses for correct operator precedence
    it("should wrap compound && conditions in parentheses", () => {
      const { conditions, s } = createConditionChain([
        "isLoggedIn && hasPermission",
        "isLoggedIn && !hasPermission",
      ]);

      // For the v-else-if condition
      const result = generateConditionText([conditions[1]], s);

      // The compound condition should be wrapped in parentheses
      // Expected: !((isLoggedIn && hasPermission)) && (isLoggedIn && !hasPermission)
      expect(result).toContain("(isLoggedIn && hasPermission)");
      expect(result).toContain("(isLoggedIn && !hasPermission)");
    });

    // @ai-generated - Tests compound || conditions are wrapped in parentheses
    it("should wrap compound || conditions in parentheses", () => {
      const { conditions, s } = createConditionChain([
        "isAdmin || isModerator",
        "isUser",
      ]);

      const result = generateConditionText([conditions[1]], s);

      // Expected: !((isAdmin || isModerator)) && isUser
      expect(result).toContain("(isAdmin || isModerator)");
    });

    // @ai-generated - Tests v-else with compound conditions
    it("should correctly handle v-else with compound conditions", () => {
      const { conditions, s } = createConditionChain([
        "isLoggedIn && hasPermission",
        "isLoggedIn && !hasPermission",
        "", // v-else
      ]);

      const result = generateConditionText([conditions[2]], s);

      // v-else negates both previous compound conditions
      // Expected: !((isLoggedIn && hasPermission)) && !((isLoggedIn && !hasPermission))
      expect(result).toContain("(isLoggedIn && hasPermission)");
      expect(result).toContain("(isLoggedIn && !hasPermission)");
      
      // Both should be negated with pattern "!(("
      const negationMatches = result.match(/!\(/g) || [];
      expect(negationMatches.length).toBe(2);
    });

    // @ai-generated - Tests that simple conditions without && or || are not double-wrapped
    it("should not double-wrap conditions already in parentheses without && or ||", () => {
      const { conditions, s } = createConditionChain([
        "(isTypeA)",
        "(isTypeB)",
      ]);

      const result = generateConditionText([conditions[1]], s);

      // Simple conditions in parentheses are not additionally wrapped by wrapIfNeeded
      // since they don't contain && or || at the top level
      // Result: !((isTypeA)) && (isTypeB)
      expect(result).toBe("!((isTypeA)) && (isTypeB)");
    });
  });

  describe("condition text parsing", () => {
    it("should strip 'if' prefix from condition", () => {
      const source = "if(isVisible){";
      const s = new MagicString(source);
      
      const condition = createConditionAtOffset(0, source.length, []);

      const result = generateConditionText([condition], s);
      
      expect(result).toBe("(isVisible)");
      expect(result).not.toContain("if");
    });

    it("should strip 'else if' prefix from condition", () => {
      const source = "else if(isVisible){";
      const s = new MagicString(source);
      
      const condition = createConditionAtOffset(0, source.length, []);

      const result = generateConditionText([condition], s);
      
      expect(result).toBe("(isVisible)");
      expect(result).not.toContain("else");
    });

    it("should strip 'else' prefix from condition", () => {
      const source = "else{";
      const s = new MagicString(source);
      
      const condition = createConditionAtOffset(0, source.length, []);

      const result = generateConditionText([condition], s);
      
      // v-else has no expression, should return empty or just the negation of siblings
      expect(result).toBe("");
    });
  });
});

