<script>
	let c = $state(0);
</script>

{#if c}<div use:foo onclick={() => c++}>x</div>{/if}
