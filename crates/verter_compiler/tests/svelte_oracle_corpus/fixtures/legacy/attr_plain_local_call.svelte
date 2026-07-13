<script>
	export let p;
	function m() { return 1; }
</script>

<button on:click={m}>x</button>
<div title={m()}></div>
