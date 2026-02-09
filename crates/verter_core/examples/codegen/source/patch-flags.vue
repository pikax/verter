<template>
  <div>
    <!-- TEXT (1): dynamic text content -->
    <span>{{ message }}</span>
    <p>Static prefix: {{ dynamicSuffix }}</p>

    <!-- CLASS (2): dynamic class binding -->
    <div :class="dynamicClass">Dynamic class</div>
    <div :class="{ active: isActive }">Object class syntax</div>
    <div :class="[baseClass, conditionalClass]">Array class syntax</div>

    <!-- STYLE (4): dynamic style binding -->
    <div :style="dynamicStyle">Dynamic style</div>
    <div :style="{ color: textColor, fontSize: fontSize + 'px' }">Object style</div>
    <div :style="[baseStyle, overrideStyle]">Array style</div>

    <!-- PROPS (8): dynamic non-class/style props -->
    <div :id="dynamicId">Dynamic id</div>
    <div :title="dynamicTitle" :data-value="dataValue">Multiple dynamic props</div>
    <input :type="inputType" :placeholder="placeholder" />

    <!-- FULL_PROPS (16): v-bind spread or dynamic key -->
    <div v-bind="allProps">Full props spread</div>
    <div :[dynamicPropName]="dynamicPropValue">Dynamic prop name</div>

    <!-- NEED_HYDRATION (32): event listeners on elements -->
    <button @click="handleClick">Click handler</button>

    <!-- Combined flags -->
    <div :class="cls" :style="stl">CLASS + STYLE (6)</div>
    <div :class="cls" :id="id">CLASS + PROPS (10)</div>
    <span :class="cls">{{ text }}</span>
    <!-- TEXT + CLASS (3) -->

    <!-- Static content (no flags, can be hoisted) -->
    <div class="static" id="static-id">Fully static element</div>

    <!-- CACHED (-1): static content in dynamic parent -->
    <div v-if="show">
      <span>This static span should be cached</span>
    </div>
  </div>
</template>

<script setup>
import { ref } from "vue";

// TEXT flag triggers
const message = ref("Hello");
const dynamicSuffix = ref("World");

// CLASS flag triggers
const dynamicClass = ref("my-class");
const isActive = ref(true);
const baseClass = ref("base");
const conditionalClass = ref("conditional");

// STYLE flag triggers
const dynamicStyle = ref({ color: "red" });
const textColor = ref("blue");
const fontSize = ref(16);
const baseStyle = ref({ margin: "10px" });
const overrideStyle = ref({ padding: "5px" });

// PROPS flag triggers
const dynamicId = ref("my-id");
const dynamicTitle = ref("My Title");
const dataValue = ref("123");
const inputType = ref("text");
const placeholder = ref("Enter value");

// FULL_PROPS flag triggers
const allProps = ref({ id: "spread-id", class: "spread-class" });
const dynamicPropName = ref("data-custom");
const dynamicPropValue = ref("custom-value");

// Combined
const cls = ref("combined-class");
const stl = ref({ border: "1px solid" });
const id = ref("combined-id");
const text = ref("combined text");
const show = ref(true);

const handleClick = () => console.log("clicked");
</script>
