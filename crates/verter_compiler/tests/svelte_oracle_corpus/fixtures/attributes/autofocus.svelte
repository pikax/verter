<script>
	let c = $state(0);
	let on = $state(true);
</script>

<input autofocus />
<input onclick={() => on = !on} autofocus={on} />
<button onclick={() => c++}>{c}</button>
