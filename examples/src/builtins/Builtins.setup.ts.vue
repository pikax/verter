<script setup lang="ts">
import { ref, shallowRef, defineAsyncComponent, type Component } from "vue";

// Teleport target
const teleportTarget = ref("#modal-container");
const showModal = ref(false);
const showTooltip = ref(false);
const tooltipDisabled = ref(false);

// KeepAlive state
const currentTab = ref<"tab1" | "tab2" | "tab3">("tab1");
const cachedComponents = ref<string[]>(["Tab1", "Tab2"]);
const maxCached = ref(10);

// Suspense state
const AsyncComponent = defineAsyncComponent(() =>
  new Promise<{ default: Component }>((resolve) => {
    setTimeout(() => {
      resolve({
        default: {
          template: "<div>Async content loaded!</div>",
        },
      });
    }, 1000);
  })
);

const showAsync = ref(true);
const suspenseKey = ref(0);

// Transition state
const transitionShow = ref(true);
const transitionName = ref<"fade" | "slide" | "zoom">("fade");
const transitionMode = ref<"out-in" | "in-out" | "default">("out-in");

// TransitionGroup list
const items = ref([
  { id: 1, text: "Item 1" },
  { id: 2, text: "Item 2" },
  { id: 3, text: "Item 3" },
]);
let nextId = 4;

// Methods
function toggleModal() {
  showModal.value = !showModal.value;
}

function toggleTooltip() {
  showTooltip.value = !showTooltip.value;
}

function setTab(tab: "tab1" | "tab2" | "tab3") {
  currentTab.value = tab;
}

function reloadAsync() {
  suspenseKey.value++;
}

function toggleTransition() {
  transitionShow.value = !transitionShow.value;
}

function setTransitionName(name: "fade" | "slide" | "zoom") {
  transitionName.value = name;
}

function addItem() {
  items.value.push({ id: nextId++, text: `Item ${nextId - 1}` });
}

function removeItem(id: number) {
  const index = items.value.findIndex((item) => item.id === id);
  if (index > -1) {
    items.value.splice(index, 1);
  }
}

function shuffleItems() {
  items.value = [...items.value].sort(() => Math.random() - 0.5);
}

// Transition hooks
function onBeforeEnter(el: Element) {
  console.log("before-enter", el);
}

function onEnter(el: Element, done: () => void) {
  console.log("enter", el);
  done();
}

function onAfterEnter(el: Element) {
  console.log("after-enter", el);
}

function onBeforeLeave(el: Element) {
  console.log("before-leave", el);
}

function onLeave(el: Element, done: () => void) {
  console.log("leave", el);
  done();
}

function onAfterLeave(el: Element) {
  console.log("after-leave", el);
}

// KeepAlive hooks
function onActivated() {
  console.log("Component activated");
}

function onDeactivated() {
  console.log("Component deactivated");
}

// Suspense events
function onPending() {
  console.log("Suspense pending");
}

function onResolve() {
  console.log("Suspense resolved");
}

function onFallback() {
  console.log("Suspense fallback shown");
}
</script>

<template>
  <div class="builtins-demo">
    <!-- Teleport -->
    <section>
      <h3>Teleport</h3>
      <button @click="toggleModal">Toggle Modal</button>
      <Teleport :to="teleportTarget" :disabled="false">
        <div v-if="showModal" class="modal">
          Modal content teleported to body
          <button @click="toggleModal">Close</button>
        </div>
      </Teleport>
      
      <!-- Conditional teleport -->
      <Teleport to="body" :disabled="tooltipDisabled">
        <div v-if="showTooltip" class="tooltip">
          Tooltip content
        </div>
      </Teleport>
      <button @click="toggleTooltip">Toggle Tooltip</button>
      <label>
        <input type="checkbox" v-model="tooltipDisabled" />
        Disable teleport
      </label>
    </section>

    <!-- KeepAlive -->
    <section>
      <h3>KeepAlive</h3>
      <div class="tabs">
        <button
          v-for="tab in ['tab1', 'tab2', 'tab3'] as const"
          :key="tab"
          :class="{ active: currentTab === tab }"
          @click="setTab(tab)"
        >
          {{ tab }}
        </button>
      </div>
      <KeepAlive :include="cachedComponents" :max="maxCached">
        <component
          :is="currentTab"
          @vue:mounted="onActivated"
          @vue:unmounted="onDeactivated"
        />
      </KeepAlive>
      <!-- KeepAlive with exclude -->
      <KeepAlive exclude="Tab3">
        <component :is="currentTab" />
      </KeepAlive>
    </section>

    <!-- Suspense -->
    <section>
      <h3>Suspense</h3>
      <button @click="reloadAsync">Reload</button>
      <button @click="showAsync = !showAsync">Toggle</button>
      <Suspense
        :key="suspenseKey"
        @pending="onPending"
        @resolve="onResolve"
        @fallback="onFallback"
      >
        <template #default>
          <AsyncComponent v-if="showAsync" />
        </template>
        <template #fallback>
          <div class="loading">Loading...</div>
        </template>
      </Suspense>
    </section>

    <!-- Transition -->
    <section>
      <h3>Transition</h3>
      <div class="controls">
        <button @click="toggleTransition">Toggle</button>
        <select v-model="transitionName">
          <option value="fade">Fade</option>
          <option value="slide">Slide</option>
          <option value="zoom">Zoom</option>
        </select>
        <select v-model="transitionMode">
          <option value="out-in">Out-In</option>
          <option value="in-out">In-Out</option>
          <option value="default">Default</option>
        </select>
      </div>
      <Transition
        :name="transitionName"
        :mode="transitionMode"
        :appear="true"
        :css="true"
        @before-enter="onBeforeEnter"
        @enter="onEnter"
        @after-enter="onAfterEnter"
        @before-leave="onBeforeLeave"
        @leave="onLeave"
        @after-leave="onAfterLeave"
      >
        <div v-if="transitionShow" class="box">
          Transitioning content
        </div>
      </Transition>
    </section>

    <!-- TransitionGroup -->
    <section>
      <h3>TransitionGroup</h3>
      <div class="controls">
        <button @click="addItem">Add</button>
        <button @click="shuffleItems">Shuffle</button>
      </div>
      <TransitionGroup name="list" tag="ul" class="list">
        <li
          v-for="item in items"
          :key="item.id"
          class="list-item"
        >
          {{ item.text }}
          <button @click="removeItem(item.id)">×</button>
        </li>
      </TransitionGroup>
    </section>
  </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.slide-enter-active,
.slide-leave-active {
  transition: transform 0.3s ease;
}
.slide-enter-from {
  transform: translateX(-100%);
}
.slide-leave-to {
  transform: translateX(100%);
}

.zoom-enter-active,
.zoom-leave-active {
  transition: transform 0.3s ease;
}
.zoom-enter-from,
.zoom-leave-to {
  transform: scale(0);
}

.list-enter-active,
.list-leave-active {
  transition: all 0.5s ease;
}
.list-enter-from,
.list-leave-to {
  opacity: 0;
  transform: translateX(30px);
}
.list-move {
  transition: transform 0.5s ease;
}
</style>
