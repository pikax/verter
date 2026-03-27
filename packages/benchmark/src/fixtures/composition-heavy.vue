<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, provide, inject } from "vue";

interface User {
  id: number;
  name: string;
  email: string;
  avatar?: string;
  role: "admin" | "user" | "guest";
  permissions: string[];
  settings: UserSettings;
}

interface UserSettings {
  theme: "light" | "dark" | "auto";
  language: string;
  notifications: boolean;
  emailFrequency: "realtime" | "daily" | "weekly" | "never";
}

interface Activity {
  id: string;
  type: "login" | "logout" | "update" | "delete" | "create";
  timestamp: Date;
  details: string;
}

// Props
const props = defineProps<{
  userId: number;
  initialView?: "profile" | "settings" | "activity";
}>();

// Emits
const emit = defineEmits<{
  userUpdated: [user: User];
  settingsChanged: [settings: UserSettings];
  viewChanged: [view: string];
}>();

// State
const user = ref<User | null>(null);
const activities = ref<Activity[]>([]);
const isLoading = ref(false);
const error = ref<string | null>(null);
const currentView = ref(props.initialView || "profile");
const isDirty = ref(false);
const saveStatus = ref<"idle" | "saving" | "saved" | "error">("idle");

// Computed
const hasPermission = computed(() => (permission: string) => {
  return user.value?.permissions.includes(permission) || user.value?.role === "admin";
});

const canEditProfile = computed(() => hasPermission.value("edit:profile"));
const canViewActivity = computed(() => hasPermission.value("view:activity"));
const isAdmin = computed(() => user.value?.role === "admin");

const activityStats = computed(() => {
  const stats = {
    logins: 0,
    updates: 0,
    creates: 0,
    deletes: 0,
  };
  activities.value.forEach((a) => {
    if (a.type === "login") stats.logins++;
    else if (a.type === "update") stats.updates++;
    else if (a.type === "create") stats.creates++;
    else if (a.type === "delete") stats.deletes++;
  });
  return stats;
});

const displayName = computed(() => {
  if (!user.value) return "Unknown User";
  return user.value.name || user.value.email || `User #${user.value.id}`;
});

// Watchers
watch(
  () => props.userId,
  async (newId) => {
    if (newId) {
      await loadUser(newId);
    }
  },
  { immediate: true },
);

watch(
  user,
  (newUser) => {
    if (newUser) {
      isDirty.value = true;
      emit("userUpdated", newUser);
    }
  },
  { deep: true },
);

watch(
  () => user.value?.settings,
  (newSettings) => {
    if (newSettings) {
      emit("settingsChanged", newSettings);
    }
  },
  { deep: true },
);

watch(currentView, (newView) => {
  emit("viewChanged", newView);
});

// Methods
async function loadUser(id: number) {
  isLoading.value = true;
  error.value = null;
  try {
    // Simulate API call
    await new Promise((resolve) => setTimeout(resolve, 500));
    user.value = {
      id,
      name: `User ${id}`,
      email: `user${id}@example.com`,
      role: "user",
      permissions: ["view:profile", "edit:profile"],
      settings: {
        theme: "auto",
        language: "en",
        notifications: true,
        emailFrequency: "daily",
      },
    };
  } catch (e) {
    error.value = "Failed to load user";
  } finally {
    isLoading.value = false;
  }
}

async function loadActivities() {
  if (!canViewActivity.value) return;

  try {
    // Simulate API call
    await new Promise((resolve) => setTimeout(resolve, 300));
    activities.value = [
      { id: "1", type: "login", timestamp: new Date(), details: "Logged in from Chrome" },
      { id: "2", type: "update", timestamp: new Date(), details: "Updated profile" },
    ];
  } catch (e) {
    console.error("Failed to load activities", e);
  }
}

async function saveUser() {
  if (!user.value || !isDirty.value) return;

  saveStatus.value = "saving";
  try {
    // Simulate API call
    await new Promise((resolve) => setTimeout(resolve, 1000));
    saveStatus.value = "saved";
    isDirty.value = false;
    setTimeout(() => {
      saveStatus.value = "idle";
    }, 2000);
  } catch (e) {
    saveStatus.value = "error";
    error.value = "Failed to save user";
  }
}

function changeView(view: typeof currentView.value) {
  currentView.value = view;
  if (view === "activity") {
    loadActivities();
  }
}

// Lifecycle
onMounted(() => {
  console.log("Component mounted", props.userId);
});

onUnmounted(() => {
  console.log("Component unmounted");
});

// Provide
provide("user", user);
provide("canEdit", canEditProfile);
</script>

<template>
  <div class="user-profile">
    <header>
      <h1>{{ displayName }}</h1>
      <nav>
        <button :class="{ active: currentView === 'profile' }" @click="changeView('profile')">
          Profile
        </button>
        <button :class="{ active: currentView === 'settings' }" @click="changeView('settings')">
          Settings
        </button>
        <button
          v-if="canViewActivity"
          :class="{ active: currentView === 'activity' }"
          @click="changeView('activity')"
        >
          Activity
        </button>
      </nav>
    </header>

    <main v-if="!isLoading && user">
      <section v-if="currentView === 'profile'">
        <h2>Profile Information</h2>
        <div class="form-group">
          <label>Name</label>
          <input v-model="user.name" :disabled="!canEditProfile" type="text" />
        </div>
        <div class="form-group">
          <label>Email</label>
          <input v-model="user.email" :disabled="!canEditProfile" type="email" />
        </div>
        <div class="form-group">
          <label>Role</label>
          <span>{{ user.role }}</span>
        </div>
        <button
          v-if="canEditProfile && isDirty"
          :disabled="saveStatus === 'saving'"
          @click="saveUser"
        >
          {{ saveStatus === "saving" ? "Saving..." : "Save Changes" }}
        </button>
        <span v-if="saveStatus === 'saved'" class="success">Saved!</span>
      </section>

      <section v-else-if="currentView === 'settings'">
        <h2>Settings</h2>
        <div class="form-group">
          <label>Theme</label>
          <select v-model="user.settings.theme">
            <option value="light">Light</option>
            <option value="dark">Dark</option>
            <option value="auto">Auto</option>
          </select>
        </div>
        <div class="form-group">
          <label>Language</label>
          <select v-model="user.settings.language">
            <option value="en">English</option>
            <option value="es">Spanish</option>
            <option value="fr">French</option>
          </select>
        </div>
        <div class="form-group">
          <label>
            <input type="checkbox" v-model="user.settings.notifications" />
            Enable notifications
          </label>
        </div>
        <div class="form-group">
          <label>Email Frequency</label>
          <select v-model="user.settings.emailFrequency">
            <option value="realtime">Real-time</option>
            <option value="daily">Daily</option>
            <option value="weekly">Weekly</option>
            <option value="never">Never</option>
          </select>
        </div>
      </section>

      <section v-else-if="currentView === 'activity'">
        <h2>Activity Log</h2>
        <div class="stats">
          <div class="stat">
            <span class="label">Logins</span>
            <span class="value">{{ activityStats.logins }}</span>
          </div>
          <div class="stat">
            <span class="label">Updates</span>
            <span class="value">{{ activityStats.updates }}</span>
          </div>
          <div class="stat">
            <span class="label">Creates</span>
            <span class="value">{{ activityStats.creates }}</span>
          </div>
        </div>
        <ul class="activity-list">
          <li v-for="activity in activities" :key="activity.id">
            <span class="type">{{ activity.type }}</span>
            <span class="details">{{ activity.details }}</span>
            <span class="timestamp">{{ activity.timestamp.toLocaleString() }}</span>
          </li>
        </ul>
      </section>
    </main>

    <div v-else-if="isLoading" class="loading">Loading user data...</div>

    <div v-else-if="error" class="error">
      {{ error }}
    </div>
  </div>
</template>

<style scoped>
.user-profile {
  max-width: 800px;
  margin: 0 auto;
  padding: 2rem;
}

header {
  border-bottom: 1px solid #ddd;
  margin-bottom: 2rem;
}

nav {
  display: flex;
  gap: 1rem;
  margin-top: 1rem;
}

button.active {
  font-weight: bold;
  border-bottom: 2px solid blue;
}

.form-group {
  margin-bottom: 1rem;
}

.stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 1rem;
  margin-bottom: 2rem;
}

.stat {
  padding: 1rem;
  background: #f5f5f5;
  border-radius: 4px;
}

.activity-list {
  list-style: none;
  padding: 0;
}

.activity-list li {
  padding: 0.5rem;
  border-bottom: 1px solid #eee;
  display: flex;
  gap: 1rem;
}

.success {
  color: green;
  margin-left: 0.5rem;
}

.error {
  color: red;
}

.loading {
  text-align: center;
  padding: 2rem;
}
</style>
