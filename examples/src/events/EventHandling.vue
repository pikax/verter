<script setup lang="ts">
// Event handling patterns for parser testing

import { ref } from "vue";

const count = ref(0);
const message = ref("");

// Event handler functions with various signatures
function handleClick(event: MouseEvent): void {
  console.log("Clicked at:", event.clientX, event.clientY);
}

function handleClickWithData(data: string, event: MouseEvent): void {
  console.log("Data:", data, "Event:", event);
}

function handleInput(event: Event): void {
  const target = event.target as HTMLInputElement;
  message.value = target.value;
}

function handleKeydown(event: KeyboardEvent): void {
  console.log("Key:", event.key, "Code:", event.code);
}

function handleSubmit(event: Event): void {
  event.preventDefault();
  console.log("Form submitted");
}

function handleFocus(event: FocusEvent): void {
  console.log("Focused:", event.target);
}

function handleDrag(event: DragEvent): void {
  console.log("Dragging:", event.dataTransfer);
}

function handleWheel(event: WheelEvent): void {
  console.log("Delta:", event.deltaY);
}

function handlePointer(event: PointerEvent): void {
  console.log("Pointer:", event.pointerId, event.pointerType);
}

function handleTouch(event: TouchEvent): void {
  console.log("Touches:", event.touches.length);
}

async function handleAsync(): Promise<void> {
  await new Promise((r) => setTimeout(r, 100));
  count.value++;
}

// Generic handler
function logEvent<T extends Event>(event: T): void {
  console.log("Event type:", event.type);
}
</script>

<template>
  <div>
    <h2>Event Handling</h2>

    <!-- Basic click -->
    <section>
      <h3>Click Events</h3>
      <button @click="handleClick">Basic Click</button>
      <button @click="count++">Inline Increment</button>
      <button @click="() => count++">Arrow Function</button>
      <button @click="(e) => handleClick(e)">Arrow with Event</button>
      <button @click="handleClickWithData('hello', $event)">With Data</button>
    </section>

    <!-- Click modifiers -->
    <section>
      <h3>Click Modifiers</h3>
      <button @click.stop="handleClick">Stop Propagation</button>
      <button @click.prevent="handleClick">Prevent Default</button>
      <button @click.stop.prevent="handleClick">Stop + Prevent</button>
      <button @click.self="handleClick">Self Only</button>
      <button @click.once="handleClick">Once Only</button>
      <button @click.passive="handleClick">Passive</button>
      <button @click.capture="handleClick">Capture Phase</button>
    </section>

    <!-- Mouse button modifiers -->
    <section>
      <h3>Mouse Buttons</h3>
      <button @click.left="handleClick">Left Click</button>
      <button @click.right="handleClick">Right Click</button>
      <button @click.middle="handleClick">Middle Click</button>
    </section>

    <!-- Keyboard events -->
    <section>
      <h3>Keyboard Events</h3>
      <input @keydown="handleKeydown" placeholder="keydown" />
      <input @keyup="handleKeydown" placeholder="keyup" />
      <input @keypress="handleKeydown" placeholder="keypress" />

      <!-- Key modifiers -->
      <input @keydown.enter="handleKeydown" placeholder=".enter" />
      <input @keydown.tab="handleKeydown" placeholder=".tab" />
      <input @keydown.delete="handleKeydown" placeholder=".delete" />
      <input @keydown.esc="handleKeydown" placeholder=".esc" />
      <input @keydown.space="handleKeydown" placeholder=".space" />
      <input @keydown.up="handleKeydown" placeholder=".up" />
      <input @keydown.down="handleKeydown" placeholder=".down" />
      <input @keydown.left="handleKeydown" placeholder=".left" />
      <input @keydown.right="handleKeydown" placeholder=".right" />

      <!-- System modifiers -->
      <input @keydown.ctrl="handleKeydown" placeholder=".ctrl" />
      <input @keydown.alt="handleKeydown" placeholder=".alt" />
      <input @keydown.shift="handleKeydown" placeholder=".shift" />
      <input @keydown.meta="handleKeydown" placeholder=".meta" />

      <!-- Exact modifier -->
      <input @keydown.ctrl.exact="handleKeydown" placeholder=".ctrl.exact" />

      <!-- Combined -->
      <input @keydown.ctrl.enter="handleKeydown" placeholder=".ctrl.enter" />
      <input @keydown.alt.shift.s="handleKeydown" placeholder=".alt.shift.s" />
    </section>

    <!-- Input events -->
    <section>
      <h3>Input Events</h3>
      <input @input="handleInput" placeholder="input" />
      <input @change="handleInput" placeholder="change" />
      <input @focus="handleFocus" placeholder="focus" />
      <input @blur="handleFocus" placeholder="blur" />
    </section>

    <!-- Form events -->
    <section>
      <h3>Form Events</h3>
      <form @submit="handleSubmit" @reset="logEvent">
        <input type="text" />
        <button type="submit">Submit</button>
        <button type="reset">Reset</button>
      </form>
    </section>

    <!-- Mouse events -->
    <section>
      <h3>Mouse Events</h3>
      <div
        @mouseenter="logEvent"
        @mouseleave="logEvent"
        @mouseover="logEvent"
        @mouseout="logEvent"
        @mousemove="logEvent"
        @mousedown="logEvent"
        @mouseup="logEvent"
        @dblclick="handleClick"
        @contextmenu.prevent="logEvent"
      >
        Mouse Events Zone
      </div>
    </section>

    <!-- Drag events -->
    <section>
      <h3>Drag Events</h3>
      <div
        draggable="true"
        @drag="handleDrag"
        @dragstart="handleDrag"
        @dragend="handleDrag"
        @dragenter="handleDrag"
        @dragleave="handleDrag"
        @dragover.prevent="handleDrag"
        @drop.prevent="handleDrag"
      >
        Draggable Element
      </div>
    </section>

    <!-- Scroll & Wheel -->
    <section>
      <h3>Scroll & Wheel</h3>
      <div @scroll="logEvent" @wheel="handleWheel" style="overflow: auto; height: 100px">
        <div style="height: 200px">Scrollable content</div>
      </div>
    </section>

    <!-- Pointer events -->
    <section>
      <h3>Pointer Events</h3>
      <div
        @pointerdown="handlePointer"
        @pointerup="handlePointer"
        @pointermove="handlePointer"
        @pointerenter="handlePointer"
        @pointerleave="handlePointer"
        @pointercancel="handlePointer"
      >
        Pointer Events Zone
      </div>
    </section>

    <!-- Touch events -->
    <section>
      <h3>Touch Events</h3>
      <div
        @touchstart="handleTouch"
        @touchend="handleTouch"
        @touchmove="handleTouch"
        @touchcancel="handleTouch"
      >
        Touch Events Zone
      </div>
    </section>

    <!-- Async handler -->
    <section>
      <h3>Async Handler</h3>
      <button @click="handleAsync">Async Click ({{ count }})</button>
    </section>

    <!-- v-on object syntax -->
    <section>
      <h3>v-on Object Syntax</h3>
      <button v-on="{ click: handleClick, mouseenter: logEvent }">
        Object Syntax
      </button>
    </section>
  </div>
</template>
