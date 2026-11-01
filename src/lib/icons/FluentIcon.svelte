<script lang="ts">
  // Fluent simgesi: Lucide bileşeniyle aynı özellikleri alır (class, size, color);
  // çizgi kalınlığı Fluent'te şeklin parçası olduğundan strokeWidth yok sayılır.
  let {
    svg,
    class: cls = "",
    size = 24,
    color = "currentColor",
    style = "",
  }: { svg: string; class?: string; size?: number | string; color?: string; style?: string; strokeWidth?: number | string; absoluteStrokeWidth?: boolean } = $props();
  const html = $derived(
    svg
      .replace(/<svg([^>]*?)\swidth="\d+"\sheight="\d+"/, "<svg$1")
      .replace("<svg ", `<svg width="${size}" height="${size}" fill="${color}" aria-hidden="true" class="lucide ${cls}" style="flex-shrink:0;${style}" `),
  );
</script>

{@html html}
