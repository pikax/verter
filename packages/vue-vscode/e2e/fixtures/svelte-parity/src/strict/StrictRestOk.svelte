<script lang="ts">
  import StrictRestHost from "./StrictRestHost.svelte";
</script>

<!-- Rest props declared → extra attrs are intentional, not unknown noise -->
<StrictRestHost label="ok" class="rest-class" data-testid="rest" />
