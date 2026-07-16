<script lang="ts">
  let count = $state(0);
  let doubled = $derived(count * 2);

  function bump() {
    count += 1;
  }
</script>

<section>
  <button onclick={bump}>{count} → {doubled}</button>
</section>
