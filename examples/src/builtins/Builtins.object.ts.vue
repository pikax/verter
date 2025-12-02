<script lang="ts">
import { defineComponent, defineAsyncComponent, type PropType, type Component } from "vue";

const AsyncComponent = defineAsyncComponent(() =>
  new Promise<{ default: Component }>((resolve) => {
    setTimeout(() => {
      resolve({
        default: {
          template: "<div>Async content loaded!</div>",
        },
      });
    }, 1000);
  })
);

interface ListItem {
  id: number;
  text: string;
}

export default defineComponent({
  name: "Builtins",
  components: {
    AsyncComponent,
  },
  data() {
    return {
      // Teleport
      teleportTarget: "#modal-container" as string,
      showModal: false,
      showTooltip: false,
      tooltipDisabled: false,
      
      // KeepAlive
      currentTab: "tab1" as "tab1" | "tab2" | "tab3",
      cachedComponents: ["Tab1", "Tab2"] as string[],
      maxCached: 10,
      
      // Suspense
      showAsync: true,
      suspenseKey: 0,
      
      // Transition
      transitionShow: true,
      transitionName: "fade" as "fade" | "slide" | "zoom",
      transitionMode: "out-in" as "out-in" | "in-out" | "default",
      
      // TransitionGroup
      items: [
        { id: 1, text: "Item 1" },
        { id: 2, text: "Item 2" },
        { id: 3, text: "Item 3" },
      ] as ListItem[],
      nextId: 4,
    };
  },
  methods: {
    // Teleport methods
    toggleModal(): void {
      this.showModal = !this.showModal;
    },
    toggleTooltip(): void {
      this.showTooltip = !this.showTooltip;
    },
    
    // KeepAlive methods
    setTab(tab: "tab1" | "tab2" | "tab3"): void {
      this.currentTab = tab;
    },
    
    // Suspense methods
    reloadAsync(): void {
      this.suspenseKey++;
    },
    
    // Transition methods
    toggleTransition(): void {
      this.transitionShow = !this.transitionShow;
    },
    setTransitionName(name: "fade" | "slide" | "zoom"): void {
      this.transitionName = name;
    },
    
    // TransitionGroup methods
    addItem(): void {
      this.items.push({ id: this.nextId++, text: `Item ${this.nextId - 1}` });
    },
    removeItem(id: number): void {
      const index = this.items.findIndex((item) => item.id === id);
      if (index > -1) {
        this.items.splice(index, 1);
      }
    },
    shuffleItems(): void {
      this.items = [...this.items].sort(() => Math.random() - 0.5);
    },
    
    // Transition hooks
    onBeforeEnter(el: Element): void {
      console.log("before-enter", el);
    },
    onEnter(el: Element, done: () => void): void {
      console.log("enter", el);
      done();
    },
    onAfterEnter(el: Element): void {
      console.log("after-enter", el);
    },
    onBeforeLeave(el: Element): void {
      console.log("before-leave", el);
    },
    onLeave(el: Element, done: () => void): void {
      console.log("leave", el);
      done();
    },
    onAfterLeave(el: Element): void {
      console.log("after-leave", el);
    },
    
    // KeepAlive hooks
    onActivated(): void {
      console.log("Component activated");
    },
    onDeactivated(): void {
      console.log("Component deactivated");
    },
    
    // Suspense events
    onPending(): void {
      console.log("Suspense pending");
    },
    onResolve(): void {
      console.log("Suspense resolved");
    },
    onFallback(): void {
      console.log("Suspense fallback shown");
    },
  },
});
</script>

<template>
  <div class="builtins-demo">
    <!-- Teleport -->
    <section>
      <h3>Teleport</h3>
      <button @click="toggleModal">Toggle Modal</button>
      <Teleport :to="teleportTarget" :disabled="false">
        <div v-if="showModal" class="modal">
          Modal content teleported to body
          <button @click="toggleModal">Close</button>
        </div>
      </Teleport>
      
      <Teleport to="body" :disabled="tooltipDisabled">
        <div v-if="showTooltip" class="tooltip">
          Tooltip content
        </div>
      </Teleport>
      <button @click="toggleTooltip">Toggle Tooltip</button>
      <label>
        <input type="checkbox" v-model="tooltipDisabled" />
        Disable teleport
      </label>
    </section>

    <!-- KeepAlive -->
    <section>
      <h3>KeepAlive</h3>
      <div class="tabs">
        <button
          v-for="tab in (['tab1', 'tab2', 'tab3'] as const)"
          :key="tab"
          :class="{ active: currentTab === tab }"
          @click="setTab(tab)"
        >
          {{ tab }}
        </button>
      </div>
      <KeepAlive :include="cachedComponents" :max="maxCached">
        <component :is="currentTab" />
      </KeepAlive>
      <KeepAlive exclude="Tab3">
        <component :is="currentTab" />
      </KeepAlive>
    </section>

    <!-- Suspense -->
    <section>
      <h3>Suspense</h3>
      <button @click="reloadAsync">Reload</button>
      <button @click="showAsync = !showAsync">Toggle</button>
      <Suspense
        :key="suspenseKey"
        @pending="onPending"
        @resolve="onResolve"
        @fallback="onFallback"
      >
        <template #default>
          <AsyncComponent v-if="showAsync" />
        </template>
        <template #fallback>
          <div class="loading">Loading...</div>
        </template>
      </Suspense>
    </section>

    <!-- Transition -->
    <section>
      <h3>Transition</h3>
      <div class="controls">
        <button @click="toggleTransition">Toggle</button>
        <select v-model="transitionName">
          <option value="fade">Fade</option>
          <option value="slide">Slide</option>
          <option value="zoom">Zoom</option>
        </select>
        <select v-model="transitionMode">
          <option value="out-in">Out-In</option>
          <option value="in-out">In-Out</option>
          <option value="default">Default</option>
        </select>
      </div>
      <Transition
        :name="transitionName"
        :mode="transitionMode"
        :appear="true"
        :css="true"
        @before-enter="onBeforeEnter"
        @enter="onEnter"
        @after-enter="onAfterEnter"
        @before-leave="onBeforeLeave"
        @leave="onLeave"
        @after-leave="onAfterLeave"
      >
        <div v-if="transitionShow" class="box">
          Transitioning content
        </div>
      </Transition>
    </section>

    <!-- TransitionGroup -->
    <section>
      <h3>TransitionGroup</h3>
      <div class="controls">
        <button @click="addItem">Add</button>
        <button @click="shuffleItems">Shuffle</button>
      </div>
      <TransitionGroup name="list" tag="ul" class="list">
        <li
          v-for="item in items"
          :key="item.id"
          class="list-item"
        >
          {{ item.text }}
          <button @click="removeItem(item.id)">×</button>
        </li>
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

.slide-enter-active,
.slide-leave-active {
  transition: transform 0.3s ease;
}
.slide-enter-from {
  transform: translateX(-100%);
}
.slide-leave-to {
  transform: translateX(100%);
}

.zoom-enter-active,
.zoom-leave-active {
  transition: transform 0.3s ease;
}
.zoom-enter-from,
.zoom-leave-to {
  transform: scale(0);
}

.list-enter-active,
.list-leave-active {
  transition: all 0.5s ease;
}
.list-enter-from,
.list-leave-to {
  opacity: 0;
  transform: translateX(30px);
}
.list-move {
  transition: transform 0.5s ease;
}
</style>
