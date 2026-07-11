<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div class=a&#38;b>x</div>
</div>
{/if}

<style>
	.wrap .a\26 b {
		color: red;
	}
</style>
