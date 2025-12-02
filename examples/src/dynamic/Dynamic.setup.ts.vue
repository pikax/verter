<script setup lang="ts">
// Dynamic component patterns for parser testing - Setup API + TypeScript

import { ref, computed, shallowRef, markRaw, type Component } from "vue";

// Mock child components (in real usage, these would be imported)
const ComponentA = { template: "<div>Component A</div>" };
const ComponentB = { template: "<div>Component B</div>" };
const ComponentC = { template: "<div>Component C</div>" };

// Basic dynamic component selection
const currentComponent = shallowRef<Component>(ComponentA);

// String-based component name (for globally registered components)
const componentName = ref<string>("ComponentA");

// Component registry for lookup
const componentRegistry: Record<string, Component> = {
  ComponentA: markRaw(ComponentA),
  ComponentB: markRaw(ComponentB),
  ComponentC: markRaw(ComponentC),
};

// Resolved component from registry
const resolvedComponent = computed(() => componentRegistry[componentName.value]);

// Tab-based component switching
interface Tab {
  id: string;
  label: string;
  component: Component;
}

const tabs = ref<Tab[]>([
  { id: "tab1", label: "Tab 1", component: markRaw(ComponentA) },
  { id: "tab2", label: "Tab 2", component: markRaw(ComponentB) },
  { id: "tab3", label: "Tab 3", component: markRaw(ComponentC) },
]);

const activeTabId = ref<string>("tab1");

const activeTab = computed<Tab | undefined>(() => 
  tabs.value.find((tab) => tab.id === activeTabId.value)
);

// Component with props
interface DynamicProps {
  title?: string;
  count?: number;
  items?: string[];
}

const dynamicProps = ref<DynamicProps>({
  title: "Dynamic Title",
  count: 42,
  items: ["item1", "item2"],
});

// Component with events
const lastEvent = ref<string>("");

function handleDynamicEvent(payload: unknown): void {
  lastEvent.value = JSON.stringify(payload);
}

// Component switching functions
function setComponent(comp: Component): void {
  currentComponent.value = comp;
}

function setComponentByName(name: string): void {
  componentName.value = name;
}

function setActiveTab(tabId: string): void {
  activeTabId.value = tabId;
}

// Conditional component type
type ComponentType = "input" | "textarea" | "select";
const inputType = ref<ComponentType>("input");

// Component key for force re-render
const componentKey = ref<number>(0);

function forceRerender(): void {
  componentKey.value++;
}
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
