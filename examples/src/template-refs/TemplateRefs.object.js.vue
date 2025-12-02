<script>
import { defineComponent } from "vue";

export default defineComponent({
  name: "TemplateRefs",
  data() {
    return {
      items: ["Item 1", "Item 2", "Item 3"],
      options: ["Option A", "Option B", "Option C"],
      dynamicRefName: "dynamic",
      listItemRefs: [],
      componentRefs: new Map(),
    };
  },
  mounted() {
    console.log("Input ref:", this.$refs.inputEl);
    console.log("Div ref:", this.$refs.divEl);
    this.$refs.inputEl?.focus();
  },
  methods: {
    focusInput() {
      this.$refs.inputEl?.focus();
    },
    getInputValue() {
      return this.$refs.inputEl?.value ?? "";
    },
    scrollDivToTop() {
      const div = this.$refs.divEl;
      if (div) {
        div.scrollTop = 0;
      }
    },
    drawOnCanvas() {
      const canvas = this.$refs.canvasEl;
      const ctx = canvas?.getContext("2d");
      if (ctx) {
        ctx.fillStyle = "blue";
        ctx.fillRect(10, 10, 100, 100);
      }
    },
    callChildMethod() {
      this.$refs.childComp?.focus?.();
    },
    getChildValue() {
      return this.$refs.childComp?.getValue?.() ?? "";
    },
    resetChild() {
      this.$refs.childComp?.reset?.();
    },
    setListItemRef(el, index) {
      if (el) {
        this.listItemRefs[index] = el;
      }
    },
    focusListItem(index) {
      this.listItemRefs[index]?.focus();
    },
    submitForm() {
      this.$refs.formEl?.submit();
    },
    getSelectedValue() {
      return this.$refs.selectEl?.value ?? "";
    },
    getTextareaValue() {
      return this.$refs.textareaEl?.value ?? "";
    },
    setComponentRef(name) {
      return (el) => {
        if (el) {
          this.componentRefs.set(name, el);
        } else {
          this.componentRefs.delete(name);
        }
      };
    },
  },
  expose: ["focusInput", "getInputValue"],
});
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
