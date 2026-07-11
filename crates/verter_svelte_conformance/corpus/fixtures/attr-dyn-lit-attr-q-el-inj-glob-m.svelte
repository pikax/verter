<svelte:options css="injected" />

<script>
	let { flag } = $props();
</script>

<div data-x="{flag ? 'b' : '\u0062'}">x</div>

<style>
	:global([data-x="b"]) {
		color: red;
	}
</style>
