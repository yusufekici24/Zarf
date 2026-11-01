<script lang="ts">
  // Outlook gibi bölme ayırıcısı: yalnızca bağlı olduğu bölmenin piksel genişliğini
  // (ya da yüksekliğini) değiştirir; diğer bölmeler yerinde kalır, ortadaki esnek
  // bölme (ileti listesi) aradaki farkı alır.
  let {
    size = $bindable(),
    min = 120,
    max = 2000,
    direction = "horizontal",
    invert = false,
    onend,
  }: {
    size: number;
    min?: number;
    max?: number;
    direction?: "horizontal" | "vertical";
    /** Bölme ayırıcının sağında/altındaysa (okuma bölmesi) sürükleme yönü tersine çalışır */
    invert?: boolean;
    onend?: (size: number) => void;
  } = $props();

  let dragging = $state(false);

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    dragging = true;
    const start = direction === "horizontal" ? e.clientX : e.clientY;
    const startSize = size;
    let raf = 0;
    const move = (ev: PointerEvent) => {
      const pos = direction === "horizontal" ? ev.clientX : ev.clientY;
      const d = (pos - start) * (invert ? -1 : 1);
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => (size = Math.round(Math.max(min, Math.min(max, startSize + d)))));
    };
    const up = () => {
      dragging = false;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      onend?.(size);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative z-10 shrink-0 {direction === 'horizontal' ? 'w-[6px] cursor-col-resize' : 'h-[6px] cursor-row-resize'}"
  onpointerdown={down}
  ondblclick={() => onend?.(size)}
  role="separator"
  aria-orientation={direction === "horizontal" ? "vertical" : "horizontal"}
>
  <div
    class="absolute rounded-full {direction === 'horizontal' ? 'inset-y-2 left-[2px] w-[2px]' : 'inset-x-2 top-[2px] h-[2px]'} {dragging ? 'bg-primary' : 'group-hover:bg-primary/50'}"
  ></div>
</div>
{#if dragging}
  <!-- Sürüklerken iframe'ler (ileti gövdesi) imleci yutmasın -->
  <div class="fixed inset-0 z-[200] {direction === 'horizontal' ? 'cursor-col-resize' : 'cursor-row-resize'}"></div>
{/if}
