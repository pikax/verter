<script setup lang="ts">
import { watch, ref, onMounted, onUnmounted } from "vue";
// import Iconify from "@purge-icons/generated";
import { iconToSVG } from "@iconify/utils";
import { iconToHTML } from "iconify-icon";

const props = defineProps<{
  icon: string;
  size?: string;
}>();

const el = ref<HTMLElement | null>(null);
const update = async () => {
  if (el.value) {
    // const svg = Iconify.renderSVG(props.icon, {});
    
    const { body, attributes } = iconToSVG(props.icon, {});
          const svgHtml = iconToHTML(body, attributes);

          const svg = document.createElement("svg");
    if (svg) {
      el.value.textContent = "";
      if (props.size) {
        svg.setAttribute("width", props.size);
        svg.setAttribute("height", props.size);
      }
      el.value.appendChild(svg);
    } else {
      const span = document.createElement("span");
      span.className = "iconify";
      span.dataset.icon = props.icon;
      el.value.textContent = "";
      el.value.appendChild(span);
    }
  }
};

watch(() => props.icon, update, { flush: "post" });
onMounted(update);

onUnmounted(update);
</script>

<template>
  <div ref="el" class="inline-flex" />
</template>

<style>
span.iconify {
  background: #5551;
  border-radius: 100%;
  display: block;

  position: relative;
  width: 100%;
  height: 100%;
}
</style>
