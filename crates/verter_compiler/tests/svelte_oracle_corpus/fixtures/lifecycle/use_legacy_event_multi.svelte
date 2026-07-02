<script>
	let c = $state(0);
</script>

<div use:foo on:click={() => c++} on:keydown={() => c++}>x</div>
