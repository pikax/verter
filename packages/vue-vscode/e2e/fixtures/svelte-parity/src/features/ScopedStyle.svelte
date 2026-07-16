<script lang="ts">
  let heading = "scoped";
</script>

<section class="card" id="card-root">
  <h2 class="card-title">{heading}</h2>
  <p class="card-body">body</p>
</section>

<style>
  .card {
    display: block;
  }
  .card-title {
    font-weight: 600;
  }
  #card-root {
    color: inherit;
  }
  :global(body) {
    margin: 0;
  }
</style>
