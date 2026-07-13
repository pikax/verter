{#snippet subject()}
	<div data-x>x</div>
{/snippet}

{@render subject()}

<style>
	[data-zz] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
