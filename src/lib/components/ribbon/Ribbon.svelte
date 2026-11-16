<script lang="ts">
  // Outlook şeridi: üstte sekmeler (Dosya menüsü + modül sekmeleri), altında beyaz,
  // köşeleri yuvarlatılmış ve gruplara ayrılmış komut alanı. Sağdaki ok "Şerit görüntüleme
  // seçenekleri"ni açar: Klasik / Basitleştirilmiş şerit, tam ekran, yalnızca sekmeler.
  import { onMount, setContext, tick, type Snippet } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import * as Popover from "$lib/components/ui/popover";
  import RibbonOverflow from "./RibbonOverflow.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";

  let {
    tabs,
    active = $bindable(tabs[0]?.key ?? ""),
    file,
    content,
    right,
  }: {
    tabs: { key: string; label: string }[];
    active?: string;
    /** "Dosya" sekmesi (menü) */
    file?: Snippet;
    content: Snippet<[string]>;
    right?: Snippet;
  } = $props();

  const collapsed = $derived(app.setting("ribbon_collapsed", "0") === "1");
  const simple = $derived(app.setting("ribbon_layout", "classic") === "simplified");
  const qatHidden = $derived(app.setting("qat_hidden", "0") === "1");
  /** Tam ekran modu (yalnızca bu pencere; şerit "…" ile açılır) */
  let fullscreen = $state(false);
  let peek = $state(false);
  const hiddenRibbon = $derived(fullscreen || collapsed);
  const showPanel = $derived(!hiddenRibbon || peek);
  const showTabs = $derived(!fullscreen || peek);

  // Sığdırma (Outlook şeridi gibi kademeli). Klasik: önce "compact" (küçük düğmelerin adları
  // gizlenir), sonra sağdaki gruplar birer birer simgeli tek düğmeye katlanır; yine sığmazsa
  // şeridin yanlarında kaydırma okları çıkar. Basitleştirilmiş: tek satır, sığmayan gruplar
  // "…" listesine taşınır.
  let panel: HTMLDivElement | undefined = $state();
  let wrap: HTMLDivElement | undefined = $state();
  let density = $state<"full" | "compact" | "simple">("full");
  let cut = $state(0);
  let groups = $state(0);
  let canLeft = $state(false);
  let canRight = $state(false);
  let moreOpen = $state(false);
  setContext("ribbon", {
    get cut() {
      return cut;
    },
    get groups() {
      return groups;
    },
    get simple() {
      return simple;
    },
  });
  let fitting = false;
  const fits = () => !panel || panel.scrollWidth <= panel.clientWidth + 1;
  function scrollState() {
    if (!panel) return;
    canLeft = panel.scrollLeft > 1;
    canRight = panel.scrollLeft + panel.clientWidth < panel.scrollWidth - 1;
  }
  async function fit() {
    if (!panel || fitting) return;
    fitting = true;
    try {
      cut = 0;
      for (const d of simple ? (["simple"] as const) : (["full", "compact"] as const)) {
        density = d;
        await tick();
        if (fits()) return;
      }
      groups = panel?.querySelectorAll("[data-rb-group]").length ?? 0;
      for (let k = 1; k <= groups; k++) {
        cut = k;
        await tick();
        if (fits()) return;
      }
    } finally {
      fitting = false;
      if (panel) panel.scrollLeft = 0;
      scrollState();
    }
  }
  $effect(() => {
    void active;
    void showPanel;
    void simple;
    if (!panel) return;
    let w = panel.clientWidth;
    const ro = new ResizeObserver(() => {
      if (panel && Math.abs(panel.clientWidth - w) > 2) {
        w = panel.clientWidth;
        void fit();
      }
    });
    ro.observe(panel);
    requestAnimationFrame(() => void fit());
    return () => ro.disconnect();
  });
  function scrollBy(d: number) {
    panel?.scrollBy({ left: d * Math.max(120, (panel?.clientWidth ?? 300) * 0.6), behavior: "smooth" });
  }

  // Açılan şerit (yalnızca sekmeler / tam ekran) dışarı tıklanınca kapanır
  onMount(() => {
    const down = (e: MouseEvent) => {
      if (!peek) return;
      const tg = e.target as Element;
      if (wrap?.contains(tg) || tg.closest?.("[data-slot$='-content'], [data-select-popup]")) return;
      peek = false;
    };
    document.addEventListener("mousedown", down, true);
    return () => {
      document.removeEventListener("mousedown", down, true);
      if (fullscreen) void getCurrentWindow().setFullscreen(false).catch(() => {});
    };
  });

  function pick(k: string) {
    if (hiddenRibbon && active === k) peek = !peek;
    else peek = hiddenRibbon;
    active = k;
  }
  async function setShow(mode: "full" | "tabs" | "always") {
    peek = false;
    if (mode === "full" || fullscreen) await getCurrentWindow().setFullscreen(mode === "full").catch(() => {});
    fullscreen = mode === "full";
    if (mode !== "full") await app.setSetting("ribbon_collapsed", mode === "tabs" ? "1" : "0");
  }
</script>

{#snippet options()}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <button {...props} class="text-muted-foreground hover:bg-accent shrink-0 rounded-[4px] p-1 {simple ? 'self-center' : 'self-end'}" title={t("ribbon.display_options")} aria-label={t("ribbon.display_options")}><ChevronDown class="size-4" /></button>
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="end" class="w-64">
      <DropdownMenu.Label class="text-[13px] font-semibold">{t("ribbon.layout")}</DropdownMenu.Label>
      <DropdownMenu.CheckboxItem checked={!simple} onCheckedChange={() => void app.setSetting("ribbon_layout", "classic")}>{t("ribbon.classic")}</DropdownMenu.CheckboxItem>
      <DropdownMenu.CheckboxItem checked={simple} onCheckedChange={() => void app.setSetting("ribbon_layout", "simplified")}>{t("ribbon.simplified")}</DropdownMenu.CheckboxItem>
      <DropdownMenu.Separator />
      <DropdownMenu.Label class="text-[13px] font-semibold">{t("ribbon.show")}</DropdownMenu.Label>
      <DropdownMenu.CheckboxItem checked={fullscreen} onCheckedChange={() => void setShow("full")}>{t("ribbon.fullscreen")}</DropdownMenu.CheckboxItem>
      <DropdownMenu.CheckboxItem checked={!fullscreen && collapsed} onCheckedChange={() => void setShow("tabs")}>{t("ribbon.tabs_only")}</DropdownMenu.CheckboxItem>
      <DropdownMenu.CheckboxItem checked={!fullscreen && !collapsed} onCheckedChange={() => void setShow("always")}>{t("ribbon.always")}</DropdownMenu.CheckboxItem>
      <DropdownMenu.Separator />
      <DropdownMenu.Item onclick={() => void app.setSetting("qat_hidden", qatHidden ? "0" : "1")}>{qatHidden ? t("ribbon.qat_show") : t("ribbon.qat_hide")}</DropdownMenu.Item>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/snippet}

<div bind:this={wrap} class="relative shrink-0 select-none">
  {#if fullscreen && !peek}
    <!-- Tam ekran: şerit gizli; "…" ile sekmeler ve şerit açılır -->
    <div class="flex h-6 items-center justify-end px-2">
      <button class="hover:bg-accent text-muted-foreground rounded-[4px] px-2" title={t("ribbon.show")} aria-label={t("ribbon.show")} onclick={() => (peek = true)}><Ellipsis class="size-4" /></button>
    </div>
  {/if}
  {#if showTabs}
    <div class="flex h-8 min-w-0 items-end gap-1 overflow-hidden px-2 text-[13px]">
      {@render file?.()}
      {#each tabs as tab (tab.key)}
        <button
          data-ribbon-tab={tab.key}
          class="relative shrink-0 rounded-t px-2.5 pb-1.5 pt-1 whitespace-nowrap hover:bg-black/5 dark:hover:bg-white/10 {active === tab.key && showPanel ? 'text-primary font-semibold' : ''}"
          onclick={() => pick(tab.key)}
        >
          {tab.label}
          {#if active === tab.key && showPanel}<span class="bg-primary absolute inset-x-2.5 bottom-0 h-[3px] rounded-full"></span>{/if}
        </button>
      {/each}
      <span class="flex-1"></span>
      {@render right?.()}
    </div>
  {/if}
  {#if showPanel}
    <div
      class="bg-background mx-0.5 mb-1.5 flex items-stretch rounded-[4px] border shadow-sm {hiddenRibbon ? 'absolute inset-x-0 z-40 shadow-lg ' + (fullscreen ? 'top-8' : 'top-8') : ''}"
    >
      <div class="relative min-w-0 flex-1">
        <div
          bind:this={panel}
          data-density={density}
          class="ribbon-panel flex h-full items-stretch overflow-x-auto px-1 {simple ? 'min-h-0 py-[3px]' : 'min-h-[88px] pt-1'}"
          role="toolbar"
          tabindex="-1"
          onscroll={scrollState}
          onclick={() => hiddenRibbon && (peek = false)}
          onkeydown={(e) => e.key === "Escape" && (peek = false)}
        >
          {@render content(active)}
          {#if simple && cut > 0}
            <Popover.Root bind:open={moreOpen}>
              <Popover.Trigger>
                {#snippet child({ props })}
                  <button {...props} class="ribbon-btn-sm shrink-0 self-center {moreOpen ? 'bg-accent' : ''}" title={t("ribbon.more")} aria-label={t("ribbon.more")}><Ellipsis class="size-4" /></button>
                {/snippet}
              </Popover.Trigger>
              <Popover.Content align="end" sideOffset={4} class="max-h-[70vh] w-auto min-w-56 gap-0 overflow-y-auto rounded-[4px] p-1">
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <div
                  onclick={(e) => {
                    const b = (e.target as HTMLElement).closest("button");
                    if (b && !b.hasAttribute("aria-haspopup") && !b.hasAttribute("aria-expanded")) setTimeout(() => (moreOpen = false), 0);
                  }}
                >
                  <RibbonOverflow {cut}>{@render content(active)}</RibbonOverflow>
                </div>
              </Popover.Content>
            </Popover.Root>
          {/if}
        </div>
        {#if canLeft}
          <button class="bg-background hover:bg-accent absolute inset-y-0 left-0 z-10 flex w-4 items-center justify-center border-r shadow-sm" title={t("ribbon.scroll_left")} aria-label={t("ribbon.scroll_left")} onclick={() => scrollBy(-1)}><ChevronLeft class="size-3.5" /></button>
        {/if}
        {#if canRight}
          <button class="bg-background hover:bg-accent absolute inset-y-0 right-0 z-10 flex w-4 items-center justify-center border-l shadow-sm" title={t("ribbon.scroll_right")} aria-label={t("ribbon.scroll_right")} onclick={() => scrollBy(1)}><ChevronRight class="size-3.5" /></button>
        {/if}
      </div>
      <div class="flex shrink-0 pr-0.5 pb-0.5 {simple ? 'items-center' : 'items-end'}">{@render options()}</div>
    </div>
  {/if}
</div>
