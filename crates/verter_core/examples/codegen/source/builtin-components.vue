<template>
  <div>
    <!-- Teleport: render content in different DOM location -->
    <Teleport to="body">
      <div class="modal">Modal teleported to body</div>
    </Teleport>

    <Teleport to="#modals">
      <div class="modal">Modal teleported to #modals</div>
    </Teleport>

    <!-- Teleport with disabled prop -->
    <Teleport to="body" :disabled="!showModal">
      <div v-if="showModal" class="modal">Conditional teleport</div>
    </Teleport>

    <!-- Transition: single element transitions -->
    <Transition name="fade">
      <div v-if="show">Fade transition</div>
    </Transition>

    <Transition name="slide" mode="out-in">
      <div :key="currentView">{{ currentView }}</div>
    </Transition>

    <!-- Transition with hooks -->
    <Transition
      @before-enter="onBeforeEnter"
      @enter="onEnter"
      @after-enter="onAfterEnter"
      @leave="onLeave"
    >
      <div v-if="visible">Transition with hooks</div>
    </Transition>

    <!-- TransitionGroup: list transitions -->
    <TransitionGroup name="list" tag="ul">
      <li v-for="item in items" :key="item.id">{{ item.name }}</li>
    </TransitionGroup>

    <!-- KeepAlive: cache component instances -->
    <KeepAlive>
      <component :is="currentComponent" />
    </KeepAlive>

    <KeepAlive :include="['CompA', 'CompB']" :max="10">
      <component :is="activeComp" />
    </KeepAlive>

    <!-- Suspense: async component loading -->
    <Suspense>
      <template #default>
        <AsyncComponent />
      </template>
      <template #fallback>
        <div>Loading...</div>
      </template>
    </Suspense>
  </div>
</template>

<script setup>
import { ref } from "vue";
import AsyncComponent from "./AsyncComponent.vue";

const showModal = ref(false);
const show = ref(true);
const visible = ref(true);
const currentView = ref("home");
const items = ref([
  { id: 1, name: "Item 1" },
  { id: 2, name: "Item 2" },
]);
const currentComponent = ref("CompA");
const activeComp = ref("CompA");

const onBeforeEnter = (el) => {};
const onEnter = (el, done) => {
  done();
};
const onAfterEnter = (el) => {};
const onLeave = (el, done) => {
  done();
};
</script>
