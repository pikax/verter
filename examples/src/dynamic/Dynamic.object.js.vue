<script>
// Dynamic component patterns for parser testing - Options API + JavaScript

import { defineComponent, markRaw } from "vue";

// Mock child components
const ComponentA = { template: "<div>Component A</div>" };
const ComponentB = { template: "<div>Component B</div>" };
const ComponentC = { template: "<div>Component C</div>" };

export default defineComponent({
  name: "DynamicObjectJs",

  data() {
    return {
      // Component instances (marked as raw to avoid reactivity overhead)
      ComponentA: markRaw(ComponentA),
      ComponentB: markRaw(ComponentB),
      ComponentC: markRaw(ComponentC),

      // Current component selection
      currentComponent: markRaw(ComponentA),

      // String-based component name
      componentName: "ComponentA",

      // Component registry
      componentRegistry: {
        ComponentA: markRaw(ComponentA),
        ComponentB: markRaw(ComponentB),
        ComponentC: markRaw(ComponentC),
      },

      // Tab-based switching
      tabs: [
        { id: "tab1", label: "Tab 1", component: markRaw(ComponentA) },
        { id: "tab2", label: "Tab 2", component: markRaw(ComponentB) },
        { id: "tab3", label: "Tab 3", component: markRaw(ComponentC) },
      ],

      activeTabId: "tab1",

      // Dynamic props
      dynamicProps: {
        title: "Dynamic Title",
        count: 42,
        items: ["item1", "item2"],
      },

      // Event tracking
      lastEvent: "",

      // Input type
      inputType: "input", // 'input' | 'textarea' | 'select'

      // Component key for force re-render
      componentKey: 0,
    };
  },

  computed: {
    resolvedComponent() {
      return this.componentRegistry[this.componentName];
    },

    activeTab() {
      return this.tabs.find((tab) => tab.id === this.activeTabId);
    },
  },

  methods: {
    setComponent(comp) {
      this.currentComponent = comp;
    },

    setComponentByName(name) {
      this.componentName = name;
    },

    setActiveTab(tabId) {
      this.activeTabId = tabId;
    },

    handleDynamicEvent(payload) {
      this.lastEvent = JSON.stringify(payload);
    },

    forceRerender() {
      this.componentKey++;
    },
  },
});
</script>

<template>
  <div>
    <!-- Basic dynamic component -->
    <component :is="currentComponent" />

    <!-- String-based component name (globally registered) -->
    <component :is="componentName" />

    <!-- Resolved from registry -->
    <component :is="resolvedComponent" />

    <!-- With KeepAlive for state preservation -->
    <KeepAlive>
      <component :is="currentComponent" />
    </KeepAlive>

    <!-- KeepAlive with include/exclude -->
    <KeepAlive :include="['ComponentA', 'ComponentB']">
      <component :is="componentName" />
    </KeepAlive>

    <KeepAlive :exclude="['ComponentC']">
      <component :is="componentName" />
    </KeepAlive>

    <KeepAlive :max="10">
      <component :is="currentComponent" />
    </KeepAlive>

    <!-- Tab-based switching -->
    <div class="tabs">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        :class="{ active: tab.id === activeTabId }"
        @click="setActiveTab(tab.id)"
      >
        {{ tab.label }}
      </button>
    </div>
    <component v-if="activeTab" :is="activeTab.component" />

    <!-- Dynamic component with props -->
    <component :is="currentComponent" v-bind="dynamicProps" />

    <!-- Dynamic component with individual props -->
    <component
      :is="currentComponent"
      :title="dynamicProps.title"
      :count="dynamicProps.count"
    />

    <!-- Dynamic component with events -->
    <component
      :is="currentComponent"
      @custom-event="handleDynamicEvent"
      @update="handleDynamicEvent"
    />

    <!-- Dynamic component with slots -->
    <component :is="currentComponent">
      <template #default>
        Default slot content
      </template>
      <template #header>
        Header slot content
      </template>
      <template #footer>
        Footer slot content
      </template>
    </component>

    <!-- Native element as dynamic component -->
    <component :is="inputType" placeholder="Dynamic input type" />

    <!-- Force re-render with key -->
    <component :is="currentComponent" :key="componentKey" />

    <!-- Conditional with v-if -->
    <component v-if="currentComponent" :is="currentComponent" />

    <!-- Controls -->
    <div class="controls">
      <button @click="setComponent(ComponentA)">Component A</button>
      <button @click="setComponent(ComponentB)">Component B</button>
      <button @click="setComponent(ComponentC)">Component C</button>
      <button @click="setComponentByName('ComponentA')">By Name: A</button>
      <button @click="setComponentByName('ComponentB')">By Name: B</button>
      <button @click="setComponentByName('ComponentC')">By Name: C</button>
      <button @click="forceRerender">Force Re-render</button>
    </div>

    <p>Last Event: {{ lastEvent }}</p>
    <p>Component Key: {{ componentKey }}</p>
  </div>
</template>
