<!--
  STS0 legacy-only control: `export let` is a genuinely legacy-only
  construct. The pinned compiler accepts it with runes:false and rejects it
  in runes mode (legacy_export_invalid), which is what keeps this row
  semantics "legacy" while the shared features above stay "both".
-->
<script>
  export let name = "hello";
</script>

<p>{name}</p>
