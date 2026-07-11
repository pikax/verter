<svelte:options css="injected" />

<script>
	let { rest } = $props();
</script>

<div {...rest}>x</div>

<style>
	:global(div) {
		color: red;
	}
</style>
