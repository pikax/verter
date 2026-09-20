<!--
  STS0 mode-paired compiler control: the Svelte 5 component ABI is a
  function under BOTH runes:true and runes:false. The same source is
  compiled with each option; both must emit `export default function`
  with zero warnings. A runes-only classification of
  component-function-shape fails this pair.
-->
<script>
  let count = 0;
</script>

<p>{count}</p>
