<script setup>
import {
  defineAsyncComponent,
  ref,
  shallowRef,
  markRaw,
  computed,
} from "vue";

// Basic async component
const AsyncBasic = defineAsyncComponent(() =>
  import("./AsyncChild.vue")
);

// Async component with loading state
const AsyncWithLoading = defineAsyncComponent({
  loader: () => import("./AsyncChild.vue"),
  loadingComponent: {
    template: '<div class="loading">Loading...</div>',
  },
  delay: 200,
});

// Async component with error handling
const AsyncWithError = defineAsyncComponent({
  loader: () => import("./AsyncChild.vue"),
  errorComponent: {
    template: '<div class="error">Failed to load component</div>',
  },
  timeout: 3000,
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
  suspensible: true,
  onError(error, retry, fail, attempts) {
    console.error(`Async component error (attempt ${attempts}):`, error);
    if (error.message.includes("network") && attempts <= 3) {
      retry();
    } else {
      fail();
    }
  },
});

// Dynamic async component
const componentName = ref("ComponentA");

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

const currentRoute = ref("dashboard");

// Preload component manually
const preloadedComponent = shallowRef(null);

async function preloadComponent() {
  const module = await import("./HeavyComponent.vue");
  preloadedComponent.value = markRaw(module.default);
}

// Factory function
function createAsyncComponent(loader, options = {}) {
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
  componentName.value = componentName.value === "ComponentA" ? "ComponentB" : "ComponentA";
}

function setRoute(route) {
  currentRoute.value = route;
}
</script>

<template>
  <div class="async-demo">
    <section>
      <h3>Basic Async Component</h3>
      <button @click="toggleAsync">Toggle Async</button>
      <AsyncBasic v-if="showAsync" message="Hello from async!" />
    </section>

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

    <section>
      <h3>Dynamic Async Component</h3>
      <button @click="switchComponent">
        Switch to {{ componentName === "ComponentA" ? "B" : "A" }}
      </button>
      <button @click="toggleDynamic">Toggle Show</button>
      <component :is="AsyncDynamic" v-if="showDynamic" />
    </section>

    <section>
      <h3>Route-based Lazy Loading</h3>
      <nav>
        <button
          v-for="route in ['dashboard', 'settings', 'profile']"
          :key="route"
          :class="{ active: currentRoute === route }"
          @click="setRoute(route)"
        >
          {{ route }}
        </button>
      </nav>
      <component :is="lazyComponents[currentRoute]" />
    </section>

    <section>
      <h3>Preloaded Component</h3>
      <button @click="preloadComponent">Preload Component</button>
      <component :is="preloadedComponent" v-if="preloadedComponent" />
    </section>

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
