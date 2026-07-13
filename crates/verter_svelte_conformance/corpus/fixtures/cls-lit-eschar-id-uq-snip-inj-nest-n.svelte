<svelte:options css="injected" />

{#snippet subject()}
	<div id=a&b>x</div>
{/snippet}

{@render subject()}

<style>
	.\zz {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
