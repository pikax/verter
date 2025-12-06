<script setup lang="ts">
import {
  computed,
  defineAsyncComponent,
  ref,
  shallowRef,
  markRaw,
  type Component,
  type AsyncComponentLoader,
} from "vue";

import Basic from "./AsyncChild.vue";

// Basic async component (without generic - env.d.ts limitation)
const AsyncBasic = defineAsyncComponent(() => import("./AsyncChild.vue"));

// Async component with loading state
const AsyncWithLoading = defineAsyncComponent({
  loader: () => import("./AsyncChild.vue"),
  loadingComponent: {
    template: '<div class="loading">Loading...</div>',
  },
  delay: 200, // Show loading after 200ms
});

// Async component with error handling
const AsyncWithError = defineAsyncComponent({
  loader: () => import("./AsyncChild.vue"),
  errorComponent: {
    template: '<div class="error">Failed to load component</div>',
  },
  timeout: 3000, // Timeout after 3 seconds
  onError(error, retry, fail, attempts) {
    if (attempts <= 3) {
      retry();
    } else {
      fail();
    }
  },
});

// Full options async component
const AsyncFull = defineAsyncComponent({
  loader: () => import("./AsyncChild.vue"),
  loadingComponent: {
    template: '<div class="loading">Loading component...</div>',
  },
  errorComponent: {
    template: '<div class="error">Error loading component</div>',
  },
  delay: 200,
  timeout: 10000,
  suspensible: true, // Integrate with Suspense
  onError(error, retry, fail, attempts) {
    console.error(`Async component error (attempt ${attempts}):`, error);
    if (error.message.includes("network") && attempts <= 3) {
      retry();
    } else {
      fail();
    }
  },
});

// Dynamic async component based on condition
const componentName = ref<"ComponentA" | "ComponentB">("ComponentA");

const AsyncDynamic = computed(() =>
  defineAsyncComponent(() =>
    componentName.value === "ComponentA"
      ? import("./ComponentA.vue")
      : import("./ComponentB.vue")
  )
);

// Lazy loaded components map
const lazyComponents = {
  dashboard: defineAsyncComponent(() => import("./Dashboard.vue")),
  settings: defineAsyncComponent(() => import("./Settings.vue")),
  profile: defineAsyncComponent(() => import("./Profile.vue")),
};

const currentRoute = ref<"dashboard" | "settings" | "profile">("dashboard");

// Component with typed props (generic removed due to env.d.ts shim limitation)
interface UserProfileProps {
  userId: number;
  showAvatar?: boolean;
}

// Props type is for documentation - actual component uses its own props
const AsyncUserProfile = defineAsyncComponent(
  () => import("./UserProfile.vue")
);

// Preload component manually
const preloadedComponent = shallowRef<Component | null>(null);

async function preloadComponent() {
  const module = await import("./HeavyComponent.vue");
  preloadedComponent.value = markRaw(module.default);
}

// Factory function for async components
function createAsyncComponent(
  loader: () => Promise<typeof import("*.vue")>,
  options: { delay?: number; timeout?: number } = {}
) {
  return defineAsyncComponent({
    loader,
    loadingComponent: {
      template: '<div class="spinner">⏳</div>',
    },
    errorComponent: {
      template: '<div class="error">❌ Failed</div>',
    },
    delay: options.delay ?? 200,
    timeout: options.timeout ?? 10000,
  });
}

const CustomAsyncComponent = createAsyncComponent(
  () => import("./CustomComponent.vue"),
  { delay: 100, timeout: 5000 }
);

// State
const showAsync = ref(false);
const showDynamic = ref(false);
const userId = ref(1);

function toggleAsync() {
  showAsync.value = !showAsync.value;
}

function toggleDynamic() {
  showDynamic.value = !showDynamic.value;
}

function switchComponent() {
  componentName.value =
    componentName.value === "ComponentA" ? "ComponentB" : "ComponentA";
}

function setRoute(route: "dashboard" | "settings" | "profile") {
  currentRoute.value = route;
}
</script>

<template>
  <div class="async-demo">
    <!-- Basic async component -->
    <section>
      <h3>Basic Async Component</h3>
      <button @click="toggleAsync">Toggle Async</button>
      <AsyncBasic v-if="showAsync" message="Hello from async!" />
      <Basic v-if="showAsync" message="Hello from async!" />
      <Basic v-if="showAsync" />
    </section>

    <!-- Async with loading -->
    <section>
      <h3>Async with Loading State</h3>
      <Suspense>
        <template #default>
          <AsyncWithLoading v-if="showAsync" />
        </template>
        <template #fallback>
          <div>Suspense fallback...</div>
        </template>
      </Suspense>
    </section>

    <!-- Dynamic async component -->
    <section>
      <h3>Dynamic Async Component</h3>
      <button @click="switchComponent">
        Switch to {{ componentName === "ComponentA" ? "B" : "A" }}
      </button>
      <button @click="toggleDynamic">Toggle Show</button>
      <component :is="AsyncDynamic" v-if="showDynamic" />
    </section>

    <!-- Route-based lazy loading -->
    <section>
      <h3>Route-based Lazy Loading</h3>
      <nav>
        <button
          v-for="route in ['dashboard', 'settings', 'profile'] as const"
          :key="route"
          :class="{ active: currentRoute === route }"
          @click="setRoute(route)"
        >
          {{ route }}
        </button>
      </nav>
      <component :is="lazyComponents[currentRoute]" />
    </section>

    <!-- Typed async component -->
    <section>
      <h3>Typed Async Component</h3>
      <label> User ID: <input type="number" v-model.number="userId" /> </label>
      <AsyncUserProfile :user-id="userId" :show-avatar="true" />
    </section>

    <!-- Preloaded component -->
    <section>
      <h3>Preloaded Component</h3>
      <button @click="preloadComponent">Preload Component</button>
      <component :is="preloadedComponent" v-if="preloadedComponent" />
    </section>

    <!-- Full options -->
    <section>
      <h3>Full Options Async</h3>
      <Suspense>
        <template #default>
          <AsyncFull v-if="showAsync" />
        </template>
        <template #fallback>
          <div>Loading via Suspense...</div>
        </template>
      </Suspense>
    </section>
  </div>
</template>

<style scoped>
.async-demo section {
  margin-bottom: 20px;
  padding: 15px;
  border: 1px solid #ddd;
}

.loading {
  color: blue;
  padding: 10px;
}

.error {
  color: red;
  padding: 10px;
}

.spinner {
  font-size: 24px;
}

nav button {
  margin-right: 10px;
}

nav button.active {
  font-weight: bold;
  text-decoration: underline;
}
</style>
