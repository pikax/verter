<script>
  // @ts-check
  import JsPropChild from "./JsPropChild.svelte";
  /** @type {string} */
  let wrongCount = "not-a-number";
</script>

<!-- Live type error: count expects number -->
<JsPropChild title="ok" count={wrongCount} />
