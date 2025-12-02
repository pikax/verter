<script setup lang="ts">
import { ref, useTemplateRef, onMounted, type ComponentPublicInstance } from "vue";

// Modern useTemplateRef API (Vue 3.5+)
const inputRef = useTemplateRef<HTMLInputElement>("inputEl");
const divRef = useTemplateRef<HTMLDivElement>("divEl");
const canvasRef = useTemplateRef<HTMLCanvasElement>("canvasEl");

// Component ref with typed instance
interface ChildComponentInstance {
  focus: () => void;
  getValue: () => string;
  reset: () => void;
}
const childRef = useTemplateRef<ChildComponentInstance>("childComp");

// Multiple refs via v-for - requires ref array
const listItemRefs = ref<HTMLLIElement[]>([]);
function setListItemRef(el: HTMLLIElement | null, index: number) {
  if (el) {
    listItemRefs.value[index] = el;
  }
}

// Dynamic ref name - using value for useTemplateRef
const dynamicRefName = ref("dynamic");
const dynamicRef = useTemplateRef<HTMLElement>("dynamic");

// Function ref pattern - must accept Element or ComponentPublicInstance
const functionRefElement = ref<HTMLButtonElement | null>(null);
function setFunctionRef(el: Element | ComponentPublicInstance | null) {
  functionRefElement.value = el as HTMLButtonElement | null;
}

// Typed form refs
const formRef = useTemplateRef<HTMLFormElement>("formEl");
const selectRef = useTemplateRef<HTMLSelectElement>("selectEl");
const textareaRef = useTemplateRef<HTMLTextAreaElement>("textareaEl");

// Multiple component refs
const componentRefs = ref<Map<string, ComponentPublicInstance>>(new Map());
function setComponentRef(name: string) {
  return (el: ComponentPublicInstance | null) => {
    if (el) {
      componentRefs.value.set(name, el);
    } else {
      componentRefs.value.delete(name);
    }
  };
}

// Methods using refs
function focusInput() {
  inputRef.value?.focus();
}

function getInputValue(): string {
  return inputRef.value?.value ?? "";
}

function scrollDivToTop() {
  if (divRef.value) {
    divRef.value.scrollTop = 0;
  }
}

function drawOnCanvas() {
  const ctx = canvasRef.value?.getContext("2d");
  if (ctx) {
    ctx.fillStyle = "blue";
    ctx.fillRect(10, 10, 100, 100);
  }
}

function callChildMethod() {
  childRef.value?.focus();
}

function getChildValue(): string {
  return childRef.value?.getValue() ?? "";
}

function resetChild() {
  childRef.value?.reset();
}

function focusListItem(index: number) {
  listItemRefs.value[index]?.focus();
}

function submitForm() {
  formRef.value?.submit();
}

function getSelectedValue(): string {
  return selectRef.value?.value ?? "";
}

function getTextareaValue(): string {
  return textareaRef.value?.value ?? "";
}

// Lifecycle usage
onMounted(() => {
  // Refs are available after mount
  console.log("Input ref:", inputRef.value);
  console.log("Div ref:", divRef.value);
  
  // Focus input on mount
  inputRef.value?.focus();
});

// Expose for parent
defineExpose({
  focusInput,
  getInputValue,
});

const items = ref(["Item 1", "Item 2", "Item 3"]);
const options = ref(["Option A", "Option B", "Option C"]);
</script>

<template>
  <div class="template-refs-demo">
    <section>
      <h3>Basic Element Refs</h3>
      <input ref="inputEl" type="text" placeholder="Input with ref" />
      <button @click="focusInput">Focus Input</button>
      <div ref="divEl" class="scrollable">Scrollable div content</div>
      <button @click="scrollDivToTop">Scroll to Top</button>
    </section>

    <section>
      <h3>Canvas Ref</h3>
      <canvas ref="canvasEl" width="200" height="150"></canvas>
      <button @click="drawOnCanvas">Draw</button>
    </section>

    <section>
      <h3>Component Ref</h3>
      <!-- Assuming ChildComponent is imported -->
      <div ref="childComp">Child component placeholder</div>
      <button @click="callChildMethod">Call Child Focus</button>
      <button @click="resetChild">Reset Child</button>
    </section>

    <section>
      <h3>v-for Refs (Function Pattern)</h3>
      <ul>
        <li
          v-for="(item, index) in items"
          :key="item"
          :ref="(el) => setListItemRef(el as HTMLLIElement, index)"
          tabindex="0"
        >
          {{ item }}
        </li>
      </ul>
      <button @click="focusListItem(0)">Focus First</button>
      <button @click="focusListItem(1)">Focus Second</button>
    </section>

    <section>
      <h3>Function Ref</h3>
      <button :ref="setFunctionRef">Function Ref Button</button>
    </section>

    <section>
      <h3>Form Refs</h3>
      <form ref="formEl" @submit.prevent="submitForm">
        <select ref="selectEl">
          <option v-for="opt in options" :key="opt" :value="opt">{{ opt }}</option>
        </select>
        <textarea ref="textareaEl" placeholder="Textarea"></textarea>
        <button type="submit">Submit</button>
      </form>
    </section>

    <section>
      <h3>Dynamic Ref Name</h3>
      <div :ref="dynamicRefName">Dynamic ref element</div>
    </section>
  </div>
</template>
