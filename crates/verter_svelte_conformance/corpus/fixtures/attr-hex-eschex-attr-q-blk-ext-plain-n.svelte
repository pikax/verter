<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="a&#x26;b">x</div>
{/if}

<style>
	[data-x="\7a z"] {
		color: red;
	}
</style>
