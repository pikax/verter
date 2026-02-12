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
  { name: "svg_element", template: `<svg><circle cx="50" cy="50" r="40"/></svg>` },
  { name: "math_element", template: `<math><mrow><mi>x</mi></mrow></math>` },
  { name: "custom_element", template: `<my-element></my-element>` },
  { name: "namespace_element", template: `<svg:g><svg:circle/></svg:g>` },

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
  { name: "v_bind_object", template: `<div v-bind="attrs">hi</div>` },
  { name: "v_bind_with_colon", template: `<div v-bind:id="myId">hi</div>` },
  { name: "v_on_with_at", template: `<button v-on:click="handler">click</button>` },
  { name: "v_on_object", template: `<div v-on="listeners">hi</div>` },
  { name: "class_array", template: `<div :class="[a, b]">hi</div>` },
  { name: "class_object", template: `<div :class="{ active: isActive }">hi</div>` },
  { name: "class_mixed", template: `<div :class="[a, { active: isActive }]">hi</div>` },
  { name: "style_object", template: `<div :style="{ color: red }">hi</div>` },
  { name: "style_array", template: `<div :style="[baseStyles, overrideStyles]">hi</div>` },
  { name: "style_camelcase", template: `<div :style="{ fontSize: size }">hi</div>` },
  { name: "attr_boolean", template: `<input :disabled="isDisabled">` },
  { name: "attr_data", template: `<div :data-id="id">hi</div>` },
  { name: "attr_aria", template: `<button :aria-label="label">click</button>` },
  { name: "attr_hyphenated", template: `<div :my-custom-attr="value">hi</div>` },
  { name: "prop_camelcase", template: `<MyComp :myProp="value"/>` },

  // === Events ===
  { name: "event_inline", template: `<button @click="count++">click</button>` },
  { name: "event_with_args", template: `<button @click="handler($event, 'arg')">click</button>` },
  { name: "event_modifier_stop", template: `<button @click.stop="handler">click</button>` },
  { name: "event_modifier_prevent", template: `<form @submit.prevent="onSubmit">submit</form>` },
  { name: "event_modifier_capture", template: `<div @click.capture="handler">click</div>` },
  { name: "event_modifier_self", template: `<div @click.self="handler">click</div>` },
  { name: "event_modifier_once", template: `<button @click.once="handler">click</button>` },
  { name: "event_modifier_passive", template: `<div @scroll.passive="onScroll">scroll</div>` },
  { name: "event_modifier_chain", template: `<button @click.stop.prevent="handler">click</button>` },
  { name: "event_key_modifier", template: `<input @keyup.enter="submit">` },
  { name: "event_key_chain", template: `<input @keydown.ctrl.enter="submit">` },
  { name: "event_mouse_modifier", template: `<button @click.right="handler">click</button>` },
  { name: "event_exact_modifier", template: `<button @click.ctrl.exact="handler">click</button>` },
  { name: "event_custom", template: `<MyComp @custom-event="handler"/>` },
  { name: "event_update", template: `<MyComp @update:modelValue="handler"/>` },

  // === Text ===
  { name: "text_only", template: `<div>hello</div>` },
  { name: "text_with_quotes", template: `<div>say "hello"</div>` },
  { name: "text_whitespace", template: `<div>   </div>` },
  { name: "text_newlines", template: `<div>line1\nline2</div>` },
  { name: "text_entities", template: `<div>&lt;&gt;&amp;</div>` },
  { name: "text_unicode", template: `<div>Hello 世界 🌍</div>` },

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
  { name: "interpolation_array", template: `<div>{{ arr[0] }}</div>` },
  { name: "interpolation_chained", template: `<div>{{ obj.nested.deep }}</div>` },
  { name: "interpolation_optional", template: `<div>{{ obj?.prop }}</div>` },
  { name: "interpolation_nullish", template: `<div>{{ val ?? 'default' }}</div>` },
  { name: "interpolation_logical", template: `<div>{{ a && b }}</div>` },
  { name: "interpolation_comparison", template: `<div>{{ a > b }}</div>` },
  { name: "interpolation_string_template", template: `<div>{{ \`Hello \${name}\` }}</div>` },
  { name: "interpolation_filter_style", template: `<div>{{ msg | capitalize }}</div>` },

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
  {
    name: "complex_mixed_content",
    template: `<div>Start {{ a }} middle {{ b }} end</div>`,
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
  { name: "comment_multiline", template: `<div><!-- line1\nline2 --></div>` },
  { name: "comment_adjacent", template: `<div><!-- a --><!-- b --></div>` },

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
  {
    name: "v_if_multiple_else_if",
    template: `<div v-if="a">A</div><div v-else-if="b">B</div><div v-else-if="c">C</div><div v-else>D</div>`,
  },
  {
    name: "v_if_complex_expression",
    template: `<div v-if="a && b || c">yes</div>`,
  },
  {
    name: "v_if_with_template",
    template: `<template v-if="show"><div>a</div><div>b</div></template>`,
  },
  {
    name: "v_if_nested",
    template: `<div v-if="outer"><div v-if="inner">nested</div></div>`,
  },

  // === v-else-if edge cases ===
  {
    name: "v_else_if_with_comment",
    template: `<div v-if="a">A</div><!-- comment --><div v-else-if="b">B</div>`,
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
  {
    name: "v_for_of",
    template: `<div v-for="item of items">{{ item }}</div>`,
  },
  {
    name: "v_for_range",
    template: `<div v-for="n in 10">{{ n }}</div>`,
  },
  {
    name: "v_for_object",
    template: `<div v-for="(value, key) in obj">{{ key }}: {{ value }}</div>`,
  },
  {
    name: "v_for_object_full",
    template: `<div v-for="(value, key, index) in obj">{{ index }}. {{ key }}: {{ value }}</div>`,
  },
  {
    name: "v_for_template",
    template: `<template v-for="item in items"><div>{{ item }}</div></template>`,
  },
  {
    name: "v_for_component",
    template: `<MyComp v-for="item in items" :key="item.id" :data="item"/>`,
  },
  {
    name: "v_for_destructure",
    template: `<div v-for="{ id, name } in items" :key="id">{{ name }}</div>`,
  },
  {
    name: "v_for_destructure_nested",
    template: `<div v-for="{ user: { name } } in items">{{ name }}</div>`,
  },

  // === v-show ===
  { name: "v_show_simple", template: `<div v-show="visible">content</div>` },
  { name: "v_show_with_class", template: `<div v-show="visible" class="box">content</div>` },

  // === v-model ===
  { name: "v_model_input", template: `<input v-model="text">` },
  { name: "v_model_textarea", template: `<textarea v-model="text"></textarea>` },
  { name: "v_model_checkbox", template: `<input type="checkbox" v-model="checked">` },
  { name: "v_model_radio", template: `<input type="radio" v-model="picked" value="a">` },
  { name: "v_model_select", template: `<select v-model="selected"><option>A</option></select>` },
  { name: "v_model_lazy", template: `<input v-model.lazy="text">` },
  { name: "v_model_number", template: `<input v-model.number="age">` },
  { name: "v_model_trim", template: `<input v-model.trim="text">` },
  { name: "v_model_modifiers_chain", template: `<input v-model.lazy.trim="text">` },
  { name: "v_model_component", template: `<MyComp v-model="value"/>` },
  { name: "v_model_named", template: `<MyComp v-model:title="title"/>` },
  { name: "v_model_multiple", template: `<MyComp v-model:foo="foo" v-model:bar="bar"/>` },
  { name: "v_model_custom_modifiers", template: `<MyComp v-model.capitalize="text"/>` },

  // === v-once ===
  { name: "v_once_simple", template: `<div v-once>static</div>` },
  {
    name: "v_once_with_dynamic",
    template: `<div v-once :id="foo">content</div>`,
  },
  {
    name: "v_once_with_interpolation",
    template: `<div v-once>{{ msg }}</div>`,
  },

  // === v-memo ===
  { name: "v_memo_simple", template: `<div v-memo="[dep]">content</div>` },
  { name: "v_memo_multiple", template: `<div v-memo="[a, b, c]">content</div>` },

  // === v-pre ===
  { name: "v_pre_simple", template: `<div v-pre>{{ not interpolated }}</div>` },
  { name: "v_pre_with_directive", template: `<div v-pre v-if="show">{{ msg }}</div>` },

  // === v-cloak ===
  { name: "v_cloak", template: `<div v-cloak>{{ msg }}</div>` },

  // === v-text ===
  { name: "v_text", template: `<div v-text="msg"></div>` },

  // === v-html ===
  { name: "v_html", template: `<div v-html="rawHtml"></div>` },

  // === Slots ===
  { name: "slot_default", template: `<slot></slot>` },
  { name: "slot_named", template: `<slot name="header"></slot>` },
  { name: "slot_fallback", template: `<slot>Default content</slot>` },
  { name: "slot_usage", template: `<MyComp><span>content</span></MyComp>` },
  { name: "slot_named_usage", template: `<MyComp><template #header>Header</template></MyComp>` },
  { name: "slot_v_slot", template: `<MyComp><template v-slot:header>Header</template></MyComp>` },
  { name: "slot_scoped", template: `<MyComp v-slot="{ item }">{{ item }}</MyComp>` },
  { name: "slot_scoped_named", template: `<MyComp><template #default="{ item }">{{ item }}</template></MyComp>` },
  { name: "slot_destructure", template: `<MyComp v-slot="{ user: { name } }">{{ name }}</MyComp>` },
  { name: "slot_shorthand", template: `<MyComp #header>Header</MyComp>` },
  { name: "slot_dynamic", template: `<MyComp><template #[dynamicSlot]>Content</template></MyComp>` },

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
  { name: "component_is", template: `<component :is="comp"/>` },
  { name: "component_is_string", template: `<component is="div"/>` },
  { name: "component_kebab", template: `<my-component/>` },
  { name: "component_pascal", template: `<MyComponent/>` },
  { name: "component_dynamic_props", template: `<MyComp v-bind="props"/>` },
  { name: "component_with_ref", template: `<MyComp ref="comp"/>` },

  // === Transition ===
  { name: "transition", template: `<transition><div v-if="show">content</div></transition>` },
  { name: "transition_name", template: `<transition name="fade"><div v-if="show">content</div></transition>` },
  { name: "transition_mode", template: `<transition mode="out-in"><div v-if="show">content</div></transition>` },
  { name: "transition_group", template: `<transition-group><div v-for="i in items" :key="i">{{ i }}</div></transition-group>` },
  { name: "transition_events", template: `<transition @enter="onEnter"><div v-if="show">content</div></transition>` },

  // === KeepAlive ===
  { name: "keep_alive", template: `<keep-alive><component :is="current"/></keep-alive>` },
  { name: "keep_alive_include", template: `<keep-alive :include="['a', 'b']"><component :is="current"/></keep-alive>` },
  { name: "keep_alive_max", template: `<keep-alive :max="10"><component :is="current"/></keep-alive>` },

  // === Teleport ===
  { name: "teleport", template: `<teleport to="body"><div>modal</div></teleport>` },
  { name: "teleport_disabled", template: `<teleport to="body" :disabled="disabled"><div>modal</div></teleport>` },

  // === Suspense ===
  { name: "suspense", template: `<suspense><AsyncComp/></suspense>` },
  { name: "suspense_fallback", template: `<suspense><AsyncComp/><template #fallback>Loading...</template></suspense>` },

  // === Template ===
  { name: "template_wrapper", template: `<template><div>a</div><div>b</div></template>` },
  { name: "template_v_if", template: `<template v-if="show"><div>content</div></template>` },
  { name: "template_v_for", template: `<template v-for="i in items"><div>{{ i }}</div></template>` },

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
  { name: "three_roots", template: `<div>a</div><div>b</div><div>c</div>` },
  { name: "mixed_root_types", template: `<div>a</div>text{{ msg }}<span>b</span>` },

  // === Whitespace handling ===
  { name: "whitespace_preserve", template: `<pre>  spaces  </pre>` },
  { name: "whitespace_collapse", template: `<div>  multiple   spaces  </div>` },
  { name: "whitespace_newline", template: `<div>\n  content\n</div>` },
  { name: "whitespace_between_tags", template: `<span>a</span>   <span>b</span>` },

  // === Special attributes ===
  { name: "attr_key", template: `<div :key="id">content</div>` },
  { name: "attr_ref", template: `<div ref="myDiv">content</div>` },
  { name: "attr_ref_for", template: `<div v-for="i in items" :ref="el => refs[i] = el">{{ i }}</div>` },
  { name: "attr_is", template: `<div :is="component">content</div>` },

  // === Dynamic arguments ===
  { name: "dynamic_attr", template: `<div :[attrName]="value">content</div>` },
  { name: "dynamic_event", template: `<button @[eventName]="handler">click</button>` },
  { name: "dynamic_slot", template: `<MyComp><template #[slotName]>content</template></MyComp>` },
  { name: "dynamic_directive", template: `<div v-[directive]="value">content</div>` },

  // === Edge cases ===
  { name: "empty_template", template: `` },
  { name: "only_whitespace", template: `   \n  \t  ` },
  { name: "only_comment", template: `<!-- only comment -->` },
  { name: "multiple_comments", template: `<!-- a --><!-- b --><!-- c -->` },
  { name: "nested_empty_elements", template: `<div><div><div></div></div></div>` },
  { name: "self_closing_component", template: `<MyComp/>` },
  { name: "self_closing_with_props", template: `<MyComp :prop="val"/>` },
  { name: "adjacent_text_nodes", template: `<div>a{{ b }}c{{ d }}e</div>` },
  { name: "deeply_nested_interpolation", template: `<div><span><em>{{ msg }}</em></span></div>` },
  { name: "complex_expression", template: `<div>{{ (a || b) && (c ? d : e) }}</div>` },
  { name: "array_destructure_rest", template: `<div v-for="[first, ...rest] in items">{{ first }}</div>` },
  { name: "object_destructure_rest", template: `<div v-for="{ id, ...rest } in items">{{ id }}</div>` },
  { name: "v_for_v_if_warning", template: `<div v-if="show" v-for="i in items">{{ i }}</div>` },
  { name: "special_chars_in_text", template: `<div>&lt;script&gt;alert('xss')&lt;/script&gt;</div>` },
  { name: "quote_escaping", template: `<div title='it\\'s'>content</div>` },
  { name: "attribute_no_value", template: `<input disabled>` },
  { name: "attribute_empty_value", template: `<input value="">` },
  { name: "multiple_v_model", template: `<input v-model="a" v-model="b">` },
  { name: "v_bind_after_v_for", template: `<div v-for="i in items" v-bind="i">{{ i }}</div>` },
  
  // === Real-world patterns ===
  {
    name: "form_complex",
    template: `<form @submit.prevent="onSubmit">
  <input v-model="form.name" required>
  <input type="email" v-model.trim="form.email">
  <select v-model="form.country">
    <option v-for="c in countries" :key="c.code" :value="c.code">{{ c.name }}</option>
  </select>
  <button type="submit" :disabled="!isValid">Submit</button>
</form>`,
  },
  {
    name: "list_with_actions",
    template: `<ul>
  <li v-for="(item, idx) in items" :key="item.id" :class="{ active: idx === activeIndex }">
    <span>{{ item.name }}</span>
    <button @click="edit(item)">Edit</button>
    <button @click.stop="remove(item.id)">Delete</button>
  </li>
</ul>`,
  },
  {
    name: "conditional_rendering_complex",
    template: `<div>
  <template v-if="loading">
    <div class="spinner"></div>
  </template>
  <template v-else-if="error">
    <div class="error">{{ error.message }}</div>
  </template>
  <template v-else-if="data">
    <div v-for="item in data" :key="item.id">{{ item.name }}</div>
  </template>
  <template v-else>
    <div class="empty">No data</div>
  </template>
</div>`,
  },
  {
    name: "modal_pattern",
    template: `<teleport to="body">
  <transition name="modal">
    <div v-if="show" class="modal-mask" @click.self="close">
      <div class="modal-wrapper">
        <div class="modal-container">
          <slot name="header"></slot>
          <slot></slot>
          <slot name="footer">
            <button @click="close">Close</button>
          </slot>
        </div>
      </div>
    </div>
  </transition>
</teleport>`,
  },
  {
    name: "recursive_component",
    template: `<div>
  <span>{{ node.name }}</span>
  <TreeNode v-for="child in node.children" :key="child.id" :node="child"/>
</div>`,
  },
  {
    name: "table_with_sorting",
    template: `<table>
  <thead>
    <tr>
      <th v-for="col in columns" :key="col.key" @click="sort(col.key)" :class="{ sorted: sortBy === col.key }">
        {{ col.label }}
        <span v-if="sortBy === col.key">{{ sortDir === 'asc' ? '↑' : '↓' }}</span>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr v-for="row in sortedData" :key="row.id">
      <td v-for="col in columns" :key="col.key">{{ row[col.key] }}</td>
    </tr>
  </tbody>
</table>`,
  },
];

// =============================================================================
// Compile
// =============================================================================

function compileTemplateCase(template, isProd, vapor = false) {
  try {
    const result = compileTemplate({
      source: template,
      filename: "test.vue",
      id: "test",
      isProd,
      ssr: false,
      vapor,
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
  const vaporDev = compileTemplateCase(c.template, false, true);
  const vaporProd = compileTemplateCase(c.template, true, true);

  output[c.name] = {
    template: c.template,
    dev: dev.code,
    dev_errors: dev.errors.length > 0 ? dev.errors : undefined,
    prod: prod.code,
    prod_errors: prod.errors.length > 0 ? prod.errors : undefined,
    vapor_dev: vaporDev.code,
    vapor_dev_errors: vaporDev.errors.length > 0 ? vaporDev.errors : undefined,
    vapor_prod: vaporProd.code,
    vapor_prod_errors: vaporProd.errors.length > 0 ? vaporProd.errors : undefined,
  };

  if (dev.errors.length > 0) {
    console.error(`  WARN [${c.name}] dev errors:`, dev.errors);
  }
  if (vaporDev.errors.length > 0) {
    console.error(`  WARN [${c.name}] vapor dev errors:`, vaporDev.errors);
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
