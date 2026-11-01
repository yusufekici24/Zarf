<script lang="ts">
  // "Dosya" görünümü (backstage): solda geri düğmesi ve sayfalar, sağda seçili sayfa.
  // Sayfası olmayan öğeler (Farklı Kaydet, Yazdır, Kapat...) doğrudan eylem çalıştırır.
  import type { Snippet } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import { t } from "$lib/i18n.svelte";

  export type BackstageItem = { key: string; label: string; action?: () => void; disabled?: boolean };

  let {
    items,
    bottom = [],
    page = $bindable(items[0]?.key ?? ""),
    onclose,
    children,
  }: { items: BackstageItem[]; bottom?: BackstageItem[]; page?: string; onclose: () => void; children: Snippet<[string]> } = $props();

  function pick(it: BackstageItem) {
    if (it.disabled) return;
    if (it.action) {
      onclose();
      it.action();
    } else page = it.key;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !document.querySelector("[data-state=open][role=menu]")) {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#snippet navItem(it: BackstageItem)}
  <button
    class="block w-full px-6 py-2 text-left text-[13px] leading-tight {page === it.key && !it.action
      ? 'bg-background mx-0 border-y border-r border-l-[3px] border-[#1b1b1b] border-l-primary font-semibold text-primary dark:border-white/70'
      : 'border border-transparent hover:bg-black/5 dark:hover:bg-white/10'} {it.disabled ? 'pointer-events-none opacity-40' : ''}"
    onclick={() => pick(it)}>{it.label}</button>
{/snippet}

<div class="bg-background absolute inset-0 z-40 flex min-h-0">
  <nav class="flex w-[180px] shrink-0 flex-col bg-[#f0f0f0] py-2 dark:bg-[#262626]">
    <button class="hover:bg-black/5 dark:hover:bg-white/10 mx-3 mb-4 mt-1 grid size-9 place-items-center rounded-full" title={t("common.back")} onclick={onclose}>
      <ArrowLeft class="size-[22px] rounded-full border-[1.5px] border-current p-0.5" />
    </button>
    {#each items as it (it.key)}{@render navItem(it)}{/each}
    <div class="flex-1"></div>
    {#if bottom.length}
      <div class="mx-4 mb-2 border-t border-black/15 dark:border-white/15"></div>
      {#each bottom as it (it.key)}{@render navItem(it)}{/each}
    {/if}
  </nav>
  <section class="min-w-0 flex-1 overflow-y-auto px-10 py-6">
    {@render children(page)}
  </section>
</div>
