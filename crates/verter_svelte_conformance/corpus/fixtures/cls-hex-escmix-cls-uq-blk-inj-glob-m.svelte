<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div class=a&#x26;b&#x26;c>x</div>
{/if}

<style>
	:global(.a\26 b\&c) {
		color: red;
	}
</style>
