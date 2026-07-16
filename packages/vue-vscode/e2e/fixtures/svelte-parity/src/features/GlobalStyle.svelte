<script lang="ts">
  let text = "global";
</script>

<p class="local-only">{text}</p>
<p class="leaked-global">x</p>

<style>
  .local-only {
    color: navy;
  }
  :global(.leaked-global) {
    color: crimson;
  }
</style>
