<script lang="ts">
  import DirectChild from "./components/DirectChild.svelte";

  interface ProbeShape {
    probeLabel: string;
    probeCount: number;
  }

  const probeValue: ProbeShape = { probeLabel: "probe", probeCount: 1 };
  let probeBound = $state("bound");
  const probeRows = [probeValue];
  // The plain-script control: member completion here must be unaffected by anything a
  // markup-region completion source does.
  const probeMember = probeValue.probeLabel;
  void probeMember;
</script>

<!--
  Each probe element carries a unique `data-probe` marker and exactly ONE space before
  its self-closing slash. A completion probe types the trigger character into that
  space, so the request runs against real post-typing text.
-->
<section>
  <input data-probe="intrinsic-bind" bind:value={probeBound} />
  <button data-probe="intrinsic-event" onclick={() => probeValue.probeCount}>go</button>
  <article data-probe="intrinsic-class" class="probe">text</article>
  <DirectChild data-probe="component-prop" contractProp={probeValue.probeLabel} />
  {#each probeRows as probeRow}
    <span>{probeRow.probeLabel}</span>
  {/each}
  <p>{probeValue.probeLabel}</p>
</section>
