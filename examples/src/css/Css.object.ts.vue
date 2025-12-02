<script lang="ts">
import { defineComponent } from "vue";

interface Theme {
  textColor: string;
  bgColor: string;
  borderColor: string;
  shadowColor: string;
}

export default defineComponent({
  name: "Css",
  data() {
    return {
      primaryColor: "#3498db",
      secondaryColor: "#2ecc71",
      fontSize: 16,
      borderRadius: 8,
      padding: 10,
      theme: {
        textColor: "#333333",
        bgColor: "#ffffff",
        borderColor: "#cccccc",
        shadowColor: "rgba(0, 0, 0, 0.1)",
      } as Theme,
      isActive: false,
      size: "medium" as "small" | "medium" | "large",
    };
  },
  computed: {
    dynamicWidth(): string {
      return `${100 + this.padding * 2}px`;
    },
    dynamicBg(): string {
      return `linear-gradient(135deg, ${this.primaryColor}, ${this.secondaryColor})`;
    },
  },
  methods: {
    toggleActive(): void {
      this.isActive = !this.isActive;
    },
    setSize(newSize: "small" | "medium" | "large"): void {
      this.size = newSize;
    },
    updatePrimaryColor(color: string): void {
      this.primaryColor = color;
    },
    updateFontSize(delta: number): void {
      this.fontSize = Math.max(12, Math.min(24, this.fontSize + delta));
    },
    updateBorderRadius(value: number): void {
      this.borderRadius = value;
    },
    toggleTheme(): void {
      if (this.theme.bgColor === "#ffffff") {
        this.theme = {
          textColor: "#ffffff",
          bgColor: "#1a1a2e",
          borderColor: "#16213e",
          shadowColor: "rgba(0, 0, 0, 0.3)",
        };
      } else {
        this.theme = {
          textColor: "#333333",
          bgColor: "#ffffff",
          borderColor: "#cccccc",
          shadowColor: "rgba(0, 0, 0, 0.1)",
        };
      }
    },
  },
});
</script>

<template>
  <div class="css-demo">
    <section>
      <h3>Dynamic CSS with v-bind</h3>
      <div class="dynamic-box">
        Dynamic styled box
      </div>
      <div class="controls">
        <label>
          Primary Color:
          <input type="color" :value="primaryColor" @input="updatePrimaryColor(($event.target as HTMLInputElement).value)" />
        </label>
        <label>
          Font Size: {{ fontSize }}px
          <button @click="updateFontSize(-2)">-</button>
          <button @click="updateFontSize(2)">+</button>
        </label>
        <label>
          Border Radius: {{ borderRadius }}px
          <input type="range" min="0" max="20" :value="borderRadius" @input="updateBorderRadius(Number(($event.target as HTMLInputElement).value))" />
        </label>
      </div>
    </section>

    <section>
      <h3>Computed CSS Values</h3>
      <div class="gradient-box">
        Gradient background
      </div>
    </section>

    <section>
      <h3>Theme Object</h3>
      <div class="themed-box">
        <p>Themed content</p>
        <button @click="toggleTheme">Toggle Theme</button>
      </div>
    </section>

    <section>
      <h3>Scoped Style Selectors</h3>
      <div class="scoped-demo">
        <p class="local">Local scoped style</p>
        <div class="child-component">
          <p class="deep-target">Deep selector target</p>
        </div>
        <slot></slot>
      </div>
    </section>

    <section>
      <h3>CSS Modules</h3>
      <div
        :class="[
          $style.container,
          isActive ? $style.active : '',
          $style[size],
        ]"
      >
        CSS Module styled content
      </div>
      <div class="controls">
        <button @click="toggleActive">Toggle Active</button>
        <button @click="setSize('small')">Small</button>
        <button @click="setSize('medium')">Medium</button>
        <button @click="setSize('large')">Large</button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.css-demo {
  padding: 20px;
}

section {
  margin-bottom: 30px;
}

.dynamic-box {
  background-color: v-bind(primaryColor);
  font-size: v-bind(fontSize + 'px');
  border-radius: v-bind(borderRadius + 'px');
  padding: v-bind(padding + 'px');
  color: white;
  transition: all 0.3s ease;
}

.gradient-box {
  background: v-bind(dynamicBg);
  width: v-bind(dynamicWidth);
  padding: 20px;
  color: white;
  border-radius: 8px;
}

.themed-box {
  background-color: v-bind('theme.bgColor');
  color: v-bind('theme.textColor');
  border: 1px solid v-bind('theme.borderColor');
  box-shadow: 0 2px 8px v-bind('theme.shadowColor');
  padding: 20px;
  border-radius: 8px;
  transition: all 0.3s ease;
}

.scoped-demo :deep(.deep-target) {
  color: purple;
  font-weight: bold;
}

.scoped-demo :slotted(p) {
  color: green;
  font-style: italic;
}

:global(.global-class) {
  font-family: monospace;
}

.local {
  color: blue;
}

.controls {
  display: flex;
  gap: 10px;
  margin-top: 10px;
  flex-wrap: wrap;
}

.controls label {
  display: flex;
  align-items: center;
  gap: 5px;
}
</style>

<style module>
.container {
  padding: 15px;
  border: 2px solid #ccc;
  border-radius: 8px;
  transition: all 0.3s ease;
}

.active {
  border-color: #3498db;
  background-color: #ecf0f1;
}

.small {
  font-size: 12px;
  padding: 8px;
}

.medium {
  font-size: 16px;
  padding: 15px;
}

.large {
  font-size: 20px;
  padding: 24px;
}
</style>
