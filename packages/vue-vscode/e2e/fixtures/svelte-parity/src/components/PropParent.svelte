<script lang="ts">
  import PropChild from "./PropChild.svelte";
  let contractProp = "from-parent";
</script>

<PropChild {contractProp} optionalFlag={true} />
