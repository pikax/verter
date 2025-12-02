<script setup>
import { ref, useTemplateRef, onMounted } from "vue";

// Modern useTemplateRef API (Vue 3.5+)
const inputRef = useTemplateRef("inputEl");
const divRef = useTemplateRef("divEl");
const canvasRef = useTemplateRef("canvasEl");

// Component ref
const childRef = useTemplateRef("childComp");

// Multiple refs via v-for
const listItemRefs = ref([]);
function setListItemRef(el, index) {
  if (el) {
    listItemRefs.value[index] = el;
  }
}

// Dynamic ref name
const dynamicRefName = ref("dynamic");
const dynamicRef = useTemplateRef(dynamicRefName);

// Function ref pattern
const functionRefElement = ref(null);
function setFunctionRef(el) {
  functionRefElement.value = el;
}

// Form refs
const formRef = useTemplateRef("formEl");
const selectRef = useTemplateRef("selectEl");
const textareaRef = useTemplateRef("textareaEl");

// Multiple component refs
const componentRefs = ref(new Map());
function setComponentRef(name) {
  return (el) => {
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

function getInputValue() {
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

function getChildValue() {
  return childRef.value?.getValue() ?? "";
}

function resetChild() {
  childRef.value?.reset();
}

function focusListItem(index) {
  listItemRefs.value[index]?.focus();
}

function submitForm() {
  formRef.value?.submit();
}

function getSelectedValue() {
  return selectRef.value?.value ?? "";
}

function getTextareaValue() {
  return textareaRef.value?.value ?? "";
}

onMounted(() => {
  console.log("Input ref:", inputRef.value);
  console.log("Div ref:", divRef.value);
  inputRef.value?.focus();
});

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
          :ref="(el) => setListItemRef(el, index)"
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
