<script lang="ts">
  // Dosya görünümündeki kare düğme ve yanındaki açıklama (Outlook "Bilgi" sayfası gibi).
  // `menu` verilirse düğme açılır menü gösterir.
  import type { Component, Snippet } from "svelte";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  let {
    icon: Icon,
    label,
    title,
    text = "",
    details = [],
    disabled = false,
    onclick = undefined,
    menu = undefined,
    iconClass = "text-primary",
    children = undefined,
  }: {
    icon: Component<{ class?: string; strokeWidth?: number }>;
    label: string;
    title: string;
    text?: string;
    details?: string[];
    disabled?: boolean;
    onclick?: () => void;
    menu?: Snippet;
    iconClass?: string;
    children?: Snippet;
  } = $props();

  const cls = "flex h-[86px] w-[112px] shrink-0 flex-col items-center justify-center gap-1.5 rounded-[3px] border border-[#c8c6c4] bg-background px-1 text-center text-[12px] leading-tight hover:border-[#a19f9d] hover:bg-[#f3f2f1] disabled:opacity-40 disabled:hover:bg-background dark:border-white/25 dark:hover:bg-white/10";
</script>

{#snippet face()}
  <Icon class="size-7 {iconClass}" strokeWidth={1.4} />
  <span>{label}{#if menu}<ChevronDown class="ml-0.5 inline size-3" />{/if}</span>
{/snippet}

<div class="flex max-w-[760px] gap-5 border-b border-transparent py-2.5">
  {#if menu}
    <DropdownMenu.Root>
      <DropdownMenu.Trigger {disabled}>{#snippet child({ props })}<button {...props} class={cls} {disabled}>{@render face()}</button>{/snippet}</DropdownMenu.Trigger>
      <DropdownMenu.Content align="start" class="max-h-96 min-w-60 overflow-y-auto">{@render menu()}</DropdownMenu.Content>
    </DropdownMenu.Root>
  {:else}
    <button class={cls} {disabled} {onclick}>{@render face()}</button>
  {/if}
  <div class="min-w-0 pt-1">
    <h3 class="text-[17px] font-semibold">{title}</h3>
    {#if text}<p class="text-muted-foreground mt-1 text-[12.5px] leading-snug">{text}</p>{/if}
    {#each details as d (d)}
      <p class="mt-1 flex items-start gap-2 text-[12.5px]"><span class="mt-[7px] size-[5px] shrink-0 bg-current"></span><span class="min-w-0 break-words">{d}</span></p>
    {/each}
    {@render children?.()}
  </div>
</div>
