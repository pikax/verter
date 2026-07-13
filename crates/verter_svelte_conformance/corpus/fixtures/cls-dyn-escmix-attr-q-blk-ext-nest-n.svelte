<script>
	let open = $state(true);
	let { flag } = $props();
</script>

{#if open}
	<div data-x="{flag ? 'a&b&c' : 'a\u0026b\u0026c'}">x</div>
{/if}

<style>
	.\7a \z {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
