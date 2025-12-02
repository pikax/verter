<script setup lang="ts">
import { ref, defineAsyncComponent, type Component } from "vue";

// Async component with loading/error handling
const AsyncHeavy = defineAsyncComponent({
  loader: () => import("./HeavyComponent.vue"),
  loadingComponent: () => import("./LoadingSpinner.vue") as Promise<Component>,
  errorComponent: () => import("./ErrorDisplay.vue") as Promise<Component>,
  delay: 200,
  timeout: 10000,
  suspensible: true,
  onError(error, retry, fail, attempts) {
    if (attempts < 3) {
      retry();
    } else {
      fail();
    }
  },
});

// Simple async component
const AsyncSimple = defineAsyncComponent(() => import("./SimpleAsync.vue"));

const showAsync = ref(false);
const showSuspense = ref(false);
const showTeleport = ref(false);
const showKeepAlive = ref(false);

const currentTab = ref<"A" | "B" | "C">("A");
const tabs = ["A", "B", "C"] as const;
</script>

<template>
  <div>
    <h2>Built-in Components</h2>

    <!-- Suspense -->
    <section>
      <h3>Suspense</h3>
      <button @click="showSuspense = !showSuspense">Toggle Suspense</button>
      <Suspense v-if="showSuspense">
        <template #default>
          <AsyncSimple />
        </template>
        <template #fallback>
          <div>Loading async component...</div>
        </template>
      </Suspense>
    </section>

    <!-- Teleport -->
    <section>
      <h3>Teleport</h3>
      <button @click="showTeleport = !showTeleport">Toggle Modal</button>
      <Teleport to="body" :disabled="false">
        <div v-if="showTeleport" class="modal">
          <p>I'm teleported to body!</p>
          <button @click="showTeleport = false">Close</button>
        </div>
      </Teleport>
    </section>

    <!-- KeepAlive -->
    <section>
      <h3>KeepAlive</h3>
      <div>
        <button
          v-for="tab in tabs"
          :key="tab"
          @click="currentTab = tab"
          :class="{ active: currentTab === tab }"
        >
          Tab {{ tab }}
        </button>
      </div>
      <KeepAlive :include="['TabA', 'TabB']" :exclude="['TabC']" :max="10">
        <component :is="'Tab' + currentTab" :key="currentTab" />
      </KeepAlive>
    </section>

    <!-- Transition -->
    <section>
      <h3>Transition</h3>
      <button @click="showAsync = !showAsync">Toggle</button>
      <Transition
        name="fade"
        mode="out-in"
        appear
        @before-enter="(el) => console.log('before-enter', el)"
        @enter="(el, done) => done()"
        @after-enter="(el) => console.log('after-enter', el)"
        @before-leave="(el) => console.log('before-leave', el)"
        @leave="(el, done) => done()"
        @after-leave="(el) => console.log('after-leave', el)"
      >
        <div v-if="showAsync" class="box">Animated content</div>
      </Transition>
    </section>

    <!-- TransitionGroup -->
    <section>
      <h3>TransitionGroup</h3>
      <TransitionGroup name="list" tag="ul">
        <li v-for="tab in tabs" :key="tab">{{ tab }}</li>
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

.list-enter-active,
.list-leave-active {
  transition: all 0.3s ease;
}
.list-enter-from,
.list-leave-to {
  opacity: 0;
  transform: translateX(30px);
}
</style>
