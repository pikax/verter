<script setup lang="ts">
import { ref, type Directive, type DirectiveBinding } from "vue";
import type { VNode } from "vue";

const lifecycleLog = ref<string[]>([]);
const inspectArg = ref<"color" | "padding">("color");
const pinnedSide = ref<"top" | "right" | "bottom" | "left">("top");
const pinDistance = ref(24);
const inspectedValue = ref("Hello directives");

const vFocusShorthand: Directive<HTMLElement> = (el) => {
  el.focus();
};

const vLifecycle: Directive<HTMLElement, string> = {
  created(_, binding) {
    pushLifecycle("created", binding);
  },
  beforeMount(_, binding) {
    pushLifecycle("beforeMount", binding);
  },
  mounted(_, binding) {
    pushLifecycle("mounted", binding);
  },
  beforeUpdate(_, binding) {
    pushLifecycle("beforeUpdate", binding);
  },
  updated(_, binding) {
    pushLifecycle("updated", binding);
  },
  beforeUnmount(_, binding) {
    pushLifecycle("beforeUnmount", binding);
  },
  unmounted(_, binding) {
    pushLifecycle("unmounted", binding);
  },
};

function pushLifecycle(hook: string, binding: DirectiveBinding<string>) {
  const old = binding.oldValue === undefined ? "" : ` (old: ${binding.oldValue})`;
  lifecycleLog.value.push(`${hook}: ${binding.value}${old}`);
}

const vInspector: Directive<HTMLElement, string> = {
  beforeMount(el, binding, vnode) {
    renderBinding("beforeMount", el, binding, vnode);
  },
  updated(el, binding, vnode, prevVnode) {
    renderBinding("updated", el, binding, vnode, prevVnode);
  },
};

function renderBinding(
  hook: string,
  el: HTMLElement,
  binding: DirectiveBinding<string>,
  vnode?: VNode,
  prevVnode?: VNode,
) {
  const { value, oldValue, arg, modifiers, instance, dir } = binding;
  const vnodeType =
    typeof vnode?.type === "string"
      ? vnode.type
      : ((vnode?.type as any)?.name ?? vnode?.type ?? "-");
  const prevType =
    typeof prevVnode?.type === "string"
      ? prevVnode.type
      : ((prevVnode?.type as any)?.name ?? prevVnode?.type ?? "-");
  const modifierList = Object.keys(modifiers);

  el.textContent = [
    `hook: ${hook}`,
    `arg: ${arg ?? "-"}`,
    `value: ${value}`,
    `old: ${oldValue ?? "-"}`,
    `modifiers: ${modifierList.length ? modifierList.join(",") : "-"}`,
    `instance: ${instance ? "available" : "none"}`,
    `dir: ${dir ? "object available" : "-"}`,
    `vnode: ${vnodeType}`,
    `prev vnode: ${prevType}`,
  ].join(" | ");
}

const vPin: Directive<HTMLElement, number> = {
  mounted(el, binding) {
    applyPin(el, binding);
  },
  updated(el, binding) {
    applyPin(el, binding);
  },
};

function applyPin(el: HTMLElement, binding: DirectiveBinding<number>) {
  el.style.position = "relative";
  const side = binding.arg ?? "top";
  const distance = binding.modifiers.round ? Math.round(binding.value) : binding.value;
  (el.style as any)[side] = `${distance}px`;
  el.dataset.pinSide = side;
  el.dataset.pinValue = `${distance}`;
}

function bumpValue() {
  inspectedValue.value = `${inspectedValue.value}!`;
}

function resetLifecycleLog() {
  lifecycleLog.value = [];
}
</script>

<template>
  <section class="directive-usage">
    <h2>Directive usages from the guide</h2>

    <div class="example">
      <h3>Function shorthand (mounted + updated)</h3>
      <input v-focus-shorthand placeholder="Auto focus on mount/update" />
    </div>

    <div class="example">
      <h3>Lifecycle hooks with value changes</h3>
      <p v-lifecycle="inspectedValue">Watch lifecycle log below</p>
      <div class="controls">
        <button @click="bumpValue">Change value</button>
        <button @click="resetLifecycleLog">Clear log</button>
      </div>
      <ul>
        <li v-for="(entry, idx) in lifecycleLog" :key="idx">{{ entry }}</li>
      </ul>
    </div>

    <div class="example">
      <h3>Binding arguments, modifiers, and hook args</h3>
      <label>
        Dynamic argument:
        <select v-model="inspectArg">
          <option value="color">color</option>
          <option value="padding">padding</option>
        </select>
      </label>
      <input v-model="inspectedValue" />
      <p v-inspector:[inspectArg].bold.once="inspectedValue"></p>
    </div>

    <div class="example">
      <h3>Dynamic argument + modifiers (pin example)</h3>
      <div class="controls">
        <label>
          Side:
          <select v-model="pinnedSide">
            <option value="top">top</option>
            <option value="right">right</option>
            <option value="bottom">bottom</option>
            <option value="left">left</option>
          </select>
        </label>
        <label>
          Distance (px):
          <input v-model.number="pinDistance" type="number" min="0" />
        </label>
      </div>
      <div class="pin-box" v-pin:[pinnedSide].round="pinDistance">Pinned box</div>
    </div>
  </section>
</template>

<style scoped>
.directive-usage {
  display: grid;
  gap: 1rem;
}

.example {
  padding: 1rem;
  border: 1px solid #ddd;
  border-radius: 8px;
}

.controls {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  flex-wrap: wrap;
}

.pin-box {
  position: relative;
  padding: 1rem;
  background: #f5f5f5;
  border: 1px dashed #999;
  margin-top: 0.5rem;
}
</style>
