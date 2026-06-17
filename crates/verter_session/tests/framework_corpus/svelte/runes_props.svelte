<script lang="ts">
  interface Props { title: string; count?: number; onClose?: () => void }
  let { title, count, onClose }: Props = $props();
</script>
<h1 onclick={onClose}>{title}: {count}</h1>
