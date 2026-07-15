{#snippet subject()}
	<div data-x>x</div>
{/snippet}

{@render subject()}

<style>
	:global(&) {
		color: red;
	}
</style>
