<script lang="ts">
  // Şerit grubu: komutlar ve altta grup adı (Outlook: "Yeni", "Sil", "Yanıtla"...).
  // `launcher`: grup adının sağındaki ↘ düğmesi (Outlook iletişim kutusu başlatıcısı)
  // Şerit sığmayınca (Outlook gibi) sağdaki gruplar katlanır: klasik şeritte grubun simgesi
  // ve adıyla tek düğme olur (tıklanınca komutlar açılır kutuda); basitleştirilmiş şeritte
  // "…" listesine taşınır.
  import { getContext, type Snippet } from "svelte";
  import MoveDownRight from "@lucide/svelte/icons/move-down-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import * as Popover from "$lib/components/ui/popover";
  let { label = "", last = false, launcher, launcherTitle = "", children }: { label?: string; last?: boolean; launcher?: () => void; launcherTitle?: string; children: Snippet } = $props();

  const ribbon = getContext<{ readonly cut: number; readonly groups: number; readonly simple?: boolean; overflow?: boolean } | undefined>("ribbon");
  let el: HTMLDivElement | undefined = $state();
  let open = $state(false);
  /** Bu grup katlandı mı (sağdan `cut` grup) */
  const folded = $derived.by(() => {
    if (!ribbon || !el || !ribbon.cut) return false;
    void ribbon.groups;
    const all = Array.from(el.closest(".ribbon-panel")?.querySelectorAll(":scope [data-rb-group]") ?? []);
    const i = all.indexOf(el);
    return i >= 0 && i >= all.length - ribbon.cut;
  });
  const overflow = !!ribbon?.overflow;
  // "…" listesinde yalnızca katlanan gruplar, şeritte yalnızca katlanmayanlar görünür
  const hidden = $derived(overflow ? !folded : folded && !!ribbon?.simple);

  // Katlanınca düğmede grubun ilk simgesi gösterilir (Outlook gibi)
  let icon = $state("");
  $effect(() => {
    if (el && !folded && !overflow) icon = el.querySelector("svg")?.outerHTML ?? "";
  });

  // Açılır kutudaki komut çalışınca kutu kapanır (alt menü açan düğmeler hariç)
  function after(e: MouseEvent) {
    const b = (e.target as HTMLElement).closest("button");
    if (b && !b.hasAttribute("aria-haspopup") && !b.hasAttribute("aria-expanded")) setTimeout(() => (open = false), 0);
  }
</script>

<div bind:this={el} data-rb-group class="flex shrink-0 flex-col px-0.5 {last ? '' : 'border-r'} {hidden ? '!hidden' : ''}">
  {#if hidden}
    <!-- yer tutucu: grupların sırası iki görünümde aynı kalsın -->
  {:else if folded && !overflow}
    <Popover.Root bind:open>
      <Popover.Trigger>
        {#snippet child({ props })}
          <button {...props} class="ribbon-btn rb-folded h-full !px-1.5 {open ? 'bg-accent' : ''}" title={label}>
            <span class="rb-folded-icon flex size-8 items-center justify-center">{@html icon}</span>
            <span class="flex items-center gap-0.5 whitespace-nowrap">{label}<ChevronDown class="size-3 opacity-70" /></span>
          </button>
        {/snippet}
      </Popover.Trigger>
      <Popover.Content align="start" sideOffset={4} class="w-auto gap-0 rounded-[4px] p-1">
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="flex flex-col" onclick={after}>
          <div class="flex items-start gap-0">{@render children()}</div>
          {#if label}<div class="text-muted-foreground pt-0.5 text-center text-[11px] leading-[14px] whitespace-nowrap">{label}</div>{/if}
        </div>
      </Popover.Content>
    </Popover.Root>
  {:else}
    <div class="rb-group-body flex flex-1 items-start gap-0">{@render children()}</div>
    {#if label}
      <div class="rb-group-label text-muted-foreground relative pt-0.5 pb-0.5 text-center text-[11px] leading-[14px] whitespace-nowrap">
        {label}
        {#if launcher}<button class="hover:bg-accent absolute right-0 bottom-0 rounded p-px" title={launcherTitle} aria-label={launcherTitle} onclick={launcher}><MoveDownRight class="size-3" /></button>{/if}
      </div>
    {/if}
  {/if}
</div>
