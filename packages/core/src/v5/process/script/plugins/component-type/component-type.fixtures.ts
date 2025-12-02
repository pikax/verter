/**
 * Component Type Plugin Fixtures
 *
 * @ai-generated - This fixture file was generated with AI assistance.
 * - Tests component type generation from Vue templates
 * - Covers basic elements, conditionals, loops, slots, and complex nesting
 * - Validates enhanceElementWithProps output for various template patterns
 *
 * This file defines fixtures for testing template-to-component-type transformations.
 * The ComponentTypePlugin generates a function that returns the correct component type
 * based on the template content.
 *
 * Example transformation:
 * <template><div></div></template>
 * Will create:
 * function Comp() {
 *   return enhanceElementWithProps(HTMLDivElement, {})
 * }
 *
 * Run `pnpm generate:fixtures` to regenerate the fixture files.
 */

import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import {
  ParsedBlockScript,
  ParsedBlockTemplate,
} from "../../../../parser/types";
import { processScript } from "../../script";
import { MacrosPlugin } from "../macros";
import { TemplateBindingPlugin } from "../template-binding";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { ImportsPlugin } from "../imports";
import { ComponentTypePlugin } from "./";
import type {
  Fixture,
  FixtureConfig,
  ProcessResult,
} from "../../../../../fixtures/types";
import { SFCCleanerPlugin } from "../sfc-cleaner";
import { AttributesPlugin } from "../attributes";
import { DeclarePlugin } from "../declare";
import { FullContextPlugin } from "../full-context";
import { ScriptDefaultPlugin } from "../script-default";
import { ComponentInstancePlugin } from "../component-instance";
import { DefineOptionsPlugin } from "../define-options";
import { InferFunctionPlugin } from "../infer-function";

/**
 * Process Vue SFC content through the component type pipeline
 */
function processComponentType(
  code: string,
  prefix: string,
  lang = "ts",
  generic?: string,
  template?: string
): ProcessResult {
  const genericAttr = generic ? ` generic="${generic}"` : "";
  const scriptPart = code
    ? `<script setup lang="${lang}"${genericAttr}>${code}</script>`
    : "";
  const templatePart = template ? `<template>${template}</template>` : "";
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

  const sourcemap = parsed.s
    .generateMap({ hires: true, includeContent: true })
    .toUrl();

  return {
    result: result.result,
    context: result.context,
    sourcemap,
  };
}

// ============================================================================
// Basic Element Fixtures
// ============================================================================

const basicElementFixtures: Fixture[] = [
  {
    name: "simple div element",
    code: "",
    template: "<div></div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "div with text content",
    code: "",
    template: "<div>Hello World</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "nested div elements",
    code: "",
    template: "<div><div><span>nested</span></div></div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "multiple root elements (fragment)",
    code: "",
    template: "<div>first</div><span>second</span><p>third</p>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "self-closing elements",
    code: "",
    template: "<input /><br /><hr />",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "input element with type",
    code: "",
    template: '<input type="text" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "form element with inputs",
    code: "",
    template: `
      <form>
        <input type="text" />
        <input type="email" />
        <textarea></textarea>
        <select><option>A</option></select>
        <button type="submit">Submit</button>
      </form>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "semantic HTML elements",
    code: "",
    template: `
      <header>
        <nav><a href="/">Home</a></nav>
      </header>
      <main>
        <article>
          <section><h1>Title</h1><p>Content</p></section>
        </article>
      </main>
      <footer><small>Copyright</small></footer>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "SVG element",
    code: "",
    template: `
      <svg width="100" height="100">
        <circle cx="50" cy="50" r="40" fill="red" />
        <rect x="10" y="10" width="30" height="30" />
      </svg>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "table element with full structure",
    code: "",
    template: `
      <table>
        <thead><tr><th>Header</th></tr></thead>
        <tbody><tr><td>Cell</td></tr></tbody>
        <tfoot><tr><td>Footer</td></tr></tfoot>
      </table>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Attribute and Binding Fixtures
// ============================================================================

const attributeFixtures: Fixture[] = [
  {
    name: "static attributes",
    code: "",
    template: '<div id="app" class="container" data-test="value"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "dynamic attribute binding",
    code: "const id = 'myId';",
    template: '<div :id="id"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "dynamic class binding - object syntax",
    code: "const isActive = true; const hasError = false;",
    template: '<div :class="{ active: isActive, error: hasError }"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "dynamic class binding - array syntax",
    code: "const activeClass = 'active'; const errorClass = 'error';",
    template: '<div :class="[activeClass, errorClass]"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "dynamic style binding - object syntax",
    code: "const color = 'red'; const fontSize = '14px';",
    template: '<div :style="{ color: color, fontSize: fontSize }"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-bind spread",
    code: "const attrs = { id: 'app', class: 'container' };",
    template: '<div v-bind="attrs"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "boolean attributes",
    code: "const isDisabled = true; const isReadonly = false;",
    template: '<input :disabled="isDisabled" :readonly="isReadonly" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Interpolation and Expression Fixtures
// ============================================================================

const expressionFixtures: Fixture[] = [
  {
    name: "simple interpolation",
    code: "const message = 'Hello';",
    template: "<div>{{ message }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "interpolation with expressions",
    code: "const count = 5;",
    template: "<div>{{ count + 1 }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "ternary expression in template",
    code: "const isLoggedIn = true;",
    template: "<div>{{ isLoggedIn ? 'Welcome' : 'Please login' }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "template literal interpolation",
    code: "const name = 'John';",
    template: "<div>{{ `Hello, ${name}!` }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "method call in interpolation",
    code: "const name = 'john'; function capitalize(s: string) { return s.toUpperCase(); }",
    template: "<div>{{ capitalize(name) }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "optional chaining in template",
    code: "const user: { name?: string } | null = { name: 'John' };",
    template: "<div>{{ user?.name }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "nullish coalescing in template",
    code: "const value: string | null = null;",
    template: "<div>{{ value ?? 'default' }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "array methods in interpolation",
    code: "const items = ['a', 'b', 'c'];",
    template: "<div>{{ items.map(i => i.toUpperCase()).join(', ') }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Conditional Rendering Fixtures (v-if / v-else-if / v-else)
// ============================================================================

const conditionalFixtures: Fixture[] = [
  {
    name: "simple v-if",
    code: "const show = true;",
    template: '<div v-if="show">Visible</div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-if with v-else",
    code: "const show = true;",
    template: '<div v-if="show">Shown</div><div v-else>Hidden</div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-if / v-else-if / v-else chain",
    code: "const status = 'active' as 'pending' | 'active' | 'completed';",
    template: `
      <div v-if="status === 'pending'">Pending</div>
      <div v-else-if="status === 'active'">Active</div>
      <div v-else>Completed</div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "multiple v-else-if conditions",
    code: "const role: 'admin' | 'moderator' | 'user' | 'guest' = 'admin';",
    template: `
      <div v-if="role === 'admin'">Admin Panel</div>
      <div v-else-if="role === 'moderator'">Moderator Panel</div>
      <div v-else-if="role === 'user'">User Dashboard</div>
      <div v-else>Guest View</div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-if with compound conditions",
    code: "import { ref } from 'vue';\nconst isLoggedIn = ref(true); const hasPermission = ref(true);",
    template: `
      <div v-if="isLoggedIn && hasPermission">Full Access</div>
      <div v-else-if="isLoggedIn && !hasPermission">Limited Access</div>
      <div v-else>No Access</div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-if with OR conditions",
    code: "const role = 'admin';",
    template:
      "<div v-if=\"role === 'admin' || role === 'moderator'\">Elevated</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-if on template element",
    code: "const show = true;",
    template: `
      <template v-if="show">
        <header>Header</header>
        <main>Content</main>
        <footer>Footer</footer>
      </template>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "nested v-if conditions",
    code: "const outer = true; const inner = true;",
    template: `
      <div v-if="outer">
        <span v-if="inner">Both true</span>
        <span v-else>Inner false</span>
      </div>
      <div v-else>Outer false</div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-if with array length check",
    code: "const items: string[] = ['a', 'b'];",
    template: `
      <ul v-if="items.length > 0">
        <li>Has items</li>
      </ul>
      <p v-else>No items</p>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-if with nullish check",
    code: "const error: Error | null = null; const data: { value: number } | undefined = { value: 42 };",
    template: `
      <div v-if="error">Error: {{ error.message }}</div>
      <div v-else-if="data">Data: {{ data.value }}</div>
      <div v-else>Loading...</div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-show directive",
    code: "const visible = true;",
    template: '<div v-show="visible">Toggle visibility</div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Loop Fixtures (v-for)
// ============================================================================

const loopFixtures: Fixture[] = [
  {
    name: "basic v-for with array",
    code: "const items = ['a', 'b', 'c'];",
    template: '<li v-for="item in items" :key="item">{{ item }}</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with index",
    code: "const items = ['a', 'b', 'c'];",
    template:
      '<li v-for="(item, index) in items" :key="index">{{ index }}: {{ item }}</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with object iteration",
    code: "const obj: Record<string, number> = { a: 1, b: 2, c: 3 };",
    template:
      '<div v-for="(value, key) in obj" :key="key">{{ key }}: {{ value }}</div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with object - value key index",
    code: "const obj: Record<string, string> = { first: 'a', second: 'b' };",
    template:
      '<div v-for="(value, key, index) in obj" :key="key">{{ index }}. {{ key }}: {{ value }}</div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with destructuring",
    code: `
interface User { id: number; name: string; email: string; }
const users: User[] = [{ id: 1, name: 'John', email: 'john@test.com' }];
    `,
    template:
      '<li v-for="{ id, name, email } in users" :key="id">{{ name }} ({{ email }})</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with destructuring and index",
    code: `
interface Item { id: number; name: string; }
const items: Item[] = [{ id: 1, name: 'First' }];
    `,
    template:
      '<li v-for="({ id, name }, index) in items" :key="id">#{{ index }}: {{ name }}</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with nested destructuring",
    code: `
interface User { id: number; roles: string[]; }
const users: User[] = [{ id: 1, roles: ['admin', 'user'] }];
    `,
    template:
      '<li v-for="{ id, roles: [primaryRole] } in users" :key="id">Primary: {{ primaryRole }}</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with range",
    code: "",
    template: '<span v-for="n in 10" :key="n">{{ n }}</span>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "nested v-for",
    code: "const matrix = [[1, 2], [3, 4], [5, 6]];",
    template: `
      <div v-for="(row, rowIdx) in matrix" :key="rowIdx">
        <span v-for="(cell, colIdx) in row" :key="colIdx">[{{ cell }}]</span>
      </div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for on template element",
    code: `
interface Item { id: number; name: string; children?: { id: number; name: string }[]; }
const items: Item[] = [{ id: 1, name: 'Parent', children: [{ id: 11, name: 'Child' }] }];
    `,
    template: `
      <template v-for="item in items" :key="item.id">
        <dt>{{ item.name }}</dt>
        <dd v-if="item.children" v-for="child in item.children" :key="child.id">{{ child.name }}</dd>
      </template>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with v-if inside",
    code: `
interface User { id: number; name: string; active: boolean; }
const users: User[] = [{ id: 1, name: 'John', active: true }];
    `,
    template: `
      <template v-for="user in users" :key="user.id">
        <li v-if="user.active">{{ user.name }} [Active]</li>
      </template>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with method in source",
    code: `
const items = [1, 2, 3, 4, 5];
function getActiveItems(items: number[]) { return items.filter(i => i > 2); }
    `,
    template:
      '<li v-for="item in getActiveItems(items)" :key="item">{{ item }}</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-for with computed expression",
    code: "const items = [1, 2, 3, 4, 5];",
    template:
      '<li v-for="item in items.filter(i => i % 2 === 0)" :key="item">{{ item }}</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Slot Fixtures
// ============================================================================

const slotFixtures: Fixture[] = [
  {
    name: "default slot",
    code: "",
    template: "<slot></slot>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "named slot",
    code: "",
    template: '<slot name="header"></slot>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "multiple named slots",
    code: "",
    template: `
      <header><slot name="header"></slot></header>
      <main><slot></slot></main>
      <footer><slot name="footer"></slot></footer>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "scoped slot with props",
    code: "const item = 'example'; const index = 0;",
    template: '<slot :item="item" :index="index"></slot>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "scoped slot with complex props",
    code: `
interface Item { id: number; name: string; }
const items: Item[] = [{ id: 1, name: 'First' }];
    `,
    template: `
      <div v-for="(item, index) in items" :key="item.id">
        <slot name="item" :item="item" :index="index" :isLast="index === items.length - 1"></slot>
      </div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "slot with fallback content",
    code: "",
    template: "<slot>Default fallback content</slot>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "dynamic slot name",
    code: "const slotName = 'header';",
    template: '<slot :name="slotName"></slot>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "slot used in consuming component - default",
    code: "import MyComponent from './MyComponent.vue';",
    template: `
      <MyComponent>
        <template #default>Default content</template>
      </MyComponent>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "slot used in consuming component - named",
    code: "import MyComponent from './MyComponent.vue';",
    template: `
      <MyComponent>
        <template #header>Header content</template>
        <template #footer>Footer content</template>
      </MyComponent>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "slot used with scoped props",
    code: "import MyList from './MyList.vue';",
    template: `
      <MyList>
        <template #item="{ item, index }">
          <div>{{ index }}: {{ item.name }}</div>
        </template>
      </MyList>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "slot with destructured scoped props",
    code: "import DataTable from './DataTable.vue';",
    template: `
      <DataTable>
        <template #row="{ row, index, isSelected }">
          <tr :class="{ selected: isSelected }">
            <td>{{ index }}</td>
            <td>{{ row.name }}</td>
          </tr>
        </template>
      </DataTable>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Event Handling Fixtures
// ============================================================================

const eventFixtures: Fixture[] = [
  {
    name: "inline event handler",
    code: "const count = ref(0);",
    template: '<button @click="count++">Increment</button>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "method event handler",
    code: "function handleClick(e: MouseEvent) { console.log(e); }",
    template: '<button @click="handleClick">Click</button>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "event handler with argument",
    code: "function handleClick(msg: string, e: MouseEvent) { console.log(msg, e); }",
    template: "<button @click=\"handleClick('hello', $event)\">Click</button>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "event modifiers",
    code: "function handleClick() {}",
    template: `
      <button @click.stop="handleClick">Stop</button>
      <button @click.prevent="handleClick">Prevent</button>
      <button @click.stop.prevent="handleClick">Both</button>
      <button @click.once="handleClick">Once</button>
      <button @click.passive="handleClick">Passive</button>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "keyboard event modifiers",
    code: "function handleKey(e: KeyboardEvent) {}",
    template: `
      <input @keydown.enter="handleKey" />
      <input @keydown.ctrl.enter="handleKey" />
      <input @keydown.alt.shift.s="handleKey" />
      <input @keydown.esc="handleKey" />
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "mouse button modifiers",
    code: "function handleClick() {}",
    template: `
      <button @click.left="handleClick">Left</button>
      <button @click.right="handleClick">Right</button>
      <button @click.middle="handleClick">Middle</button>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "form events",
    code: `
function handleSubmit(e: Event) { e.preventDefault(); }
function handleInput(e: Event) {}
    `,
    template: `
      <form @submit.prevent="handleSubmit">
        <input @input="handleInput" @change="handleInput" />
        <button type="submit">Submit</button>
      </form>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// v-model Fixtures
// ============================================================================

const vModelFixtures: Fixture[] = [
  {
    name: "basic v-model on input",
    code: "const text = ref('');",
    template: '<input v-model="text" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-model on textarea",
    code: "const content = ref('');",
    template: '<textarea v-model="content"></textarea>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-model on select",
    code: "const selected = ref('');",
    template: `
      <select v-model="selected">
        <option value="a">A</option>
        <option value="b">B</option>
      </select>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-model on checkbox",
    code: "const checked = ref(false);",
    template: '<input type="checkbox" v-model="checked" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-model on radio",
    code: "const picked = ref('');",
    template: `
      <input type="radio" value="a" v-model="picked" />
      <input type="radio" value="b" v-model="picked" />
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-model with modifiers",
    code: "const text = ref(''); const num = ref(0);",
    template: `
      <input v-model.trim="text" />
      <input v-model.number="num" />
      <input v-model.lazy="text" />
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-model on component",
    code: "import CustomInput from './CustomInput.vue'; const value = ref('');",
    template: '<CustomInput v-model="value" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-model with custom name on component",
    code: "import CustomInput from './CustomInput.vue'; const title = ref('');",
    template: '<CustomInput v-model:title="title" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "multiple v-models on component",
    code: `
import UserForm from './UserForm.vue';
const firstName = ref('');
const lastName = ref('');
    `,
    template:
      '<UserForm v-model:first-name="firstName" v-model:last-name="lastName" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Component Usage Fixtures
// ============================================================================

const componentFixtures: Fixture[] = [
  {
    name: "basic component usage",
    code: "import MyComponent from './MyComponent.vue';",
    template: "<MyComponent />",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "component with props",
    code: "import MyComponent from './MyComponent.vue';",
    template: '<MyComponent title="Hello" :count="42" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "component with events",
    code: `
import MyComponent from './MyComponent.vue';
function handleUpdate(value: string) {}
    `,
    template: '<MyComponent @update="handleUpdate" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "component with v-bind spread",
    code: `
import MyComponent from './MyComponent.vue';
const props = { title: 'Hello', count: 42 };
    `,
    template: '<MyComponent v-bind="props" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "component in v-for",
    code: `
import UserCard from './UserCard.vue';
interface User { id: number; name: string; }
const users: User[] = [{ id: 1, name: 'John' }];
    `,
    template: '<UserCard v-for="user in users" :key="user.id" :user="user" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "dynamic component",
    code: `
import { shallowRef, type Component } from 'vue';
import CompA from './CompA.vue';
import CompB from './CompB.vue';
const current = shallowRef<Component>(CompA);
    `,
    template: '<component :is="current" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "dynamic component with string name",
    code: "const componentName = 'MyComponent';",
    template: '<component :is="componentName" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "async component",
    code: "const AsyncComp = defineAsyncComponent(() => import('./Heavy.vue'));",
    template: "<AsyncComp />",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Built-in Components Fixtures
// ============================================================================

const builtinFixtures: Fixture[] = [
  {
    name: "Transition component",
    code: "const show = ref(true);",
    template: `
      <Transition name="fade">
        <div v-if="show">Content</div>
      </Transition>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "TransitionGroup component",
    code: "const items = ref(['a', 'b', 'c']);",
    template: `
      <TransitionGroup name="list" tag="ul">
        <li v-for="item in items" :key="item">{{ item }}</li>
      </TransitionGroup>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "KeepAlive component",
    code: `
const current = ref('CompA');
    `,
    template: `
      <KeepAlive>
        <component :is="current" />
      </KeepAlive>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "KeepAlive with include/exclude",
    code: "const current = ref('CompA');",
    template: `
      <KeepAlive :include="['CompA', 'CompB']" :exclude="['CompC']" :max="10">
        <component :is="current" />
      </KeepAlive>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "Teleport component",
    code: "const showModal = ref(false);",
    template: `
      <Teleport to="body">
        <div v-if="showModal" class="modal">Modal content</div>
      </Teleport>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "Suspense component",
    code: "const AsyncComp = defineAsyncComponent(() => import('./Async.vue'));",
    template: `
      <Suspense>
        <template #default>
          <AsyncComp />
        </template>
        <template #fallback>
          <div>Loading...</div>
        </template>
      </Suspense>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Template Refs Fixtures
// ============================================================================

const refFixtures: Fixture[] = [
  {
    name: "basic template ref",
    code: "const inputRef = ref<HTMLInputElement | null>(null);",
    template: '<input ref="inputRef" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "multiple template refs",
    code: `
const inputRef = ref<HTMLInputElement | null>(null);
const buttonRef = ref<HTMLButtonElement | null>(null);
    `,
    template: `
      <input ref="inputRef" />
      <button ref="buttonRef">Click</button>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "template ref in v-for",
    code: "const itemRefs = ref<HTMLLIElement[]>([]);",
    template:
      '<li v-for="(item, i) in items" :key="i" :ref="el => itemRefs[i] = el">{{ item }}</li>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "component ref",
    code: `
import ChildComp from './ChildComp.vue';
const childRef = ref<InstanceType<typeof ChildComp> | null>(null);
    `,
    template: '<ChildComp ref="childRef" />',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "useTemplateRef",
    code: "const buttonRef = useTemplateRef<HTMLButtonElement>('myButton');",
    template: '<button ref="myButton">Click</button>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Complex/Real-world Fixtures
// ============================================================================

const complexFixtures: Fixture[] = [
  {
    name: "todo list component",
    code: `
interface Todo {
  id: number;
  text: string;
  completed: boolean;
}
const todos = ref<Todo[]>([]);
const newTodo = ref('');
function addTodo() {}
function removeTodo(id: number) {}
function toggleTodo(id: number) {}
    `,
    template: `
      <div class="todo-app">
        <h1>Todo List</h1>
        <form @submit.prevent="addTodo">
          <input v-model="newTodo" placeholder="Add todo" />
          <button type="submit">Add</button>
        </form>
        <ul v-if="todos.length">
          <li v-for="todo in todos" :key="todo.id" :class="{ completed: todo.completed }">
            <input type="checkbox" :checked="todo.completed" @change="toggleTodo(todo.id)" />
            <span>{{ todo.text }}</span>
            <button @click="removeTodo(todo.id)">×</button>
          </li>
        </ul>
        <p v-else>No todos yet!</p>
      </div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "data table component",
    code: `
interface Column<T> {
  key: keyof T;
  label: string;
  sortable?: boolean;
}
interface User {
  id: number;
  name: string;
  email: string;
  role: string;
}
const columns: Column<User>[] = [
  { key: 'name', label: 'Name', sortable: true },
  { key: 'email', label: 'Email', sortable: true },
  { key: 'role', label: 'Role' },
];
const users = ref<User[]>([]);
const sortKey = ref<keyof User>('name');
const sortOrder = ref<'asc' | 'desc'>('asc');
function sort(key: keyof User) {}
    `,
    template: `
      <table>
        <thead>
          <tr>
            <th v-for="col in columns" :key="col.key" @click="col.sortable && sort(col.key)">
              {{ col.label }}
              <span v-if="col.sortable && sortKey === col.key">
                {{ sortOrder === 'asc' ? '↑' : '↓' }}
              </span>
            </th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="user in users" :key="user.id">
            <td v-for="col in columns" :key="col.key">{{ user[col.key] }}</td>
            <td>
              <slot name="actions" :user="user">
                <button>Edit</button>
                <button>Delete</button>
              </slot>
            </td>
          </tr>
        </tbody>
      </table>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "modal dialog component",
    code: `
const props = defineProps<{
  open: boolean;
  title: string;
  closable?: boolean;
}>();
const emit = defineEmits<{
  close: [];
  confirm: [];
}>();
function close() { emit('close'); }
function confirm() { emit('confirm'); close(); }
    `,
    template: `
      <Teleport to="body">
        <Transition name="modal">
          <div v-if="props.open" class="modal-backdrop" @click.self="closable && close()">
            <div class="modal-content">
              <header class="modal-header">
                <h2>{{ title }}</h2>
                <button v-if="closable" @click="close" class="close-btn">×</button>
              </header>
              <main class="modal-body">
                <slot></slot>
              </main>
              <footer class="modal-footer">
                <slot name="footer">
                  <button @click="close">Cancel</button>
                  <button @click="confirm">Confirm</button>
                </slot>
              </footer>
            </div>
          </div>
        </Transition>
      </Teleport>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "tab component",
    code: `
interface Tab {
  id: string;
  label: string;
  disabled?: boolean;
}
const props = defineProps<{
  tabs: Tab[];
  modelValue: string;
}>();
const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();
function selectTab(id: string) {
  emit('update:modelValue', id);
}
    `,
    template: `
      <div class="tabs">
        <div class="tab-list" role="tablist">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            role="tab"
            :aria-selected="modelValue === tab.id"
            :disabled="tab.disabled"
            :class="{ active: modelValue === tab.id }"
            @click="!tab.disabled && selectTab(tab.id)"
          >
            <slot :name="'tab-' + tab.id" :tab="tab">
              {{ tab.label }}
            </slot>
          </button>
        </div>
        <div class="tab-panels">
          <div
            v-for="tab in tabs"
            :key="tab.id"
            v-show="modelValue === tab.id"
            role="tabpanel"
          >
            <slot :name="'panel-' + tab.id" :tab="tab"></slot>
          </div>
        </div>
      </div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "generic list with filtering and sorting",
    generic: "T extends { id: number | string }",
    code: `
const props = defineProps<{
  items: T[];
  filterFn?: (item: T, query: string) => boolean;
  sortFn?: (a: T, b: T) => number;
}>();
const searchQuery = ref('');
const filteredItems = computed(() => {
  let result = props.items;
  if (searchQuery.value && props.filterFn) {
    result = result.filter(item => props.filterFn!(item, searchQuery.value));
  }
  if (props.sortFn) {
    result = [...result].sort(props.sortFn);
  }
  return result;
});
    `,
    template: `
      <div class="list-container">
        <div class="list-header">
          <slot name="header">
            <input v-model="searchQuery" placeholder="Search..." />
          </slot>
        </div>
        <ul v-if="filteredItems.length" class="list-items">
          <li v-for="item in filteredItems" :key="item.id">
            <slot name="item" :item="item">{{ item }}</slot>
          </li>
        </ul>
        <div v-else class="empty-state">
          <slot name="empty">No items found</slot>
        </div>
        <div class="list-footer">
          <slot name="footer" :count="filteredItems.length">
            {{ filteredItems.length }} items
          </slot>
        </div>
      </div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "deeply nested conditionals and loops",
    code: `
interface Category {
  id: number;
  name: string;
  expanded: boolean;
  items: {
    id: number;
    name: string;
    tags: string[];
    status: 'active' | 'inactive' | 'pending';
  }[];
}
const categories = ref<Category[]>([]);
function toggleCategory(id: number) {}
    `,
    template: `
      <div class="category-list">
        <template v-for="category in categories" :key="category.id">
          <div class="category-header" @click="toggleCategory(category.id)">
            <span>{{ category.name }}</span>
            <span>{{ category.expanded ? '−' : '+' }}</span>
          </div>
          <Transition name="expand">
            <ul v-if="category.expanded" class="category-items">
              <template v-for="item in category.items" :key="item.id">
                <li v-if="item.status !== 'inactive'" :class="item.status">
                  <span class="item-name">{{ item.name }}</span>
                  <span class="item-status">
                    <template v-if="item.status === 'active'">✓</template>
                    <template v-else-if="item.status === 'pending'">⏳</template>
                  </span>
                  <div v-if="item.tags.length" class="item-tags">
                    <span v-for="tag in item.tags" :key="tag" class="tag">{{ tag }}</span>
                  </div>
                </li>
              </template>
              <li v-if="!category.items.some(i => i.status !== 'inactive')">
                No active items
              </li>
            </ul>
          </Transition>
        </template>
        <div v-if="!categories.length" class="empty">No categories</div>
      </div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "form with validation states",
    code: `
interface FormField {
  value: string;
  error: string | null;
  touched: boolean;
}
interface Form {
  name: FormField;
  email: FormField;
  password: FormField;
}
const form = ref<Form>({
  name: { value: '', error: null, touched: false },
  email: { value: '', error: null, touched: false },
  password: { value: '', error: null, touched: false },
});
const isSubmitting = ref(false);
function validateField(field: keyof Form) {}
function handleSubmit() {}
    `,
    template: `
      <form @submit.prevent="handleSubmit" :class="{ submitting: isSubmitting }">
        <div
          v-for="(field, name) in form"
          :key="name"
          class="form-group"
          :class="{
            'has-error': field.error && field.touched,
            'is-valid': !field.error && field.touched
          }"
        >
          <label :for="name">{{ name }}</label>
          <input
            :id="name"
            v-model="field.value"
            :type="name === 'password' ? 'password' : name === 'email' ? 'email' : 'text'"
            @blur="field.touched = true; validateField(name as keyof Form)"
            :disabled="isSubmitting"
          />
          <Transition name="fade">
            <span v-if="field.error && field.touched" class="error">
              {{ field.error }}
            </span>
          </Transition>
        </div>
        <button type="submit" :disabled="isSubmitting || Object.values(form).some(f => f.error)">
          <template v-if="isSubmitting">Submitting...</template>
          <template v-else>Submit</template>
        </button>
      </form>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Edge Cases Fixtures
// ============================================================================

const edgeCaseFixtures: Fixture[] = [
  {
    name: "empty template",
    code: "",
    template: "",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "template with only whitespace",
    code: "",
    template: "   \n\t  ",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "template with only text",
    code: "",
    template: "Just some text",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "template with only comment",
    code: "",
    template: "<!-- This is a comment -->",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "deeply nested elements",
    code: "",
    template: `
      <div>
        <div>
          <div>
            <div>
              <div>
                <span>Deep</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    `,
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "mixed content with interpolation",
    code: "const name = 'World';",
    template: "<div>Hello {{ name }}! How are you?</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-pre directive (skip compilation)",
    code: "",
    template: "<div v-pre>{{ this will not be compiled }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-once directive",
    code: "const msg = 'Hello';",
    template: "<div v-once>{{ msg }}</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-memo directive",
    code: "const item = ref({ id: 1 });",
    template: '<div v-memo="[item.id]">{{ item }}</div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-cloak directive",
    code: "",
    template: "<div v-cloak>Loading...</div>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-html directive",
    code: "const html = '<strong>Bold</strong>';",
    template: '<div v-html="html"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "v-text directive",
    code: "const text = 'Plain text';",
    template: '<div v-text="text"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "custom directive",
    code: "const vFocus = { mounted: (el: HTMLElement) => el.focus() };",
    template: "<input v-focus />",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "custom directive with argument",
    code: "",
    template: '<div v-my-directive:arg="value"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "custom directive with modifiers",
    code: "",
    template: '<div v-my-directive.mod1.mod2="value"></div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "special characters in template",
    code: "",
    template: '<div data-test="a < b && c > d">&lt;escaped&gt;</div>',
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
  {
    name: "template with CDATA-like content",
    code: "",
    template: "<pre><code>const x = 1 &amp;&amp; y &lt; 2;</code></pre>",
    expectations: {
      patterns: ["enhanceElementWithProps"],
    },
  },
];

// ============================================================================
// Combine all fixtures
// ============================================================================

/**
 * All fixtures for component type testing
 */
export const fixtures: Fixture[] = [
  ...basicElementFixtures,
  ...attributeFixtures,
  ...expressionFixtures,
  ...conditionalFixtures,
  ...loopFixtures,
  ...slotFixtures,
  ...eventFixtures,
  ...vModelFixtures,
  ...componentFixtures,
  ...builtinFixtures,
  ...refFixtures,
  ...complexFixtures,
  ...edgeCaseFixtures,
];

/**
 * Create the fixture configuration for the generator
 */
export function createFixtures(prefix: string = ""): FixtureConfig {
  return {
    fixtures,
    process: (fixture) => {
      return processComponentType(
        fixture.code,
        prefix,
        fixture.lang,
        fixture.generic,
        fixture.template
      );
    },
    prefix,
  };
}
