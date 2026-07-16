<script lang="ts">
  let {
    count,
    title,
  }: {
    count: number;
    title: string;
  } = $props();
</script>

<div>{title}:{count}</div>
