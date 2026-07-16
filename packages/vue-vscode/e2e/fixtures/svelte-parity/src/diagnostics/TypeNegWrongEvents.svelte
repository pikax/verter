<script lang="ts">
  import TypeNegChild from "./TypeNegChild.svelte";

  function wrongPick(n: number): void {
    void n;
  }

  function wrongChange(s: string): void {
    void s;
  }

  function wrongNativeClick(s: string): void {
    void s;
  }
</script>

<!-- Live type errors: wrong callback / event handler shapes -->
<TypeNegChild title="ok" count={1} onPick={wrongPick} onChange={wrongChange} />
<button type="button" onclick={wrongNativeClick}>native</button>
