<script setup lang="ts">
defineProps<{
  item: string;
}>();

const emits = defineEmits<{
  close: [arg: { item: string }];
}>();

// emits("close", {item:});
</script>
