<svelte:options css="injected" />

<script>
	let { value } = $props();
</script>

<div class={value}>x</div>

<style>
	.a\26 b\&c {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
