<script lang="ts">
  const trulyUnused = 1;
  const templateOnly = 2;
  const styleDirectiveOnly = "rebeccapurple";
</script>

<div class="binding-contract" data-template-only={templateOnly} style:color={styleDirectiveOnly}></div>

<style>
  .binding-contract {
    display: block;
  }
</style>
