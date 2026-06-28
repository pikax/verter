<script>
	let t = $state(0); let p = $state(false); let d = $state(0); let played = $state();
</script>
<video bind:currentTime={t} bind:paused={p} bind:duration={d} bind:played={played}></video>
