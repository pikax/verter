<script lang="ts">
  interface MapValue {
    label: string;
    count: number;
  }

  let mapValue: MapValue = { label: "map", count: 7 };

  function mapFn(x: number): number {
    return x + 1;
  }
</script>

<section>
  <p id="map-label">{mapValue.label}</p>
  <p id="map-count">{mapValue.count}</p>
  <button type="button" onclick={() => mapFn(mapValue.count)}>{mapFn(1)}</button>
</section>
