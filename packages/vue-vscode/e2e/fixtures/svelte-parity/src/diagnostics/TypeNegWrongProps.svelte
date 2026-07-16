<script lang="ts">
  import TypeNegChild from "./TypeNegChild.svelte";

  const wrongCount: string = "not-a-number";
  const wrongTitle: number = 99;
  const wrongEnabled: string = "yes";
</script>

<!-- Live type errors: wrong prop types -->
<TypeNegChild title="ok" count={wrongCount} />
<TypeNegChild title={wrongTitle} count={1} />
<TypeNegChild title="ok" count={1} enabled={wrongEnabled} />
