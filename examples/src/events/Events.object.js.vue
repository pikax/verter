<script>
// Event handling patterns for parser testing - Options API + JavaScript

import { defineComponent } from "vue";

export default defineComponent({
  name: "EventsObjectJs",

  data() {
    return {
      clickCount: 0,
      inputValue: "",
      lastKey: "",
      mousePosition: { x: 0, y: 0 },
      formData: { name: "", email: "" },
      dynamicEventName: "click",
    };
  },

  methods: {
    handleClick(event) {
      this.clickCount++;
      console.log("Clicked!", event);
    },

    handleInput(event) {
      this.inputValue = event.target.value;
    },

    handleKeydown(event) {
      this.lastKey = event.key;
    },

    handleEnter() {
      console.log("Enter pressed");
    },

    handleEscape() {
      console.log("Escape pressed");
    },

    handleMouseMove(event) {
      this.mousePosition = { x: event.clientX, y: event.clientY };
    },

    handleMouseEnter() {
      console.log("Mouse entered");
    },

    handleMouseLeave() {
      console.log("Mouse left");
    },

    handleSubmit(event) {
      event.preventDefault();
      console.log("Form submitted", this.formData);
    },

    handleFormSubmitWithModifier() {
      console.log("Form submitted (with .prevent modifier)", this.formData);
    },

    handleWithArgs(arg1, arg2, event) {
      console.log("Args:", arg1, arg2, event);
    },

    handleContextMenu(event) {
      event.preventDefault();
      console.log("Context menu", event);
    },

    handleFocus(event) {
      console.log("Focused", event.target);
    },

    handleBlur(event) {
      console.log("Blurred", event.target);
    },

    handleScroll(event) {
      console.log("Scrolled", event);
    },

    setLastKey(key) {
      this.lastKey = key;
    },
  },
});
</script>

<template>
  <div>
    <!-- Basic click event -->
    <button @click="handleClick">Click me ({{ clickCount }})</button>

    <!-- Shorthand vs full syntax -->
    <button v-on:click="handleClick">Full syntax</button>
    <button @click="handleClick">Shorthand syntax</button>

    <!-- Inline expression -->
    <button @click="clickCount++">Inline increment</button>
    <button @click="clickCount = 0">Reset</button>

    <!-- Method with $event -->
    <button @click="handleClick($event)">With $event</button>

    <!-- Multiple arguments -->
    <button @click="handleWithArgs('hello', 42, $event)">With args</button>

    <!-- Event modifiers -->
    <button @click.stop="handleClick">Stop propagation</button>
    <button @click.prevent="handleClick">Prevent default</button>
    <button @click.stop.prevent="handleClick">Stop + Prevent</button>
    <button @click.capture="handleClick">Capture mode</button>
    <button @click.once="handleClick">Fire once only</button>
    <button @click.self="handleClick">Only if target is self</button>
    <a href="#" @click.prevent="handleClick">Prevent link</a>

    <!-- Passive modifier (for scroll performance) -->
    <div @scroll.passive="handleScroll" style="overflow: auto; height: 100px;">
      <div style="height: 200px;">Scroll content</div>
    </div>

    <!-- Key modifiers -->
    <input @keydown="handleKeydown" placeholder="Press any key" />
    <input @keyup.enter="handleEnter" placeholder="Press Enter" />
    <input @keyup.esc="handleEscape" placeholder="Press Escape" />
    <input @keyup.tab="setLastKey('Tab')" placeholder="Press Tab" />
    <input @keyup.delete="setLastKey('Delete')" placeholder="Press Delete/Backspace" />
    <input @keyup.space="setLastKey('Space')" placeholder="Press Space" />
    <input @keyup.up="setLastKey('Up')" placeholder="Press Up" />
    <input @keyup.down="setLastKey('Down')" placeholder="Press Down" />
    <input @keyup.left="setLastKey('Left')" placeholder="Press Left" />
    <input @keyup.right="setLastKey('Right')" placeholder="Press Right" />

    <!-- System modifier keys -->
    <button @click.ctrl="handleClick">Ctrl+Click</button>
    <button @click.alt="handleClick">Alt+Click</button>
    <button @click.shift="handleClick">Shift+Click</button>
    <button @click.meta="handleClick">Meta+Click (Cmd/Win)</button>
    <input @keyup.ctrl.enter="handleEnter" placeholder="Ctrl+Enter" />
    <input @keyup.alt.enter="handleEnter" placeholder="Alt+Enter" />

    <!-- Exact modifier -->
    <button @click.ctrl.exact="handleClick">Only Ctrl+Click (no other keys)</button>
    <button @click.exact="handleClick">Only Click (no modifiers)</button>

    <!-- Mouse button modifiers -->
    <button @click.left="handleClick">Left click only</button>
    <button @click.right="handleContextMenu">Right click only</button>
    <button @click.middle="handleClick">Middle click only</button>
    <button @contextmenu.prevent="handleContextMenu">Context menu</button>

    <!-- Mouse events -->
    <div
      @mousemove="handleMouseMove"
      @mouseenter="handleMouseEnter"
      @mouseleave="handleMouseLeave"
      style="width: 200px; height: 100px; background: #eee;"
    >
      Mouse: {{ mousePosition.x }}, {{ mousePosition.y }}
    </div>

    <!-- Input events -->
    <input @input="handleInput" :value="inputValue" placeholder="Type something" />
    <input @change="handleInput" placeholder="Change event" />
    <input @focus="handleFocus" @blur="handleBlur" placeholder="Focus/blur" />

    <!-- Form events -->
    <form @submit="handleSubmit">
      <input v-model="formData.name" placeholder="Name" />
      <button type="submit">Submit (manual prevent)</button>
    </form>

    <form @submit.prevent="handleFormSubmitWithModifier">
      <input v-model="formData.email" placeholder="Email" />
      <button type="submit">Submit (with .prevent)</button>
    </form>

    <!-- Dynamic event name -->
    <button v-on:[dynamicEventName]="handleClick">Dynamic event</button>

    <!-- Multiple events on same element -->
    <input
      @focus="handleFocus"
      @blur="handleBlur"
      @input="handleInput"
      @keydown.enter="handleEnter"
      placeholder="Multiple events"
    />

    <p>Last key: {{ lastKey }}</p>
  </div>
</template>
