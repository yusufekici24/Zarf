<script lang="ts">
  // Şerit düğmesi. Büyük: simge üstte, ad altta (Outlook büyük düğme); `small`: simge ve ad yan yana.
  import type { Component, Snippet } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  let {
    icon: Icon,
    label,
    title = undefined,
    disabled = false,
    onclick = undefined,
    menu = false,
    accent = false,
    small = false,
    active = false,
    iconClass = "",
    children = undefined,
    ...rest
  }: {
    icon: Component<{ class?: string; strokeWidth?: number }>;
    label: string;
    title?: string;
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    menu?: boolean;
    accent?: boolean;
    small?: boolean;
    active?: boolean;
    iconClass?: string;
    children?: Snippet;
    [key: string]: unknown;
  } = $props();

  /** Outlook gibi büyük düğme adı iki satıra dengeli bölünür (boşluktan ya da tireden);
      açılır ok ikinci satırın sonunda durur */
  function twoLines(l: string): [string, string] {
    let best: [string, string] = [l, ""];
    let width = Infinity;
    for (const m of l.matchAll(/ |-(?=\S)/g)) {
      const at = m.index! + (m[0] === "-" ? 1 : 0);
      const a = l.slice(0, at).trim();
      const b = l.slice(at).trim();
      const w = Math.max(a.length, b.length);
      if (a && b && w <= width) {
        width = w;
        best = [a, b];
      }
    }
    return best;
  }
  const lines = $derived(twoLines(label));
</script>

{#if small}
  <button type="button" class="ribbon-btn-sm {active ? 'bg-accent' : ''}" {disabled} title={title ?? label} {onclick} {...rest}>
    <Icon class="size-4 shrink-0 {accent ? 'text-primary' : ''} {iconClass}" />
    <span class="rb-label whitespace-nowrap">{label}</span>{#if menu}<ChevronDown class="size-3 opacity-60" />{/if}
    {@render children?.()}
  </button>
{:else}
  <button type="button" class="ribbon-btn {active ? 'bg-accent' : ''}" {disabled} title={title ?? label} {onclick} {...rest}>
    <Icon class="size-8 {accent ? 'text-primary' : ''} {iconClass}" strokeWidth={1.5} />
    <span class="rb-label-full hidden items-center whitespace-nowrap">{label}{#if menu}<ChevronDown class="ml-0.5 size-3 opacity-70" />{/if}</span>
    <span class="rb-label flex flex-col items-center text-center leading-[14px] whitespace-nowrap"><span>{lines[0]}</span><span class="flex min-h-[14px] items-center">{lines[1]}{#if menu}<ChevronDown class="ml-px size-3 opacity-70" />{/if}</span></span>
    {@render children?.()}
  </button>
{/if}
