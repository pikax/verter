<script>
	let c = $state(0);
</script>

<div use:foo={c} onclick={() => c++}></div>
