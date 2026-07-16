<script lang="ts">
  let name = $state("ada");
</script>

<input bind:value={name} />
<p>{name}</p>
