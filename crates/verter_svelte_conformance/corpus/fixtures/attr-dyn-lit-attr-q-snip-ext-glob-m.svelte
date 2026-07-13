<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div data-x="{flag ? 'b' : '\u0062'}">x</div>
{/snippet}

{@render subject()}

<style>
	:global([data-x="b"]) {
		color: red;
	}
</style>
