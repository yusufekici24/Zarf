<script lang="ts">
  // Outlook kişi kartı: ada tıklayınca açılır. Fotoğraf, unvan, bölüm, e-posta, telefonlar;
  // grupta üyeler (ilk birkaçı fotoğrafla, "N üye daha"; tıklayınca tam liste).
  import Avatar from "./Avatar.svelte";
  import { people } from "$lib/stores/people.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import { displayName, splitNameAddr } from "$lib/format";
  import X from "@lucide/svelte/icons/x";
  import Mail from "@lucide/svelte/icons/mail";
  import Phone from "@lucide/svelte/icons/phone";
  import Copy from "@lucide/svelte/icons/copy";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Building2 from "@lucide/svelte/icons/building-2";
  import Loader from "@lucide/svelte/icons/loader-circle";
  import { toast } from "svelte-sonner";

  type Card = Awaited<ReturnType<typeof api.contactCard>>;
  let data = $state<Card | null>(null);
  let loading = $state(false);
  let showMembers = $state(false);
  let el: HTMLDivElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });

  $effect(() => {
    const c = people.card;
    data = null;
    showMembers = false;
    if (!c) return;
    loading = true;
    const email = c.email;
    api
      .contactCard(email)
      .then((d) => {
        if (people.card?.email === email) data = d;
      })
      .catch(() => {})
      .finally(() => (loading = false));
  });

  // Pencereye sığacak şekilde konumlandır
  $effect(() => {
    const c = people.card;
    if (!c || !el) return;
    void data;
    void showMembers;
    requestAnimationFrame(() => {
      if (!el) return;
      const w = el.offsetWidth;
      const h = el.offsetHeight;
      pos = {
        left: Math.max(8, Math.min(c.x, window.innerWidth - w - 8)),
        top: c.y + h > window.innerHeight - 8 ? Math.max(8, window.innerHeight - h - 8) : c.y,
      };
    });
  });

  const isGroup = $derived(!!data && (data.kind.endsWith("DL") || data.kind === "GroupMailbox" || data.members.length > 0));
  const name = $derived(data?.name || people.card?.name || people.card?.email || "");
  const PHONE_KEYS: Record<string, string> = { BusinessPhone: "card.phone_work", MobilePhone: "card.phone_mobile", HomePhone: "card.phone_home" };

  function close() {
    people.card = null;
  }

  function mailTo() {
    const c = people.card;
    if (!c) return;
    const n = name && name !== c.email ? `${name.replace(/[",;]/g, " ").trim()} <${c.email}>` : c.email;
    close();
    app.compose({ mode: "new", data: { to: n } });
  }

  async function copy(v: string) {
    try {
      await navigator.clipboard.writeText(v);
      toast.success(t("card.copied"));
    } catch {
      /* yok */
    }
  }

  function onwindowdown(e: PointerEvent) {
    if (people.card && el && !el.contains(e.target as Node) && !(e.target as HTMLElement)?.closest?.("[data-card-trigger]")) close();
  }
</script>

<!-- Pencere odağı kaybedince (ör. ileti çerçevesine tıklanınca) de kapanır -->
<svelte:window onblur={() => { if (people.card && document.activeElement?.tagName === "IFRAME") close(); }} onpointerdown={onwindowdown} onkeydown={(e) => { if (e.key === "Escape" && people.card) { e.stopPropagation(); close(); } }} />

{#if people.card}
  {@const c = people.card}
  <div
    bind:this={el}
    data-contact-card
    role="dialog"
    aria-label={name}
    class="bg-popover text-popover-foreground fixed z-[90] flex max-h-[80vh] w-[360px] flex-col overflow-hidden rounded-md border text-[13px] shadow-xl"
    style="left:{pos.left || c.x}px;top:{pos.top || c.y}px"
  >
    <button class="hover:bg-accent absolute top-1.5 right-1.5 rounded p-1" title={t("common.close")} onclick={close}><X class="size-4" /></button>
    <div class="flex gap-4 px-5 pt-5 pb-3">
      <Avatar name={name} email={c.email} class="size-[72px] text-2xl" />
      <div class="selectable min-w-0 flex-1 pt-1">
        <div class="text-[18px] leading-tight break-words">{name}</div>
        {#if data?.job_title}<div class="text-muted-foreground mt-1">{data.job_title}</div>{/if}
        {#if data?.department}<div class="text-muted-foreground">{data.department}</div>{/if}
        <div class="mt-3 flex items-center gap-1">
          <button class="hover:bg-accent rounded p-1.5" title={t("card.send_mail")} onclick={mailTo}><Mail class="size-4" /></button>
          <button class="hover:bg-accent rounded p-1.5" title={t("card.copy_address")} onclick={() => copy(c.email)}><Copy class="size-4" /></button>
        </div>
      </div>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
      {#if loading && !data}
        <div class="text-muted-foreground flex items-center gap-2 border-t px-5 py-3"><Loader class="size-4 animate-spin" />{t("common.loading")}</div>
      {/if}
      {#if isGroup && data}
        <div class="border-t px-5 py-3">
          <button class="flex items-center gap-1 text-[15px]" onclick={() => (showMembers = !showMembers)}>
            {t("card.members")}{#if showMembers}<ChevronDown class="size-4" />{:else}<ChevronRight class="size-4" />{/if}
          </button>
          {#if !showMembers}
            <button class="mt-2 flex items-center gap-1.5" onclick={() => (showMembers = true)}>
              {#each data.members.slice(0, 5) as m (m)}
                <Avatar name={displayName(m)} email={splitNameAddr(m).addr} class="size-8 text-[11px]" />
              {/each}
              {#if data.members.length > 5}<span class="ml-1">{t("card.more_members", data.members.length - 5)}</span>{/if}
              {#if !data.members.length}<span class="text-muted-foreground">{t("card.no_members")}</span>{/if}
            </button>
          {:else}
            <div class="mt-2 space-y-0.5">
              {#each data.members as m (m)}
                <button data-card-trigger class="hover:bg-accent flex w-full items-center gap-2 rounded px-1 py-1 text-left" onclick={(e) => people.openCard(m, e)}>
                  <Avatar name={displayName(m)} email={splitNameAddr(m).addr} class="size-7 text-[10px]" />
                  <span class="min-w-0 flex-1 truncate">{displayName(m)}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
      <div class="border-t px-5 py-3">
        <div class="mb-2 text-[15px]">{isGroup ? t("card.contact") : t("card.person")}</div>
        <div class="selectable space-y-1.5">
          <div class="flex items-center gap-3">
            <Mail class="text-muted-foreground size-4 shrink-0" />
            <button class="text-primary truncate hover:underline" onclick={mailTo}>{data?.email || c.email}</button>
          </div>
          {#each data?.phones ?? [] as [k, v] (k + v)}
            <div class="flex items-center gap-3">
              <Phone class="text-muted-foreground size-4 shrink-0" />
              <!-- Arama uygulaması olmadığından numara bağlantı değil, tıklayınca kopyalanır -->
              <button class="hover:text-primary text-left" title={t("ribbon.copy")} onclick={() => copy(v)}>{v}</button>
              {#if PHONE_KEYS[k]}<span class="text-muted-foreground text-xs">{t(PHONE_KEYS[k])}</span>{/if}
            </div>
          {/each}
        </div>
      </div>
      {#if data && (data.company || data.office || data.manager)}
        <div class="border-t px-5 py-3">
          <div class="mb-2 text-[15px]">{t("card.organization")}</div>
          <div class="selectable space-y-1">
            {#if data.company}<div class="flex items-center gap-3"><Building2 class="text-muted-foreground size-4 shrink-0" />{data.company}</div>{/if}
            {#if data.office}<div class="text-muted-foreground pl-7">{t("card.office")}: {data.office}</div>{/if}
            {#if data.manager}<div class="text-muted-foreground pl-7">{t("card.manager")}: {data.manager}</div>{/if}
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}
