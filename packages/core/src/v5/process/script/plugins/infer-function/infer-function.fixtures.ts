/**
 * InferFunctionPlugin Type Fixtures
 *
 * This file defines fixtures for testing the function parameter type inference.
 * The plugin infers types for function parameters based on their usage in templates
 * as event handlers.
 *
 * It is used by:
 * - scripts/fixtures.generator.ts: To generate __generated__/*.ts files
 * - infer-function.fixtures.spec.ts: For automated testing
 *
 * Run `pnpm generate:fixtures` to regenerate the fixture files.
 */

import { MagicString } from "@vue/compiler-sfc";
import { parser, TemplateTypes } from "../../../../parser";
import { ParsedBlockScript, ParsedBlockTemplate } from "../../../../parser/types";
import { processScript } from "../../script";
import { InferFunctionPlugin } from "./index.js";
import { MacrosPlugin } from "../macros";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import type {
  Fixture,
  FixtureConfig,
  ProcessResult,
} from "../../../../../fixtures/types";

/**
 * Process Vue SFC content through the infer-function plugin pipeline
 */
function processInferFunction(
  code: string,
  prefix: string,
  lang = "ts",
  generic?: string,
  template?: string
): ProcessResult {
  const genericAttr = generic ? ` generic="${generic}"` : "";
  const prepend = `<script setup lang="${lang}"${genericAttr}>`;
  const templateContent = template ? `<template>${template}</template>` : "";
  const source = `${templateContent}${prepend}${code}</script>`;
  const parsed = parser(source);

  const s = new MagicString(source);

  const scriptBlock = parsed.blocks.find(
    (x) => x.type === "script"
  ) as ParsedBlockScript;

  const templateBlock = parsed.blocks.find(
    (x) => x.type === "template"
  ) as ParsedBlockTemplate;

  const result = processScript(
    scriptBlock.result.items,
    [InferFunctionPlugin, BindingPlugin, MacrosPlugin, ScriptBlockPlugin],
    {
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      isSetup: true,
      block: scriptBlock,
      generic: parsed.generic,
      isAsync: parsed.isAsync,
      prefix: (name: string) => prefix + name,
      blockNameResolver: (name: string) => name,
      templateBindings:
        templateBlock?.result?.items.filter(
          (x) => x.type === TemplateTypes.Binding
        ) ?? [],
    }
  );

  const sourcemap = s
    .generateMap({ hires: true, includeContent: true })
    .toUrl();

  return {
    result: result.result,
    context: result.context,
    sourcemap,
  };
}

/**
 * Fixture definitions for infer-function plugin
 */
const fixtures: Fixture[] = [
  // ============================================================================
  // Native HTML Element Events
  // ============================================================================
  {
    name: "click event on button",
    code: `function handleClick(e) { return e }`,
    template: `<button @click="handleClick">Click me</button>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["click"]`,
        "...[e]:",
      ],
      typeTests: [
        {
          target: "handleClick",
          kind: "function",
          description: "Function should have inferred event parameter type",
          notAny: true,
        },
      ],
    },
  },
  {
    name: "input event on input element",
    code: `function handleInput(event) { console.log(event.target) }`,
    template: `<input @input="handleInput" />`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["input"]`,
        "...[event]:",
      ],
    },
  },
  {
    name: "change event on select element",
    code: `function handleChange(e) { }`,
    template: `<select @change="handleChange"></select>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["change"]`,
      ],
    },
  },
  {
    name: "submit event on form element",
    code: `function handleSubmit(e) { e.preventDefault() }`,
    template: `<form @submit="handleSubmit"></form>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["submit"]`,
      ],
    },
  },
  {
    name: "keydown event on input element",
    code: `function handleKeydown(e) { if (e.key === 'Enter') {} }`,
    template: `<input @keydown="handleKeydown" />`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["keydown"]`,
      ],
    },
  },
  {
    name: "focus event on input element",
    code: `function handleFocus(e) { }`,
    template: `<input @focus="handleFocus" />`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["focus"]`,
      ],
    },
  },
  {
    name: "blur event on input element",
    code: `function handleBlur(e) { }`,
    template: `<input @blur="handleBlur" />`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["blur"]`,
      ],
    },
  },
  {
    name: "mouseenter event on div element",
    code: `function handleMouseEnter(e) { }`,
    template: `<div @mouseenter="handleMouseEnter"></div>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["mouseenter"]`,
      ],
    },
  },
  {
    name: "mouseleave event on div element",
    code: `function handleMouseLeave(e) { }`,
    template: `<div @mouseleave="handleMouseLeave"></div>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["mouseleave"]`,
      ],
    },
  },

  // ============================================================================
  // Multiple Parameters
  // ============================================================================
  {
    name: "function with multiple parameters",
    code: `function handler(a, b, c) { return [a, b, c] }`,
    template: `<div @click="handler"></div>`,
    expectations: {
      patterns: [
        "...[a, b, c]:",
        `HTMLElementEventMap["click"]`,
      ],
    },
  },

  // ============================================================================
  // No Template Binding (should not transform)
  // ============================================================================
  {
    name: "function not used in template",
    code: `function unusedFunction(x) { return x * 2 }`,
    template: `<div>No event binding here</div>`,
    expectations: {
      patterns: [
        "function unusedFunction(x)",
      ],
      antiPatterns: [
        "HTMLElementEventMap",
        "...[",
      ],
    },
  },

  // ============================================================================
  // Functions with existing types
  // NOTE: Currently the plugin transforms parameters even when they have type annotations.
  // This is because the OXC parser returns "Identifier" for the parameter name itself,
  // even when the parameter has a type annotation.
  // ============================================================================
  {
    name: "function with typed parameter",
    code: `function handler(e: MouseEvent) { return e.clientX }`,
    template: `<div @click="handler"></div>`,
    expectations: {
      patterns: [
        // Currently transforms even typed parameters - adds spread syntax around existing type
        "...[e: MouseEvent]:",
        `HTMLElementEventMap["click"]`,
      ],
    },
  },

  // ============================================================================
  // Arrow functions (should not transform - only FunctionDeclarations)
  // ============================================================================
  {
    name: "arrow function handler",
    code: `const handler = (e) => e.target`,
    template: `<div @click="handler"></div>`,
    expectations: {
      patterns: [
        "const handler = (e) => e.target",
      ],
      antiPatterns: [
        "HTMLElementEventMap",
      ],
    },
  },

  // ============================================================================
  // Different element types
  // ============================================================================
  {
    name: "click on anchor element",
    code: `function handleAnchorClick(e) { e.preventDefault() }`,
    template: `<a href="#" @click="handleAnchorClick">Link</a>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["click"]`,
      ],
    },
  },
  {
    name: "double click on div",
    code: `function handleDblClick(e) { }`,
    template: `<div @dblclick="handleDblClick"></div>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["dblclick"]`,
      ],
    },
  },
  {
    name: "contextmenu event",
    code: `function handleContextMenu(e) { e.preventDefault() }`,
    template: `<div @contextmenu="handleContextMenu"></div>`,
    expectations: {
      patterns: [
        `HTMLElementEventMap["contextmenu"]`,
      ],
    },
  },

  // ============================================================================
  // Edge cases
  // ============================================================================
  {
    name: "function with no parameters",
    code: `function noParams() { console.log('clicked') }`,
    template: `<div @click="noParams"></div>`,
    expectations: {
      patterns: [
        "function noParams()",
      ],
      antiPatterns: [
        "HTMLElementEventMap",
      ],
    },
  },

  // ============================================================================
  // Vue Component Events
  // ============================================================================
  {
    name: "component event with change handler",
    code: `import MyComp from './MyComp.vue'\nfunction handleChange(e) { return e }`,
    template: `<MyComp @change="handleChange" />`,
    expectations: {
      patterns: [
        "Parameters<",
        '["onChange"]',
        "$props",
        "...[e]:",
      ],
      antiPatterns: [
        "HTMLElementEventMap",
      ],
    },
  },
  {
    name: "component event with click handler",
    code: `import MyButton from './MyButton.vue'\nfunction onClick(e) { return e }`,
    template: `<MyButton @click="onClick" />`,
    expectations: {
      patterns: [
        "Parameters<",
        '["onClick"]',
        "$props",
      ],
      antiPatterns: [
        "HTMLElementEventMap",
      ],
    },
  },
  {
    name: "component event with custom event",
    code: `import CustomSelect from './CustomSelect.vue'\nfunction onSelect(item) { console.log(item) }`,
    template: `<CustomSelect @select="onSelect" />`,
    expectations: {
      patterns: [
        "Parameters<",
        '["onSelect"]',
        "$props",
      ],
    },
  },
  {
    name: "component event with update event",
    code: `import DataTable from './DataTable.vue'\nfunction handleUpdate(data) { console.log(data) }`,
    template: `<DataTable @update="handleUpdate" />`,
    expectations: {
      patterns: [
        "Parameters<",
        '["onUpdate"]',
        "$props",
      ],
    },
  },
];

/**
 * Creates the fixture configuration for the infer-function plugin
 */
export function createFixtures(prefix: string): FixtureConfig {
  return {
    fixtures,
    prefix,
    process: (fixture: Fixture) => {
      return processInferFunction(
        fixture.code,
        prefix,
        fixture.lang || "ts",
        fixture.generic,
        fixture.template
      );
    },
  };
}

export { fixtures };
