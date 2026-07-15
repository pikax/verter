<script>
	let r = $state.raw({ a: 1 });
	let p = $state({ b: 2 });
</script>

<button onclick={() => { r = { a: 9 }; p = { b: 9 }; }}>{r} {p}</button>
