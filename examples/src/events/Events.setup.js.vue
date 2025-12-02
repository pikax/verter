<script setup>
// Event handling patterns for parser testing - Setup API + JavaScript

import { ref } from "vue";

// Event data
const clickCount = ref(0);
const inputValue = ref("");
const lastKey = ref("");
const mousePosition = ref({ x: 0, y: 0 });
const formData = ref({ name: "", email: "" });

// Basic click handler
function handleClick(event) {
  clickCount.value++;
  console.log("Clicked!", event);
}

// Input event handler
function handleInput(event) {
  inputValue.value = event.target.value;
}

// Keyboard event handler
function handleKeydown(event) {
  lastKey.value = event.key;
}

// Key-specific handlers
function handleEnter() {
  console.log("Enter pressed");
}

function handleEscape() {
  console.log("Escape pressed");
}

// Mouse event handlers
function handleMouseMove(event) {
  mousePosition.value = { x: event.clientX, y: event.clientY };
}

function handleMouseEnter() {
  console.log("Mouse entered");
}

function handleMouseLeave() {
  console.log("Mouse left");
}

// Form event handlers
function handleSubmit(event) {
  event.preventDefault();
  console.log("Form submitted", formData.value);
}

function handleFormSubmitWithModifier() {
  console.log("Form submitted (with .prevent modifier)", formData.value);
}

// Custom event handler with payload
function handleCustomEvent(payload) {
  console.log("Custom event received", payload);
}

// Multiple arguments handler
function handleWithArgs(arg1, arg2, event) {
  console.log("Args:", arg1, arg2, event);
}

// Inline expression variables
const dynamicEventName = ref("click");

// Right click handler
function handleContextMenu(event) {
  event.preventDefault();
  console.log("Context menu", event);
}

// Focus/blur handlers
function handleFocus(event) {
  console.log("Focused", event.target);
}

function handleBlur(event) {
  console.log("Blurred", event.target);
}

// Scroll handler
function handleScroll(event) {
  console.log("Scrolled", event);
}
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
    <input @keyup.tab="lastKey = 'Tab'" placeholder="Press Tab" />
    <input @keyup.delete="lastKey = 'Delete'" placeholder="Press Delete/Backspace" />
    <input @keyup.space="lastKey = 'Space'" placeholder="Press Space" />
    <input @keyup.up="lastKey = 'Up'" placeholder="Press Up" />
    <input @keyup.down="lastKey = 'Down'" placeholder="Press Down" />
    <input @keyup.left="lastKey = 'Left'" placeholder="Press Left" />
    <input @keyup.right="lastKey = 'Right'" placeholder="Press Right" />

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
