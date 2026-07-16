<script lang="ts">
  const label = "x";
  const num = 1;
  const notBool: string = "not-boolean";
</script>

<!-- Live type errors on native attrs / bindings -->
<input disabled={notBool} />
<div class={num}>{label}</div>
