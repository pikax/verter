<script setup lang="ts">
import { provide, ref, readonly } from "vue";
import {
  ThemeKey,
  UserKey,
  CounterKey,
  NotifyKey,
  ConfigKey,
  type UserContext,
  type NotifyFn,
} from "./keys";
import Consumer from "./Consumer.vue";
import DeepNested from "./DeepNested.vue";

// Provide simple value
provide(ThemeKey, "dark");

// Provide complex object
const user: UserContext = {
  id: 1,
  name: "John Doe",
  email: "john@example.com",
  isAdmin: true,
};
provide(UserKey, user);

// Provide reactive value (readonly to prevent mutation from consumers)
const counter = ref(0);
provide(CounterKey, readonly(counter));

// Provide function
const notifications = ref<string[]>([]);
const notify: NotifyFn = (message, type) => {
  notifications.value.push(`[${type.toUpperCase()}] ${message}`);
};
provide(NotifyKey, notify);

// Provide config
provide(ConfigKey, {
  apiUrl: "https://api.example.com",
  debug: true,
  version: "2.0.0",
});

function incrementCounter() {
  counter.value++;
}
</script>

<template>
  <div>
    <h2>Provider Component</h2>

    <section>
      <h3>Counter (controlled by provider)</h3>
      <p>Counter: {{ counter }}</p>
      <button @click="incrementCounter">Increment from Provider</button>
    </section>

    <section>
      <h3>Notifications</h3>
      <ul>
        <li v-for="(note, idx) in notifications" :key="idx">{{ note }}</li>
      </ul>
    </section>

    <hr />

    <Consumer />

    <hr />

    <DeepNested />
  </div>
</template>
