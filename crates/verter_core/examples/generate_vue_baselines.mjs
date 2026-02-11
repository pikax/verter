/**
 * Vue Baseline Generator for Template Codegen Tests
 *
 * Compiles template strings using Vue's official compiler (@vue/compiler-sfc)
 * and outputs the render function code as JSON baselines.
 *
 * Usage: node crates/verter_core/examples/generate_vue_baselines.mjs
 *
 * Output: crates/verter_core/examples/codegen/generated/vue_baselines.json
 *
 * Requires: @vue/compiler-sfc (already in root package.json)
 */

import { createRequire } from "module";
const require = createRequire(import.meta.url);

const { compileTemplate } = require("@vue/compiler-sfc");
const fs = require("fs");
const path = require("path");
const { fileURLToPath } = require("url");

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// =============================================================================
// Test Cases
// =============================================================================

const cases = [
  // === Elements ===
  { name: "simple_div", template: `<div>hello</div>` },
  { name: "self_closing_br", template: `<br/>` },
  { name: "void_input", template: `<input>` },
  {
    name: "void_img_attrs",
    template: `<img src="a.png" alt="pic">`,
  },
  { name: "nested_elements", template: `<div><span>inner</span></div>` },
  {
    name: "deeply_nested",
    template: `<div><span><em>deep</em></span></div>`,
  },
  {
    name: "multiple_children_elements",
    template: `<div><span>a</span><span>b</span></div>`,
  },
  { name: "empty_div", template: `<div></div>` },
  {
    name: "sibling_void_elements",
    template: `<div><input><hr><br></div>`,
  },

  // === Props ===
  { name: "static_id", template: `<div id="app">hi</div>` },
  { name: "static_class", template: `<div class="foo bar">hi</div>` },
  { name: "static_style", template: `<div style="color: red">hi</div>` },
  { name: "bound_id", template: `<div :id="myId">hi</div>` },
  { name: "bound_class", template: `<div :class="cls">hi</div>` },
  { name: "bound_style", template: `<div :style="sty">hi</div>` },
  {
    name: "multiple_bound",
    template: `<div :id="a" :title="b">hi</div>`,
  },
  {
    name: "mixed_static_bound",
    template: `<div id="s" :title="d">hi</div>`,
  },
  {
    name: "class_style_combined",
    template: `<div :class="c" :style="s">hi</div>`,
  },
  {
    name: "event_click",
    template: `<button @click="handler">click</button>`,
  },
  {
    name: "multiple_events",
    template: `<button @click="a" @mouseover="b">hi</button>`,
  },
  { name: "props_null_when_empty", template: `<div>hello</div>` },

  // === Text ===
  { name: "text_only", template: `<div>hello</div>` },
  { name: "text_with_quotes", template: `<div>say "hello"</div>` },
  { name: "text_whitespace", template: `<div>   </div>` },

  // === Interpolation ===
  { name: "interpolation_simple", template: `<div>{{ msg }}</div>` },
  { name: "interpolation_expr", template: `<div>{{ a + b }}</div>` },
  {
    name: "interpolation_ternary",
    template: `<div>{{ a ? b : c }}</div>`,
  },
  { name: "interpolation_method", template: `<div>{{ foo() }}</div>` },
  {
    name: "interpolation_member",
    template: `<div>{{ obj.prop }}</div>`,
  },

  // === Text + Interpolation Mix (concatenation) ===
  {
    name: "text_and_interpolation",
    template: `<div>hello {{ msg }}</div>`,
  },
  {
    name: "text_interpolation_text",
    template: `<div>hello {{ msg }} world</div>`,
  },
  {
    name: "multiple_interpolations",
    template: `<div>{{ a }}{{ b }}</div>`,
  },

  // === Comments ===
  {
    name: "comment_basic",
    template: `<div><!-- my comment --></div>`,
  },
  { name: "comment_only_child", template: `<div><!-- only --></div>` },
  { name: "comment_empty", template: `<div><!----></div>` },
  {
    name: "comment_with_elements",
    template: `<div><span>a</span><!-- mid --><span>b</span></div>`,
  },

  // === v-if ===
  { name: "v_if_simple", template: `<div v-if="show">yes</div>` },
  {
    name: "v_if_else",
    template: `<div v-if="show">yes</div><div v-else>no</div>`,
  },
  {
    name: "v_if_else_if_else",
    template: `<div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div>`,
  },
  {
    name: "v_if_with_class",
    template: `<div v-if="show" class="foo">hi</div>`,
  },

  // === v-for ===
  {
    name: "v_for_simple",
    template: `<div v-for="item in items">{{ item }}</div>`,
  },
  {
    name: "v_for_keyed",
    template: `<div v-for="item in items" :key="item">{{ item }}</div>`,
  },
  {
    name: "v_for_index",
    template: `<div v-for="(item, index) in items" :key="index">{{ item }}</div>`,
  },
  {
    name: "v_for_nested",
    template: `<div v-for="g in groups"><span v-for="i in g">{{ i }}</span></div>`,
  },

  // === v-once ===
  { name: "v_once_simple", template: `<div v-once>static</div>` },
  {
    name: "v_once_with_dynamic",
    template: `<div v-once :id="foo">content</div>`,
  },

  // === Mixed children ===
  {
    name: "text_and_element",
    template: `<div>text<span>child</span></div>`,
  },
  {
    name: "element_and_text",
    template: `<div><span>child</span>text</div>`,
  },
  {
    name: "interpolation_and_element",
    template: `<div>{{ msg }}<span>fixed</span></div>`,
  },
  {
    name: "v_if_inside_v_for",
    template: `<div v-for="item in items"><span v-if="item.show">{{ item.name }}</span></div>`,
  },

  // === Components ===
  { name: "component_simple", template: `<MyComponent/>` },
  {
    name: "component_with_props",
    template: `<MyComponent :msg="hello"/>`,
  },
  {
    name: "component_with_children",
    template: `<MyComponent>content</MyComponent>`,
  },

  // === Multiple roots ===
  {
    name: "multiple_roots",
    template: `<div>a</div><div>b</div>`,
  },
  { name: "root_text_only", template: `just text` },
  { name: "root_interpolation", template: `{{ msg }}` },
  {
    name: "root_comment",
    template: `<!-- comment -->`,
  },
];

// =============================================================================
// Compile
// =============================================================================

function compileTemplateCase(template, isProd) {
  try {
    const result = compileTemplate({
      source: template,
      filename: "test.vue",
      id: "test",
      isProd,
      ssr: false,
      compilerOptions: {
        mode: "module",
        hoistStatic: false,
        prefixIdentifiers: true,
        cacheHandlers: false,
      },
    });
    return {
      code: result.code,
      errors: (result.errors || []).map((e) => e.message || String(e)),
    };
  } catch (e) {
    return { code: "", errors: [e.message] };
  }
}

const output = {};

for (const c of cases) {
  const dev = compileTemplateCase(c.template, false);
  const prod = compileTemplateCase(c.template, true);

  output[c.name] = {
    template: c.template,
    dev: dev.code,
    dev_errors: dev.errors.length > 0 ? dev.errors : undefined,
    prod: prod.code,
    prod_errors: prod.errors.length > 0 ? prod.errors : undefined,
  };

  if (dev.errors.length > 0) {
    console.error(`  WARN [${c.name}] dev errors:`, dev.errors);
  }
}

// =============================================================================
// Output
// =============================================================================

const GENERATED_DIR = path.join(__dirname, "codegen", "generated");
if (!fs.existsSync(GENERATED_DIR)) {
  fs.mkdirSync(GENERATED_DIR, { recursive: true });
}

const outputPath = path.join(GENERATED_DIR, "vue_baselines.json");
fs.writeFileSync(outputPath, JSON.stringify(output, null, 2) + "\n");
console.log(`Written ${Object.keys(output).length} baselines to ${outputPath}`);

// Also print a human-readable summary
console.log("\n=== Summary ===\n");
for (const [name, data] of Object.entries(output)) {
  console.log(`--- ${name} ---`);
  console.log(`Template: ${data.template}`);
  if (data.dev_errors) {
    console.log(`Errors: ${data.dev_errors.join(", ")}`);
  } else {
    // Print just the render function body (skip imports)
    const lines = data.dev.split("\n");
    const renderStart = lines.findIndex((l) =>
      l.includes("export function render")
    );
    if (renderStart >= 0) {
      console.log("Dev render:");
      for (let i = renderStart; i < lines.length; i++) {
        console.log(`  ${lines[i]}`);
      }
    } else {
      console.log(`Dev: ${data.dev.substring(0, 200)}...`);
    }
  }
  console.log();
}
