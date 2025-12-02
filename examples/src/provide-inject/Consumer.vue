<script setup lang="ts">
import { inject } from "vue";
import {
  ThemeKey,
  UserKey,
  CounterKey,
  NotifyKey,
  ConfigKey,
  defaultConfig,
} from "./keys";

// Inject with type safety (may be undefined if not provided)
const theme = inject(ThemeKey);
const user = inject(UserKey);
const counter = inject(CounterKey);
const notify = inject(NotifyKey);

// Inject with default value (never undefined)
const config = inject(ConfigKey, defaultConfig);

// Inject with factory default
const factoryConfig = inject(ConfigKey, () => ({
  apiUrl: "/fallback",
  debug: false,
  version: "0.0.0",
}), true);

function sendNotification() {
  if (notify) {
    notify("Hello from Consumer!", "success");
  }
}
</script>

<template>
  <div>
    <h3>Consumer Component</h3>

    <section>
      <h4>Theme</h4>
      <p>Theme: {{ theme ?? "not provided" }}</p>
    </section>

    <section>
      <h4>User</h4>
      <p v-if="user">
        {{ user.name }} ({{ user.email }})
        <span v-if="user.isAdmin">[Admin]</span>
      </p>
      <p v-else>No user provided</p>
    </section>

    <section>
      <h4>Counter (readonly)</h4>
      <p>Counter: {{ counter ?? "not provided" }}</p>
    </section>

    <section>
      <h4>Config</h4>
      <pre>{{ JSON.stringify(config, null, 2) }}</pre>
    </section>

    <section>
      <h4>Notify</h4>
      <button @click="sendNotification" :disabled="!notify">
        Send Notification
      </button>
    </section>
  </div>
</template>
