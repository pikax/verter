{#snippet subject()}
	<div data-x>x</div>
{/snippet}

{@render subject()}

<style>
	:global(div) {
		color: red;
	}
</style>
