<script lang="ts">
  let typeCount = $state(0);
  function bumpType() {
    typeCount += 1;
  }
</script>

<div>
  <button type="button" onclick={bumpType}>{typeCount}</button>
  <!-- type-complete-here --><
</div>
