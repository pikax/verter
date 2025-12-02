<script>
import {
  defineComponent,
  defineAsyncComponent,
  shallowRef,
  markRaw,
} from "vue";

const AsyncBasic = defineAsyncComponent(() =>
  import("./AsyncChild.vue")
);

const AsyncWithLoading = defineAsyncComponent({
  loader: () => import("./AsyncChild.vue"),
  loadingComponent: {
    template: '<div class="loading">Loading...</div>',
  },
  delay: 200,
});

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

const lazyComponents = {
  dashboard: defineAsyncComponent(() => import("./Dashboard.vue")),
  settings: defineAsyncComponent(() => import("./Settings.vue")),
  profile: defineAsyncComponent(() => import("./Profile.vue")),
};

export default defineComponent({
  name: "Async",
  components: {
    AsyncBasic,
    AsyncWithLoading,
    AsyncWithError,
    AsyncFull,
  },
  data() {
    return {
      showAsync: false,
      showDynamic: false,
      componentName: "ComponentA",
      currentRoute: "dashboard",
      userId: 1,
      preloadedComponent: null,
      lazyComponents,
    };
  },
  computed: {
    AsyncDynamic() {
      return defineAsyncComponent(() =>
        this.componentName === "ComponentA"
          ? import("./ComponentA.vue")
          : import("./ComponentB.vue")
      );
    },
    currentLazyComponent() {
      return this.lazyComponents[this.currentRoute];
    },
  },
  methods: {
    toggleAsync() {
      this.showAsync = !this.showAsync;
    },
    toggleDynamic() {
      this.showDynamic = !this.showDynamic;
    },
    switchComponent() {
      this.componentName = this.componentName === "ComponentA" ? "ComponentB" : "ComponentA";
    },
    setRoute(route) {
      this.currentRoute = route;
    },
    async preloadComponent() {
      const module = await import("./HeavyComponent.vue");
      this.preloadedComponent = markRaw(module.default);
    },
  },
});
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
      <component :is="currentLazyComponent" />
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
