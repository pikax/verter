{#snippet subject()}
	<div data-x=a&amp;b&amp;c>x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="\7a \z"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
