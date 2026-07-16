<script lang="ts">
  import BadPropChild from "./BadPropChild.svelte";

  // Wrong type on purpose: string where number is required.
  const wrongCount: string = "not-a-number";
</script>

<BadPropChild title="x" count={wrongCount} />
