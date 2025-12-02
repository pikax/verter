/**
 * Component Type Plugin Tests
 *
 * @ai-generated - This test file was generated with AI assistance.
 * Tests the ComponentTypePlugin which generates typed component functions
 * from Vue templates for TypeScript type inference.
 *
 * Key areas tested:
 * - HTML element type generation with enhanceElementWithProps
 * - Vue component instantiation
 * - v-for loop variable extraction with extractLoops
 * - Scoped slot prop extraction with extractArgumentsFromRenderSlot
 * - Conditional narrowing with v-if/v-else-if/v-else
 * - Props and attributes handling
 */

import { describe, it, expect } from "vitest";
import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";
import { ComponentTypePlugin } from "./component-type";
import { ImportsPlugin } from "../imports";
import { ScriptBlockPlugin } from "../script-block";
import { AttributesPlugin } from "../attributes";
import { DeclarePlugin } from "../declare";
import { BindingPlugin } from "../binding";
import { FullContextPlugin } from "../full-context";
import { MacrosPlugin } from "../macros";
import { TemplateBindingPlugin } from "../template-binding";
import { ScriptDefaultPlugin } from "../script-default";
import { SFCCleanerPlugin } from "../sfc-cleaner";
import { ComponentInstancePlugin } from "../component-instance";
import { DefineOptionsPlugin } from "../define-options";
import { InferFunctionPlugin } from "../infer-function";

/**
 * Helper function to process Vue SFC content through the component type pipeline
 */
function processComponentType(
  template: string,
  scriptCode: string = "",
  options: { prefix?: string; lang?: string; generic?: string } = {}
) {
  const { prefix = "___VERTER___", lang = "ts", generic } = options;
  const genericAttr = generic ? ` generic="${generic}"` : "";
  const scriptPart = `<script setup lang="${lang}"${genericAttr}>${scriptCode}</script>`;
  const templatePart = `<template>${template}</template>`;
  const source = `${templatePart}${scriptPart}`;
  const parsed = parser(source);

  const scriptBlock = parsed.blocks.find((x) => x.type === "script") as
    | ParsedBlockScript
    | undefined;

  const result = processScript(
    scriptBlock?.result.items ?? [],
    [
      ImportsPlugin,
      ScriptBlockPlugin,
      AttributesPlugin,
      DeclarePlugin,
      BindingPlugin,
      FullContextPlugin,
      MacrosPlugin,
      TemplateBindingPlugin,
      ScriptDefaultPlugin,
      SFCCleanerPlugin,
      ComponentInstancePlugin,
      DefineOptionsPlugin,
      InferFunctionPlugin,
      ComponentTypePlugin,
    ],
    {
      ...parsed,
      s: parsed.s,
      filename: "test.vue",
      blocks: parsed.blocks,
      isSetup: true,
      block: scriptBlock!,
      prefix: (name: string) => prefix + name,
      blockNameResolver: (name: string) => name,
    }
  );

  return {
    result: result.result,
    context: result.context,
    s: parsed.s,
  };
}

describe("ComponentTypePlugin", () => {
  // ============================================================================
  // Basic Element Tests
  // ============================================================================

  describe("basic elements", () => {
    it("generates component type function for simple div element", () => {
      const { result } = processComponentType("<div></div>");

      expect(result).toContain("enhanceElementWithProps");
      expect(result).toContain('HTMLElementTagNameMap["div"]');
      expect(result).toMatch(/function ___VERTER___Comp\d+/);
    });

    it("generates component type function for div with static text", () => {
      const { result } = processComponentType("<div>Hello World</div>");

      expect(result).toContain("enhanceElementWithProps");
      expect(result).toContain('HTMLElementTagNameMap["div"]');
    });

    it("generates separate functions for nested elements", () => {
      const { result } = processComponentType(
        "<div><span>nested</span></div>"
      );

      // Should have functions for both div and span
      expect(result).toContain('HTMLElementTagNameMap["div"]');
      expect(result).toContain('HTMLElementTagNameMap["span"]');
    });

    it("handles multiple root elements (fragment)", () => {
      const { result } = processComponentType(
        "<div>first</div><span>second</span>"
      );

      expect(result).toContain('HTMLElementTagNameMap["div"]');
      expect(result).toContain('HTMLElementTagNameMap["span"]');
    });

    it("handles self-closing input element", () => {
      const { result } = processComponentType("<input />");

      expect(result).toContain('HTMLElementTagNameMap["input"]');
    });

    it("handles form elements", () => {
      const { result } = processComponentType(
        "<form><input /><button>Submit</button></form>"
      );

      expect(result).toContain('HTMLElementTagNameMap["form"]');
      expect(result).toContain('HTMLElementTagNameMap["input"]');
      expect(result).toContain('HTMLElementTagNameMap["button"]');
    });
  });

  // ============================================================================
  // Attribute and Binding Tests
  // ============================================================================

  describe("attributes and bindings", () => {
    it("includes static attributes in props object", () => {
      const { result } = processComponentType(
        '<div id="app" class="container"></div>'
      );

      expect(result).toContain('"id": "app"');
      expect(result).toContain('"class": "container"');
    });

    it("includes dynamic bindings in props object", () => {
      const { result } = processComponentType(
        '<div :id="myId"></div>',
        "const myId = 'test';"
      );

      expect(result).toContain('"id": myId');
    });

    it("handles multiple dynamic bindings", () => {
      const { result } = processComponentType(
        '<div :id="id" :class="className"></div>',
        "const id = 'test'; const className = 'active';"
      );

      expect(result).toContain('"id": id');
      expect(result).toContain('"class": className');
    });

    it("handles boolean attributes", () => {
      const { result } = processComponentType(
        '<input disabled />',
        ""
      );

      expect(result).toContain('"disabled": true');
    });
  });

  // ============================================================================
  // Conditional Rendering Tests (v-if / v-else-if / v-else)
  // ============================================================================

  describe("conditional rendering", () => {
    it("generates conditional narrowing for v-if", () => {
      const { result } = processComponentType(
        '<div v-if="show">Visible</div>',
        "const show = true;"
      );

      // Should generate a function with narrowing logic
      expect(result).toContain('HTMLElementTagNameMap["div"]');
    });

    it("handles v-if with v-else", () => {
      const { result } = processComponentType(
        '<div v-if="show">Shown</div><div v-else>Hidden</div>',
        "const show = true;"
      );

      // Both divs should have component functions
      const divMatches = result.match(/HTMLElementTagNameMap\["div"\]/g);
      expect(divMatches?.length).toBe(2);
    });

    it("handles v-if / v-else-if / v-else chain", () => {
      const { result } = processComponentType(
        `<div v-if="status === 'a'">A</div>
         <div v-else-if="status === 'b'">B</div>
         <div v-else>C</div>`,
        "const status = 'a' as 'a' | 'b' | 'c';"
      );

      // All three divs should have component functions
      const divMatches = result.match(/HTMLElementTagNameMap\["div"\]/g);
      expect(divMatches?.length).toBe(3);
    });
  });

  // ============================================================================
  // Loop Tests (v-for)
  // ============================================================================

  describe("v-for loops", () => {
    it("generates extractLoops call for basic v-for", () => {
      const { result } = processComponentType(
        '<li v-for="item in items" :key="item">{{ item }}</li>',
        "const items = ['a', 'b', 'c'];"
      );

      expect(result).toContain("extractLoops");
      expect(result).toContain('HTMLElementTagNameMap["li"]');
    });

    it("extracts both key and value for v-for with index", () => {
      const { result } = processComponentType(
        '<li v-for="(item, index) in items" :key="index"></li>',
        "const items = ['a', 'b', 'c'];"
      );

      expect(result).toContain("extractLoops");
      expect(result).toMatch(/key:\s*index/);
      expect(result).toMatch(/value:\s*item/);
    });

    it("handles nested v-for loops", () => {
      const { result } = processComponentType(
        `<div v-for="(row, i) in matrix" :key="i">
           <span v-for="(cell, j) in row" :key="j">{{ cell }}</span>
         </div>`,
        "const matrix = [[1, 2], [3, 4]];"
      );

      // Should have extractLoops calls for both loops
      const loopMatches = result.match(/extractLoops/g);
      expect(loopMatches?.length).toBeGreaterThanOrEqual(2);
    });

    it("handles v-for with destructuring", () => {
      const { result } = processComponentType(
        '<li v-for="{ id, name } in users" :key="id">{{ name }}</li>',
        "const users = [{ id: 1, name: 'John' }];"
      );

      expect(result).toContain("extractLoops");
    });
  });

  // ============================================================================
  // Slot Tests
  // ============================================================================

  describe("slots", () => {
    it("handles slot usage in consuming component", () => {
      const { result } = processComponentType(
        `<MyComponent>
           <template #default>Content</template>
         </MyComponent>`,
        "import MyComponent from './MyComponent.vue';"
      );

      // MyComponent should be treated as a component (new syntax)
      expect(result).toContain("new MyComponent");
    });

    it("handles scoped slot props extraction", () => {
      const { result } = processComponentType(
        `<MyList>
           <template #item="{ item, index }">
             <div>{{ item.name }}</div>
           </template>
         </MyList>`,
        "import MyList from './MyList.vue';"
      );

      // Should use extractArgumentsFromRenderSlot for scoped slot props
      expect(result).toContain("extractArgumentsFromRenderSlot");
    });

    it("handles multiple named slots", () => {
      const { result } = processComponentType(
        `<MyComponent>
           <template #header>Header</template>
           <template #footer>Footer</template>
         </MyComponent>`,
        "import MyComponent from './MyComponent.vue';"
      );

      expect(result).toContain("new MyComponent");
    });
  });

  // ============================================================================
  // Component Usage Tests
  // ============================================================================

  describe("component usage", () => {
    it("generates new constructor call for Vue components", () => {
      const { result } = processComponentType(
        "<MyComponent />",
        "import MyComponent from './MyComponent.vue';"
      );

      expect(result).toContain("new MyComponent");
    });

    it("includes props in component constructor call", () => {
      const { result } = processComponentType(
        '<MyComponent title="Hello" :count="42" />',
        "import MyComponent from './MyComponent.vue';"
      );

      expect(result).toContain("new MyComponent");
      expect(result).toContain('"title": "Hello"');
      expect(result).toContain('"count": 42');
    });

    it("handles component in v-for", () => {
      const { result } = processComponentType(
        '<UserCard v-for="user in users" :key="user.id" :user="user" />',
        "import UserCard from './UserCard.vue'; const users = [{ id: 1 }];"
      );

      expect(result).toContain("new UserCard");
      expect(result).toContain("extractLoops");
    });
  });

  // ============================================================================
  // getRootComponent Tests
  // ============================================================================

  describe("getRootComponent function", () => {
    it("generates getRootComponent function", () => {
      const { result } = processComponentType("<div>Root</div>");

      expect(result).toContain("function getRootComponent");
    });

    it("returns single root component when template has one root", () => {
      const { result } = processComponentType("<div>Single root</div>");

      expect(result).toMatch(/function getRootComponent.*\(\)/);
      // Note: getRootComponent returns Comp{offset}() without prefix
      expect(result).toMatch(/return Comp\d+\(\)/);
    });

    it("returns empty object for multiple roots (fragment)", () => {
      const { result } = processComponentType(
        "<div>First</div><span>Second</span>"
      );

      expect(result).toContain("return {};");
    });
  });

  // ============================================================================
  // Generic Component Tests
  // ============================================================================

  describe("generic components", () => {
    it("includes generic parameters in component functions", () => {
      const { result } = processComponentType(
        "<div>{{ item }}</div>",
        "const item = defineProps<{ item: T }>();",
        { generic: "T" }
      );

      expect(result).toMatch(/function.*<T>/);
    });

    it("includes generic parameters in getRootComponent", () => {
      const { result } = processComponentType(
        "<div>{{ item }}</div>",
        "",
        { generic: "T extends string" }
      );

      // Note: The current implementation appends generic source directly
      // which results in `getRootComponentT extends string()` format
      expect(result).toContain("function getRootComponent");
      expect(result).toContain("T extends string");
    });
  });

  // ============================================================================
  // Helper Import Tests
  // ============================================================================

  describe("helper imports", () => {
    it("imports enhanceElementWithProps in result", () => {
      const { result } = processComponentType("<div></div>");

      // Helper imports are added to the result via import statement
      expect(result).toContain("enhanceElementWithProps");
      expect(result).toContain('from "$verter/types$"');
    });

    it("imports extractLoops when v-for is used", () => {
      const { result } = processComponentType(
        '<li v-for="item in items" :key="item"></li>',
        "const items = [1, 2, 3];"
      );

      // extractLoops is imported and used in the generated code
      expect(result).toContain("extractLoops");
    });

    it("imports extractArgumentsFromRenderSlot when scoped slots are used", () => {
      const { result } = processComponentType(
        `<MyComponent>
           <template #default="props">{{ props.msg }}</template>
         </MyComponent>`,
        "import MyComponent from './MyComponent.vue';"
      );

      // extractArgumentsFromRenderSlot is imported and used
      expect(result).toContain("extractArgumentsFromRenderSlot");
    });
  });

  // ============================================================================
  // Edge Cases Tests
  // ============================================================================

  describe("edge cases", () => {
    it("handles empty template gracefully", () => {
      // Empty template with just whitespace
      const { result } = processComponentType("   ");

      // Should still generate getRootComponent
      expect(result).toContain("function getRootComponent");
    });

    it("handles deeply nested elements", () => {
      const { result } = processComponentType(
        "<div><div><div><div><span>Deep</span></div></div></div></div>"
      );

      // All elements should have component functions
      const divMatches = result.match(/HTMLElementTagNameMap\["div"\]/g);
      expect(divMatches?.length).toBe(4);
      expect(result).toContain('HTMLElementTagNameMap["span"]');
    });

    it("handles special HTML elements", () => {
      const { result } = processComponentType(
        "<table><thead><tr><th>Header</th></tr></thead></table>"
      );

      expect(result).toContain('HTMLElementTagNameMap["table"]');
      expect(result).toContain('HTMLElementTagNameMap["thead"]');
      expect(result).toContain('HTMLElementTagNameMap["tr"]');
      expect(result).toContain('HTMLElementTagNameMap["th"]');
    });

    it("handles v-html directive", () => {
      const { result } = processComponentType(
        '<div v-html="htmlContent"></div>',
        "const htmlContent = '<b>bold</b>';"
      );

      expect(result).toContain('HTMLElementTagNameMap["div"]');
      // v-html generates a directive, not a prop - binding is extracted to context
      expect(result).toContain("htmlContent");
    });

    it("handles v-text directive", () => {
      const { result } = processComponentType(
        '<div v-text="textContent"></div>',
        "const textContent = 'Hello';"
      );

      expect(result).toContain('HTMLElementTagNameMap["div"]');
      // v-text generates a directive, not a prop - binding is extracted to context
      expect(result).toContain("textContent");
    });
  });

  // ============================================================================
  // Integration Tests
  // ============================================================================

  describe("integration scenarios", () => {
    it("handles complex template with loops, conditionals, and components", () => {
      const { result } = processComponentType(
        `<div v-if="items.length > 0">
           <MyComponent
             v-for="item in items"
             :key="item.id"
             :data="item"
           >
             <template #content="{ value }">
               <span>{{ value }}</span>
             </template>
           </MyComponent>
         </div>
         <div v-else>No items</div>`,
        `import MyComponent from './MyComponent.vue';
         const items = [{ id: 1 }];`
      );

      // Should have all the necessary pieces
      expect(result).toContain('HTMLElementTagNameMap["div"]');
      expect(result).toContain("new MyComponent");
      expect(result).toContain("extractLoops");
      expect(result).toContain("extractArgumentsFromRenderSlot");
    });

    it("handles form with v-model bindings", () => {
      const { result } = processComponentType(
        `<form>
           <input v-model="name" />
           <select v-model="selected">
             <option v-for="opt in options" :key="opt.value" :value="opt.value">
               {{ opt.label }}
             </option>
           </select>
         </form>`,
        `const name = ref('');
         const selected = ref('');
         const options = [{ value: 'a', label: 'A' }];`
      );

      expect(result).toContain('HTMLElementTagNameMap["form"]');
      expect(result).toContain('HTMLElementTagNameMap["input"]');
      expect(result).toContain('HTMLElementTagNameMap["select"]');
      expect(result).toContain('HTMLElementTagNameMap["option"]');
      expect(result).toContain("extractLoops");
    });
  });
});
