<script lang="ts" setup>
defineProps<{
  name: string;
}>();

defineEmits<{
  close: [{ name: string }];
}>();

defineSlots<{
  default(args: { name: string }): any;
}>();
</script>
