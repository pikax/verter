<svelte:options css="injected" />

<ul class="list"><li class="item">x</li></ul>

<style>
	.list :is(.item, .gone) {
		color: blue;
	}

	.list li:not(.off) {
		padding: 0;
	}
</style>
