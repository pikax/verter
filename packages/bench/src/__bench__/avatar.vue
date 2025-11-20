<script setup lang="ts">
import { computed, defineComponent, ref, watchEffect } from "vue";

const props = withDefaults(
  defineProps<{
    src?: string | null;
    alt?: string;
    badge?: boolean;

    size?: string;

    // just used for colour
    id?: string;
  }>(),
  {
    size: "2.5rem",
  }
);
const initialsBackground = computed(() => stringToHslColor(props.id || props.alt || "", 30, 80));

const avatarEl = ref<HTMLElement>();
// TODO handle the image don't exist and fall back to initials, otherwise it will
// show a broken image

const initials = computed(() => {
  return (props.alt || "")
    .split(/\W/)
    .filter(Boolean)
    .map((x) => x[0])
    .slice(0, avatarEl.value?.offsetHeight! < 20 ? 1 : 2)
    .join("")
    .toUpperCase();
});

const validImage = ref(!!props.src);
let ii = 0;
watchEffect(() => {
  const id = ++ii;
  const image = new Image();

  image.onerror = () => {
    if (ii === id) {
      validImage.value = false;
    }
  };

  image.onload = () => {
    if (ii === id) {
      validImage.value = true;
    }
  };

  if (props.src) {
    image.src = props.src!;
  } else {
    validImage.value = false;
  }
});

function stringToHslColor(str: string, s: string | number, l: string | number) {
  let hash = 0;
  for (var i = 0; i < str.length; i++) {
    hash = str.charCodeAt(i) + ((hash << 5) - hash);
  }

  var h = hash % 360;
  return "hsl(" + h + ", " + s + "%, " + l + "%)";
}
</script>

<template>
  <div ref="avatarEl" class="h-avatar relative inline-flex flex-shrink-0 flex-col">
    <span
      v-if="initials && !validImage"
      class="h-avatar--initials flex h-full w-full flex-shrink-0 items-center justify-center rounded-full object-cover"
      :title="alt"
      data-testid="initials-avatar"
    >
      {{ initials }}
    </span>
    <img
      v-if="src && validImage"
      class="z-10 h-full w-full rounded-full object-cover"
      :src="src"
      :alt="alt"
      :title="alt"
      data-testid="img-avatar"
    />
    <slot name="badge">
      <span
        v-if="badge"
        class="absolute top-0 right-0 z-10 h-4 w-4 rounded-full border-2 border-white bg-green-600"
        data-testid="avatar-badge"
      ></span>
    </slot>
  </div>
</template>

<style lang="scss" scoped>
.h-avatar {
  width: v-bind(size);
  height: v-bind(size);
  font-size: calc(v-bind(size) / 2);
  padding: 0;
  margin: 0;
}

.h-avatar--initials {
  background-color: v-bind(initialsBackground);
}
</style>
