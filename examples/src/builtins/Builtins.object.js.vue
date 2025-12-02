<script>
import { defineComponent, defineAsyncComponent } from "vue";

const AsyncComponent = defineAsyncComponent(() =>
  new Promise((resolve) => {
    setTimeout(() => {
      resolve({
        default: {
          template: "<div>Async content loaded!</div>",
        },
      });
    }, 1000);
  })
);

export default defineComponent({
  name: "Builtins",
  components: {
    AsyncComponent,
  },
  data() {
    return {
      // Teleport
      teleportTarget: "#modal-container",
      showModal: false,
      showTooltip: false,
      tooltipDisabled: false,
      
      // KeepAlive
      currentTab: "tab1",
      cachedComponents: ["Tab1", "Tab2"],
      maxCached: 10,
      
      // Suspense
      showAsync: true,
      suspenseKey: 0,
      
      // Transition
      transitionShow: true,
      transitionName: "fade",
      transitionMode: "out-in",
      
      // TransitionGroup
      items: [
        { id: 1, text: "Item 1" },
        { id: 2, text: "Item 2" },
        { id: 3, text: "Item 3" },
      ],
      nextId: 4,
    };
  },
  methods: {
    toggleModal() {
      this.showModal = !this.showModal;
    },
    toggleTooltip() {
      this.showTooltip = !this.showTooltip;
    },
    setTab(tab) {
      this.currentTab = tab;
    },
    reloadAsync() {
      this.suspenseKey++;
    },
    toggleTransition() {
      this.transitionShow = !this.transitionShow;
    },
    setTransitionName(name) {
      this.transitionName = name;
    },
    addItem() {
      this.items.push({ id: this.nextId++, text: `Item ${this.nextId - 1}` });
    },
    removeItem(id) {
      const index = this.items.findIndex((item) => item.id === id);
      if (index > -1) {
        this.items.splice(index, 1);
      }
    },
    shuffleItems() {
      this.items = [...this.items].sort(() => Math.random() - 0.5);
    },
    onBeforeEnter(el) {
      console.log("before-enter", el);
    },
    onEnter(el, done) {
      console.log("enter", el);
      done();
    },
    onAfterEnter(el) {
      console.log("after-enter", el);
    },
    onBeforeLeave(el) {
      console.log("before-leave", el);
    },
    onLeave(el, done) {
      console.log("leave", el);
      done();
    },
    onAfterLeave(el) {
      console.log("after-leave", el);
    },
    onActivated() {
      console.log("Component activated");
    },
    onDeactivated() {
      console.log("Component deactivated");
    },
    onPending() {
      console.log("Suspense pending");
    },
    onResolve() {
      console.log("Suspense resolved");
    },
    onFallback() {
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
          v-for="tab in ['tab1', 'tab2', 'tab3']"
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
