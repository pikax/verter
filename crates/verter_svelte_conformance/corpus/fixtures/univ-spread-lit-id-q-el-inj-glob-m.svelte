<svelte:options css="injected" />

<script>
	let { rest } = $props();
</script>

<div {...rest}>x</div>

<style>
	:global(*) {
		color: red;
	}
</style>
