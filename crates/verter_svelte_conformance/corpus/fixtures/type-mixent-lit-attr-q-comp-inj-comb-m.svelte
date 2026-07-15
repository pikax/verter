<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div data-x="&#98;&#x62;">x</div>
</div>
</Child>

<style>
	.wrap div {
		color: red;
	}
</style>
