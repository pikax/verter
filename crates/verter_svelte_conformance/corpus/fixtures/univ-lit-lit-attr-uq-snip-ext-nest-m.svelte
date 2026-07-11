{#snippet subject()}
	<div data-x=b>x</div>
{/snippet}

{@render subject()}

<style>
	* {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
