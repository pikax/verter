<script lang="ts">
  import BindableChild from "./BindableChild.svelte";
  let value = $state("bound");
</script>

<BindableChild bind:value />
<p>{value}</p>
