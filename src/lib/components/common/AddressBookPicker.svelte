<script lang="ts">
  // Outlook "Ad Seç: Genel Adres Listesi": arama (tüm sütunlar / yalnızca ad), adres defteri
  // seçimi (kurum dizini, kişiler, oda listesi), sütunlu liste. Çift tıklama ya da düğme
  // seçileni alttaki alana ekler; birden çok kişi eklenebilir.
  import * as Dialog from "$lib/components/ui/dialog";
  import { untrack } from "svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import { splitAddresses, displayName, fold } from "$lib/format";
  import User from "@lucide/svelte/icons/user";
  import Users from "@lucide/svelte/icons/users";
  import DoorOpen from "@lucide/svelte/icons/door-open";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import X from "@lucide/svelte/icons/x";
  import Loader from "@lucide/svelte/icons/loader-circle";
  import { app } from "$lib/stores/app.svelte";
  import Avatar from "./Avatar.svelte";
  import Mail from "@lucide/svelte/icons/mail";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import IdCard from "@lucide/svelte/icons/id-card";
  import { listen } from "@tauri-apps/api/event";

  type Row = { name: string; email: string; title: string; department: string; office: string; phone: string; kind: string };

  let {
    open = $bindable(false),
    title = "",
    fieldLabel = "",
    source: initialSource = "gal",
    limit = 0,
    fields = [],
    initial = {},
    onpick = () => {},
    onpickFields,
    browse = false,
  }: {
    /** Ana penceredeki "Adres Defteri": yalnızca göz atma; çift tıklama kişinin bilgilerini açar */
    browse?: boolean;
    open?: boolean;
    title?: string;
    /** Alttaki alanın düğme adı (ör. "Takvim", "Kime") */
    fieldLabel?: string;
    source?: "gal" | "contacts" | "rooms";
    /** En çok kaç kişi seçilebilir (0 = sınırsız) */
    limit?: number;
    onpick?: (entries: string[]) => void;
    /** Birden çok alan (Outlook yeni ileti: Kime / Bilgi / Gizli) */
    fields?: { key: string; label: string }[];
    /** Alanların başlangıç değerleri */
    initial?: Record<string, string>;
    onpickFields?: (r: Record<string, string[]>) => void;
  } = $props();

  const fieldList = $derived(fields.length ? fields : [{ key: "main", label: fieldLabel || t("picker.add") }]);
  let values = $state<Record<string, string>>({});
  /** Çift tıklamanın ekleneceği alan (son kullanılan) */
  let target = $state("main");

  let source = $state<string>("gal");
  /** Outlook "Adres Defteri" listesi: her Exchange hesabının Genel Adres Listesi ve odaları, kişiler */
  const books = $derived.by(() => {
    const ex = app.accounts.filter((a) => a.kind === "exchange");
    const out: { value: string; label: string }[] = [];
    for (const a of ex) out.push({ value: ex.length > 1 ? `gal:${a.id}` : "gal", label: `${t("picker.gal")} - ${a.email}` });
    if (!ex.length) out.push({ value: "gal", label: t("picker.gal") });
    for (const a of ex.slice(0, 1)) out.push({ value: "rooms", label: `${t("picker.rooms")} - ${a.email}` });
    if (!ex.length) out.push({ value: "rooms", label: t("picker.rooms") });
    out.push({ value: "contacts", label: t("picker.contacts") });
    return out;
  });
  const bookLabel = $derived(books.find((b) => b.value === source)?.label ?? t("picker.gal"));
  let query = $state("");
  let nameOnly = $state(true);
  let rows = $state<Row[]>([]);
  let more = $state(false);
  let loading = $state(false);
  let error = $state("");
  let active = $state(-1);

  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (!open) return;
    const src = initialSource;
    untrack(() => {
      source = src === "gal" ? (books.find((b) => b.value.startsWith("gal"))?.value ?? "gal") : src;
      query = "";
      values = Object.fromEntries(fieldList.map((f) => [f.key, initial[f.key] ?? ""]));
      target = fieldList[0].key;
      active = -1;
      void load(true);
    });
  });

  // Adres defteri arka planda indirildiğinde açık liste yerel kopyadan yenilenir
  $effect(() => {
    if (!open) return;
    let off: (() => void) | undefined;
    let gone = false;
    listen("directory://updated", () => {
      if (source.startsWith("gal")) void load(true);
    })
      .then((f) => (gone ? f() : (off = f)))
      .catch(() => {});
    return () => {
      gone = true;
      off?.();
    };
  });

  async function load(reset: boolean) {
    const my = ++seq;
    loading = true;
    error = "";
    try {
      const r = await api.directoryPage(source, query, reset ? 0 : rows.length);
      if (my !== seq) return;
      rows = reset ? r.rows : [...rows, ...r.rows];
      more = r.more;
      if (reset) active = rows.length ? 0 : -1;
    } catch (e) {
      if (my !== seq) return;
      if (reset) rows = [];
      more = false;
      // Sunucu boş aramada tüm listeyi vermiyorsa kullanıcıdan yazması istenir
      error = query.trim() ? String(e) : t("picker.type_to_search");
    } finally {
      if (my === seq) loading = false;
    }
  }

  // "Yalnızca ad": sunucu sonuçları ada göre süzülür; "Tüm sütunlar": unvan, bölüm, konum da
  const shown = $derived.by(() => {
    const q = fold(query.trim());
    if (!q) return rows;
    return rows.filter((r) =>
      nameOnly
        ? q.split(/\s+/).every((w) => fold(r.name).includes(w)) || fold(r.email).startsWith(q)
        : [r.name, r.email, r.title, r.department, r.office, r.phone].some((x) => fold(x).includes(q)),
    );
  });

  function oninput() {
    clearTimeout(timer);
    timer = setTimeout(() => load(true), 300);
  }

  function entryOf(r: Row): string {
    return r.name && r.name !== r.email ? `${r.name.replace(/[",;<>]/g, " ").trim()} <${r.email}>` : r.email;
  }

  const listOf = (key: string) => splitAddresses(values[key] ?? "").filter((x) => x.trim());
  const total = $derived(fieldList.reduce((n, f) => n + listOf(f.key).length, 0));

  function add(r: Row | undefined, key = target) {
    if (!r) return;
    target = key;
    const list = listOf(key);
    if (list.some((x) => (x.match(/<([^>]+)>/)?.[1] ?? x).trim().toLowerCase() === r.email.toLowerCase())) return;
    if (limit && total >= limit) return;
    values = { ...values, [key]: [...list, entryOf(r)].join("; ") };
  }

  function onscroll(e: Event) {
    const el = e.currentTarget as HTMLElement;
    if (more && !loading && el.scrollTop + el.clientHeight > el.scrollHeight - 200) void load(false);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      active = Math.min(shown.length - 1, active + 1);
      document.querySelector(`[data-pick="${active}"]`)?.scrollIntoView({ block: "nearest" });
      e.preventDefault();
    } else if (e.key === "ArrowUp") {
      active = Math.max(0, active - 1);
      document.querySelector(`[data-pick="${active}"]`)?.scrollIntoView({ block: "nearest" });
      e.preventDefault();
    } else if (e.key === "Enter") {
      if (browse) void openProps(shown[active]);
      else add(shown[active]);
      e.preventDefault();
    }
  }

  function ok() {
    if (!total && shown[active]) add(shown[active], fieldList[0].key);
    const r = Object.fromEntries(fieldList.map((f) => [f.key, listOf(f.key)]));
    open = false;
    if (onpickFields) onpickFields(r);
    else if (r.main?.length) onpick(r.main);
  }

  // ------------------------------------------------------------- Özellikler (Outlook gibi)
  type Card = Awaited<ReturnType<typeof api.contactCard>>;
  let propsOpen = $state(false);
  let propsRow = $state<Row | null>(null);
  let propsData = $state<Card | null>(null);
  let propsLoading = $state(false);
  async function openProps(r: Row | undefined) {
    if (!r) return;
    propsRow = r;
    propsData = null;
    propsOpen = true;
    propsLoading = true;
    try {
      const d = await api.contactCard(r.email);
      if (propsRow?.email === r.email) propsData = d;
    } catch {
      /* dizinde yok: listedeki bilgiler gösterilir */
    } finally {
      propsLoading = false;
    }
  }
  function mailTo(r: Row | null | undefined) {
    if (!r) return;
    open = false;
    propsOpen = false;
    app.compose({ mode: "new", data: { to: entryOf(r) } });
  }
  function addToContacts(r: Row | null | undefined) {
    if (!r) return;
    window.dispatchEvent(new CustomEvent("lynora:add-contact", { detail: entryOf(r) }));
  }
  const PHONE_KEYS: Record<string, string> = { BusinessPhone: "card.phone_work", MobilePhone: "card.phone_mobile", HomePhone: "card.phone_home" };

  const ICON = (k: string) => (/room/i.test(k) ? DoorOpen : /dl|distribution|group/i.test(k) ? Users : User);
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="z-[75] flex h-[min(600px,92vh)] w-[min(820px,96vw)] max-w-none gap-0 rounded-md p-0 sm:max-w-none" overlayClass="z-[75]" showCloseButton={false}>
    <div class="flex h-9 shrink-0 items-center border-b pl-3 text-[13px]">
      <Dialog.Title class="flex-1 text-[13px] font-normal">{browse ? t("picker.book_title") : title || t("picker.title")}: {bookLabel}</Dialog.Title>
      <button class="flex h-full w-11 items-center justify-center hover:bg-[#c42b1c] hover:text-white" aria-label={t("common.close")} onclick={() => (open = false)}><X class="size-4" /></button>
    </div>
    <div class="flex min-h-0 flex-1 flex-col gap-2 p-3 text-[13px]">
      <div class="flex flex-wrap items-end gap-x-6 gap-y-1">
        <div class="min-w-64 flex-1">
          <div class="mb-1 flex items-center gap-3">
            <span class="font-semibold">{t("picker.search")}:</span>
            <label class="flex items-center gap-1"><input type="radio" checked={!nameOnly} onchange={() => (nameOnly = false)} />{t("picker.all_columns")}</label>
            <label class="flex items-center gap-1"><input type="radio" checked={nameOnly} onchange={() => (nameOnly = true)} />{t("picker.name_only")}</label>
          </div>
          <div class="flex items-center gap-1">
            <!-- svelte-ignore a11y_autofocus -->
            <input class="h-7 flex-1 rounded-sm border px-2 outline-none focus:border-primary" autofocus bind:value={query} {oninput} {onkeydown} />
            <button class="hover:bg-accent flex size-7 items-center justify-center rounded-sm border" aria-label={t("picker.search")} onclick={() => load(true)}><ArrowRight class="size-4" /></button>
            <button class="hover:bg-accent flex size-7 items-center justify-center rounded-sm border" aria-label={t("common.clear")} onclick={() => { query = ""; void load(true); }}><X class="size-4" /></button>
          </div>
        </div>
        <div class="w-80">
          <div class="mb-1 font-semibold">{t("picker.address_book")}:</div>
          <select class="h-7 w-full rounded-sm border bg-transparent px-1" bind:value={source} onchange={() => load(true)}>
            {#each books as b (b.value)}<option value={b.value}>{b.label}</option>{/each}
          </select>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-auto border" {onscroll}>
        <table class="w-full min-w-[720px] table-fixed border-collapse text-[12px]">
          {#if source === "rooms"}
            <!-- Outlook "Oda Seç": Ad, Konum, Kapasite, Açıklama -->
            <colgroup><col class="w-[38%]" /><col class="w-[16%]" /><col class="w-[9%]" /><col class="w-[10%]" /><col class="w-[27%]" /></colgroup>
          {:else}
            <colgroup><col class="w-[30%]" /><col class="w-[14%]" /><col class="w-[12%]" /><col class="w-[12%]" /><col class="w-[12%]" /><col class="w-[20%]" /></colgroup>
          {/if}
          <thead class="bg-background sticky top-0 z-10">
            <tr class="border-b text-left">
              {#each source === "rooms" ? ["picker.col_name", "picker.col_location", "picker.col_capacity", "picker.col_description", "picker.col_email"] : ["picker.col_name", "picker.col_title", "picker.col_phone", "picker.col_office", "picker.col_department", "picker.col_email"] as h (h)}
                <th class="border-r px-1.5 py-1 font-normal">{t(h)}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each shown as r, i (r.email + i)}
              {@const Icon = ICON(r.kind)}
              <tr
                data-pick={i}
                class="cursor-default {i === active ? 'bg-[#cce8ff] dark:bg-primary/30' : 'hover:bg-[#e5f3ff] dark:hover:bg-white/5'}"
                onclick={() => (active = i)}
                ondblclick={() => (browse ? openProps(r) : add(r))}
              >
                <td class="truncate px-1.5 py-0.5"><span class="flex items-center gap-1.5"><Icon class="text-muted-foreground size-3.5 shrink-0" /><span class="truncate {/dl|group/i.test(r.kind) ? 'font-semibold' : ''}">{r.name}</span></span></td>
                {#if source === "rooms"}
                  <td class="truncate px-1.5" title={r.department}>{r.office}</td>
                  <td class="truncate px-1.5"></td>
                  <td class="truncate px-1.5">{t("picker.room_desc")}</td>
                  <td class="truncate px-1.5">{r.email}</td>
                {:else}
                  <td class="truncate px-1.5">{r.title}</td>
                  <td class="truncate px-1.5">{r.phone}</td>
                  <td class="truncate px-1.5">{r.office}</td>
                  <td class="truncate px-1.5">{r.department}</td>
                  <td class="truncate px-1.5">{r.email}</td>
                {/if}
              </tr>
            {/each}
          </tbody>
        </table>
        {#if loading}
          <div class="text-muted-foreground flex items-center justify-center gap-2 p-3"><Loader class="size-4 animate-spin" />{t("common.loading")}</div>
        {:else if error}
          <div class="text-muted-foreground p-6 text-center">{error}</div>
        {:else if !shown.length}
          <div class="text-muted-foreground p-6 text-center">{t("common.no_results")}</div>
        {/if}
      </div>

      {#if browse}
        <!-- Göz atma: seçili kişi için Yeni İleti / Kişilere Ekle / Özellikler -->
        <div class="flex flex-wrap items-center gap-2">
          <button class="hover:bg-accent flex h-7 items-center gap-1.5 rounded-sm border px-3 disabled:opacity-40" disabled={!shown[active]} onclick={() => mailTo(shown[active])}><Mail class="size-4" />{t("picker.new_message")}</button>
          <button class="hover:bg-accent flex h-7 items-center gap-1.5 rounded-sm border px-3 disabled:opacity-40" disabled={!shown[active]} onclick={() => addToContacts(shown[active])}><UserPlus class="size-4" />{t("picker.add_contact")}</button>
          <button class="hover:bg-accent flex h-7 items-center gap-1.5 rounded-sm border px-3 disabled:opacity-40" disabled={!shown[active]} onclick={() => openProps(shown[active])}><IdCard class="size-4" />{t("picker.properties")}</button>
          <span class="flex-1"></span>
          <button class="border-primary hover:bg-accent h-7 min-w-20 rounded-sm border-2 px-4" onclick={() => (open = false)}>{t("common.close")}</button>
        </div>
      {:else}
      <div class="grid grid-cols-[auto_1fr] items-center gap-x-2 gap-y-1.5">
        {#each fieldList as f (f.key)}
          <button class="hover:bg-accent h-7 min-w-20 rounded-sm border px-3 text-left" onclick={() => add(shown[active], f.key)}>{f.label} →</button>
          <input
            class="h-7 rounded-sm border px-2 outline-none focus:border-primary {target === f.key && fieldList.length > 1 ? 'border-primary/60' : ''}"
            value={values[f.key] ?? ""}
            onfocus={() => (target = f.key)}
            oninput={(e) => (values = { ...values, [f.key]: (e.target as HTMLInputElement).value })}
            title={listOf(f.key).map(displayName).join("; ")}
          />
        {/each}
      </div>
      {#if limit && total >= limit}<div class="text-xs text-amber-700">{t("picker.limit", limit)}</div>{/if}
      <div class="flex justify-end gap-2">
        <button class="border-primary hover:bg-accent h-7 min-w-20 rounded-sm border-2 px-4" onclick={ok}>{t("common.ok")}</button>
        <button class="hover:bg-accent h-7 min-w-20 rounded-sm border px-4" onclick={() => (open = false)}>{t("common.cancel")}</button>
      </div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<!-- Kişi özellikleri (Outlook adres defterinde çift tıklama) -->
<Dialog.Root bind:open={propsOpen}>
  <Dialog.Content class="z-[80] w-[min(520px,94vw)] max-w-none gap-0 rounded-md p-0 sm:max-w-none" overlayClass="z-[80]" showCloseButton={false}>
    {@const r = propsRow}
    {@const d = propsData}
    {@const name = d?.name || r?.name || r?.email || ""}
    <div class="flex h-9 shrink-0 items-center border-b pl-3 text-[13px]">
      <Dialog.Title class="flex-1 truncate text-[13px] font-normal">{name}</Dialog.Title>
      <button class="flex h-full w-11 items-center justify-center hover:bg-[#c42b1c] hover:text-white" aria-label={t("common.close")} onclick={() => (propsOpen = false)}><X class="size-4" /></button>
    </div>
    {#if r}
      <div class="selectable max-h-[70vh] overflow-y-auto p-5 text-[13px]">
        <div class="flex items-center gap-4">
          <Avatar name={name} email={r.email} class="size-16 text-xl" />
          <div class="min-w-0">
            <div class="text-[17px] leading-tight break-words">{name}</div>
            {#if d?.job_title || r.title}<div class="text-muted-foreground mt-0.5">{d?.job_title || r.title}</div>{/if}
            {#if d?.department || r.department}<div class="text-muted-foreground">{d?.department || r.department}</div>{/if}
          </div>
        </div>
        <div class="mt-4 grid grid-cols-[130px_1fr] gap-x-3 gap-y-1.5">
          <span class="text-muted-foreground">{t("picker.col_email")}</span><span class="break-all">{d?.email || r.email}</span>
          {#if d?.company}<span class="text-muted-foreground">{t("picker.company")}</span><span>{d.company}</span>{/if}
          {#if d?.department || r.department}<span class="text-muted-foreground">{t("picker.col_department")}</span><span>{d?.department || r.department}</span>{/if}
          {#if d?.job_title || r.title}<span class="text-muted-foreground">{t("picker.col_title")}</span><span>{d?.job_title || r.title}</span>{/if}
          {#if d?.office || r.office}<span class="text-muted-foreground">{t("picker.col_office")}</span><span>{d?.office || r.office}</span>{/if}
          {#if d?.manager}<span class="text-muted-foreground">{t("card.manager")}</span><span>{d.manager}</span>{/if}
          {#each d?.phones?.length ? d.phones : r.phone ? [["BusinessPhone", r.phone] as [string, string]] : [] as [k, v] (k + v)}
            <span class="text-muted-foreground">{PHONE_KEYS[k] ? t(PHONE_KEYS[k]) : t("picker.col_phone")}</span><span>{v}</span>
          {/each}
        </div>
        {#if d?.members?.length}
          <div class="mt-4 mb-1.5 font-semibold">{t("card.members")} ({d.members.length})</div>
          <div class="max-h-56 overflow-y-auto rounded-sm border">
            {#each d.members as m (m)}
              <div class="flex items-center gap-2 border-b px-2 py-1 last:border-b-0">
                <Avatar name={displayName(m)} email={m.match(/<([^>]+)>/)?.[1] ?? m} class="size-6 text-[10px]" />
                <span class="min-w-0 flex-1 truncate">{displayName(m)}</span>
              </div>
            {/each}
          </div>
        {/if}
        {#if propsLoading}<div class="text-muted-foreground mt-3 flex items-center gap-2"><Loader class="size-4 animate-spin" />{t("common.loading")}</div>{/if}
      </div>
      <div class="bg-chrome flex flex-wrap justify-end gap-2 border-t px-4 py-3">
        <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded-[4px] border bg-background px-3 text-[13px]" onclick={() => mailTo(r)}><Mail class="size-4" />{t("picker.new_message")}</button>
        <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded-[4px] border bg-background px-3 text-[13px]" onclick={() => addToContacts(r)}><UserPlus class="size-4" />{t("picker.add_contact")}</button>
        <button class="bg-primary hover:bg-primary/90 h-8 min-w-24 rounded-[4px] px-4 text-[13px] text-white" onclick={() => (propsOpen = false)}>{t("common.close")}</button>
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>
