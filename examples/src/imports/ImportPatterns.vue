<script setup lang="ts">
// Import patterns for parser testing

// Named imports
import { ref, computed, watch, defineAsyncComponent, type Ref } from "vue";

// Default import
import DefaultComp from "./DefaultExport.vue";

// Namespace import
import * as VueAll from "vue";

// Type-only imports
import type { PropType, ComputedRef, ComponentPublicInstance } from "vue";
import type { User, Config, SomeType } from "./types.ts";

// Default import from component with named export (type moved to types.ts)
import DefComp from "./MixedExport.vue";

// Side-effect import
import "./side-effects";

// Re-exports would be in separate file
// export { ref, computed } from "vue";
// export * from "./utils";
// export * as utils from "./utils";
// export { default } from "./Component.vue";

// Dynamic imports
const AsyncComp = defineAsyncComponent(() => import("./AsyncComponent.vue"));

// Import assertions (when supported)
// import json from "./data.json" assert { type: "json" };

// Interface for local use
interface LocalUser {
  id: number;
  name: string;
}

// Type alias
type UserId = number;
type UserMap = Map<UserId, LocalUser>;

// Using imported types
const userRef = ref<LocalUser | null>(null);
const configRef = ref<Config | undefined>();
</script>

<template>
  <div>
    <h2>Import Patterns</h2>
    <DefaultComp />
    <AsyncComp />
  </div>
</template>
