<script>
	let open = $state(true);
	let { flag } = $props();
</script>

{#if open}
	<div data-x="{flag ? 'b' : '\u0062'}">x</div>
{/if}

<style>
	[data-x="b"] {
		color: red;
	}
</style>
