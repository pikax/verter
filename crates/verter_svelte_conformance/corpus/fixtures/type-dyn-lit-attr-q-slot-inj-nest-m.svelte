<svelte:options css="injected" />

<script>
	let { flag } = $props();
</script>

<slot>
	<div data-x="{flag ? 'b' : '\u0062'}">x</div>
</slot>

<style>
	div {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
