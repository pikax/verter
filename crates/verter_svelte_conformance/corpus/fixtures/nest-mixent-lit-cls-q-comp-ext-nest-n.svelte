<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a &#98;&#x62;">x</div>
</Child>

<style>
	.zz {
		& {
			color: red;
		}
	}
</style>
