<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div data-x="{flag ? 'a&b' : 'a\u0026b'}">x</div>
{/snippet}

{@render subject()}

<style>
	#\7a z {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
