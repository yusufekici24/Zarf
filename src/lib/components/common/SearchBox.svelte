<script lang="ts">
  // Üst çubuktaki arama kutusu: yazdıkça öneriler (kişiler, toplantılar, ekler), sağdaki
  // seçenekler düğmesiyle gelişmiş arama (kimden, kime, konu, tarih, ek, durum, kapsam).
  // Başka yerden başlatılan aramalar da (ör. İlişkili Bul) kutuda görünür, × ile temizlenir.
  import { untrack, type Snippet } from "svelte";
  import * as Popover from "$lib/components/ui/popover";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { mail } from "$lib/stores/mail.svelte";
  import { i18n, t } from "$lib/i18n.svelte";
  import { fmtDateTime } from "$lib/format";
  import type { CalEvent } from "$lib/types";
  import Avatar from "$lib/components/common/Avatar.svelte";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import Inbox from "@lucide/svelte/icons/inbox";

  let { input = $bindable(), extra }: { input?: HTMLInputElement; extra?: Snippet } = $props();

  let text = $state("");
  let timer: ReturnType<typeof setTimeout> | undefined;
  const isMail = $derived(app.module === "mail");

  // Arama başka yerden başlatıldığında (ör. İlişkili Bul) kutuda görünür; temizlenince
  // İlişkili Bul'un açtığı "tüm posta kutuları" kapsamı eski haline döner
  $effect(() => {
    const q = app.search;
    untrack(() => {
      if (q !== text.trim()) {
        clearTimeout(timer);
        text = q;
      }
      if (!q && mail.searchAllPrev != null) {
        mail.searchAll = mail.searchAllPrev;
        mail.searchAllPrev = null;
      }
    });
  });
  // Modül değişince kutu boşalır
  $effect(() => {
    void app.module;
    untrack(() => {
      clearTimeout(timer);
      text = "";
      sugOpen = false;
    });
  });

  function oninput() {
    clearTimeout(timer);
    timer = setTimeout(() => (app.search = text.trim()), 250);
    void suggest();
  }
  function commit(q = text) {
    clearTimeout(timer);
    text = q;
    app.search = q.trim();
    sugOpen = false;
  }
  function clear() {
    commit("");
  }

  // ---------------------------------------------------------------- öneriler
  type Sug = { kind: "search" | "person" | "event" | "file" | "attach" | "all"; label: string; sub?: string; email?: string; ev?: CalEvent; id?: number; count?: number };
  let sugOpen = $state(false);
  let people = $state<{ name: string; email: string; count: number }[]>([]);
  let files = $state<{ id: number; name: string; subject: string; sender: string; date: number }[]>([]);
  let events = $state<CalEvent[]>([]);
  let active = $state(0);
  let seq = 0;
  let sugTimer: ReturnType<typeof setTimeout> | undefined;
  const q = $derived(text.trim());
  // Sorgu sözdizimi (kimden: vb.) yazılırken öneri gösterilmez
  const plainQuery = $derived(q.length >= 2 && !/\w:/.test(q));
  const sugs = $derived.by((): Sug[] => {
    if (!isMail || !plainQuery) return [];
    const out: Sug[] = [{ kind: "search", label: q }];
    for (const p of people) out.push({ kind: "person", label: p.name || p.email, sub: p.email, email: p.email, count: p.count });
    for (const f of files) out.push({ kind: "file", label: f.name, sub: [f.subject || t("mail.no_subject"), f.sender].filter(Boolean).join(" · "), id: f.id });
    for (const e of events) out.push({ kind: "event", label: e.title || t("mail.no_subject"), sub: fmtDateTime(e.start), ev: e });
    out.push({ kind: "attach", label: q });
    if (!mail.searchAll) out.push({ kind: "all", label: q });
    return out;
  });

  async function suggest() {
    clearTimeout(sugTimer);
    if (!isMail || !plainQuery) {
      people = [];
      files = [];
      events = [];
      return;
    }
    sugOpen = true;
    active = 0;
    const my = ++seq;
    const query = q;
    sugTimer = setTimeout(() => {
      // Kişiler (postalarda en sık geçenler önce, adres defteri) ve adı eşleşen ekler
      void api.searchSuggest(query).then((r) => {
        if (my !== seq) return;
        people = r.people.slice(0, 5);
        files = r.attachments.slice(0, 4);
      }).catch(() => {});
      void api.searchEvents(query).then((r) => {
        if (my === seq) events = r.slice(0, 3);
      }).catch(() => {});
    }, 150);
  }

  const tr = $derived(i18n.current === "tr");
  function pick(s: Sug) {
    // Kişi: o kişiden gelen ya da ona giden tüm iletiler (Outlook "Katılımcılar")
    if (s.kind === "person") commit(`${tr ? "kişi" : "participants"}:${s.email}`);
    else if (s.kind === "file" && s.id != null) {
      sugOpen = false;
      app.openMessage(s.id);
    }
    else if (s.kind === "event" && s.ev) {
      sugOpen = false;
      app.openItem("calendar", s.ev.id);
    } else if (s.kind === "attach") commit(`${tr ? "ekadı" : "attachmentname"}:${/\s/.test(s.label) ? `"${s.label}"` : s.label}`);
    else if (s.kind === "all") {
      mail.searchAll = true;
      commit(s.label);
    } else commit(s.label);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (sugOpen && sugs.length) sugOpen = false;
      else clear();
      return;
    }
    if (sugOpen && sugs.length && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      active = (active + (e.key === "ArrowDown" ? 1 : sugs.length - 1)) % sugs.length;
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (sugOpen && sugs[active]) pick(sugs[active]);
      else commit();
    }
  }

  // ---------------------------------------------------------------- gelişmiş arama
  type Adv = { from: string; to: string; subject: string; words: string; body: string; category: string; after: string; before: string; att: boolean; unread: boolean; flagged: boolean; important: boolean };
  const EMPTY: Adv = { from: "", to: "", subject: "", words: "", body: "", category: "", after: "", before: "", att: false, unread: false, flagged: false, important: false };
  let advOpen = $state(false);
  let adv = $state<Adv>({ ...EMPTY });
  let advAll = $state(false);

  /** Kutudaki sorguyu alanlara ayırır (arama sözdizimi: kimden:, konu:"…", ek:var, durum:…, sonra:, önce:) */
  function parse(query: string): Adv {
    const a: Adv = { ...EMPTY };
    const toks = query.match(/(?:[^\s"]+:"[^"]*"|"[^"]*"|\S+)/g) ?? [];
    const words: string[] = [];
    for (const tok of toks) {
      const m = tok.match(/^([^:@]+):(.+)$/);
      const key = m?.[1].toLocaleLowerCase("tr") ?? "";
      const val = (m?.[2] ?? "").replace(/^"|"$/g, "");
      const lv = val.toLocaleLowerCase("tr");
      if (["from", "kimden", "gönderen"].includes(key)) a.from = val;
      else if (["to", "kime", "alıcı"].includes(key)) a.to = val;
      else if (["subject", "konu"].includes(key)) a.subject = val;
      else if (["body", "gövde", "metin"].includes(key)) a.body = val;
      else if (["category", "categories", "kategori"].includes(key)) a.category = val;
      else if (["has", "hasattachment", "hasattachments", "ekvar", "ek", "attachment", "attachments"].includes(key)) a.att = !["no", "false", "hayır", "yok"].includes(lv);
      else if (["is", "durum"].includes(key) && ["unread", "okunmamış", "okunmadı"].includes(lv)) a.unread = true;
      else if (["is", "durum"].includes(key) && ["flagged", "bayraklı", "işaretli"].includes(lv)) a.flagged = true;
      else if (["is", "durum"].includes(key) && ["important", "önemli"].includes(lv)) a.important = true;
      else if (["after", "since", "sonra"].includes(key)) a.after = toIso(val);
      else if (["before", "önce"].includes(key)) a.before = toIso(val);
      else words.push(tok);
    }
    a.words = words.join(" ");
    return a;
  }
  function toIso(v: string): string {
    const m = v.match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})$/);
    return m ? `${m[3]}-${m[2].padStart(2, "0")}-${m[1].padStart(2, "0")}` : v;
  }
  /** Alanlardan sorgu metni (dile göre anahtar sözcüklerle) */
  function build(a: Adv): string {
    const k = tr
      ? { from: "kimden", to: "kime", subject: "konu", body: "gövde", category: "kategori", att: "ek:var", unread: "durum:okunmamış", flagged: "durum:bayraklı", important: "durum:önemli", after: "sonra", before: "önce" }
      : { from: "from", to: "to", subject: "subject", body: "body", category: "category", att: "has:attachment", unread: "is:unread", flagged: "is:flagged", important: "is:important", after: "after", before: "before" };
    const v = (s: string) => {
      const c = s.trim().replace(/"/g, "");
      return /\s/.test(c) ? `"${c}"` : c;
    };
    const parts: string[] = [];
    if (a.from.trim()) parts.push(`${k.from}:${v(a.from)}`);
    if (a.to.trim()) parts.push(`${k.to}:${v(a.to)}`);
    if (a.subject.trim()) parts.push(`${k.subject}:${v(a.subject)}`);
    if (a.body.trim()) parts.push(`${k.body}:${v(a.body)}`);
    if (a.category.trim()) parts.push(`${k.category}:${v(a.category)}`);
    if (a.after) parts.push(`${k.after}:${a.after}`);
    if (a.before) parts.push(`${k.before}:${a.before}`);
    if (a.att) parts.push(k.att);
    if (a.unread) parts.push(k.unread);
    if (a.flagged) parts.push(k.flagged);
    if (a.important) parts.push(k.important);
    if (a.words.trim()) parts.push(a.words.trim());
    return parts.join(" ");
  }

  $effect(() => {
    if (advOpen)
      untrack(() => {
        adv = parse(text.trim());
        advAll = mail.searchAll;
        sugOpen = false;
      });
  });
  function applyAdv() {
    mail.searchAll = advAll;
    commit(build(adv));
    advOpen = false;
  }
  function resetAdv() {
    adv = { ...EMPTY };
  }

  const field = "border-input bg-background focus:border-primary h-8 w-full rounded-md border px-2 text-[13px] outline-none";
</script>

<div class="relative w-full max-w-[600px]">
  <div class="bg-background focus-within:ring-primary/40 flex h-8 w-full items-center gap-2 rounded-[4px] border pl-2.5 focus-within:ring-2">
    <Search class="text-muted-foreground size-4 shrink-0" />
    <input
      bind:this={input}
      bind:value={text}
      {oninput}
      {onkeydown}
      onfocus={() => void suggest()}
      onblur={() => setTimeout(() => (sugOpen = false), 150)}
      placeholder={t(`search.placeholder_${app.module}`)}
      class="h-8 min-w-0 flex-1 bg-transparent text-[13px] outline-none"
    />
    {#if text}<button class="hover:bg-accent rounded p-0.5" title={t("search.clear")} aria-label={t("search.clear")} onclick={clear}><X class="size-4" /></button>{/if}
    {@render extra?.()}
    {#if isMail}
      <Popover.Root bind:open={advOpen}>
        <Popover.Trigger>
          {#snippet child({ props })}
            <button {...props} class="hover:bg-accent flex h-full w-9 shrink-0 items-center justify-center rounded-r-[4px] border-l {advOpen ? 'bg-accent text-primary' : ''}" title={t("search.advanced")} aria-label={t("search.advanced")}><SlidersHorizontal class="size-4" /></button>
          {/snippet}
        </Popover.Trigger>
        <Popover.Content align="end" sideOffset={6} class="w-[460px] p-4">
          <form class="grid grid-cols-[110px_1fr] items-center gap-x-3 gap-y-2 text-[13px]" onsubmit={(e) => { e.preventDefault(); applyAdv(); }}>
            <div class="col-span-2 mb-1 text-[14px] font-semibold">{t("search.advanced")}</div>
            <span>{t("search.scope")}</span>
            <select class={field} value={advAll ? "all" : "folder"} onchange={(e) => (advAll = (e.target as HTMLSelectElement).value === "all")}>
              <option value="folder">{t("search.scope_folder")}</option>
              <option value="all">{t("mail.search_all_mailboxes")}</option>
            </select>
            <label for="adv-from">{t("search.f_from")}</label><input id="adv-from" class={field} bind:value={adv.from} />
            <label for="adv-to">{t("search.f_to")}</label><input id="adv-to" class={field} bind:value={adv.to} />
            <label for="adv-subj">{t("search.f_subject")}</label><input id="adv-subj" class={field} bind:value={adv.subject} />
            <label for="adv-words">{t("search.f_words")}</label><input id="adv-words" class={field} bind:value={adv.words} />
            <label for="adv-body">{t("search.f_body")}</label><input id="adv-body" class={field} bind:value={adv.body} />
            <label for="adv-cat">{t("search.f_category")}</label>
            <select id="adv-cat" class={field} bind:value={adv.category}>
              <option value="">—</option>
              {#each app.categories as c (c.name)}<option value={c.name}>{c.name}</option>{/each}
            </select>
            <span>{t("search.f_date")}</span>
            <div class="flex items-center gap-2"><input type="date" class={field} bind:value={adv.after} /><span class="text-muted-foreground">–</span><input type="date" class={field} bind:value={adv.before} /></div>
            <span></span>
            <div class="grid grid-cols-2 gap-x-3 gap-y-1.5 pt-1">
              <label class="flex items-center gap-2"><input type="checkbox" class="accent-primary" bind:checked={adv.att} /><Paperclip class="size-3.5" />{t("search.f_attachments")}</label>
              <label class="flex items-center gap-2"><input type="checkbox" class="accent-primary" bind:checked={adv.unread} />{t("search.unread_only")}</label>
              <label class="flex items-center gap-2"><input type="checkbox" class="accent-primary" bind:checked={adv.flagged} />{t("search.flagged_only")}</label>
              <label class="flex items-center gap-2"><input type="checkbox" class="accent-primary" bind:checked={adv.important} />{t("search.important_only")}</label>
            </div>
            <div class="col-span-2 mt-2 flex justify-end gap-2">
              <button type="button" class="hover:bg-accent h-8 rounded-md px-3" onclick={resetAdv}>{t("search.reset")}</button>
              <button type="submit" class="bg-primary text-primary-foreground hover:bg-primary/90 h-8 rounded-md px-4 font-medium">{t("search.do_search")}</button>
            </div>
          </form>
        </Popover.Content>
      </Popover.Root>
    {/if}
  </div>

  {#if sugOpen && sugs.length}
    <!-- Öneriler: kutunun altında; seçmek için tıkla ya da ↑↓ ve Enter -->
    <div class="bg-popover text-popover-foreground absolute top-[calc(100%+4px)] right-0 left-0 z-50 overflow-hidden rounded-md border py-1 text-[13px] shadow-lg" role="listbox">
      {#each sugs as s, i (s.kind + i)}
        {#if (s.kind === "person" || s.kind === "event" || s.kind === "file") && sugs[i - 1]?.kind !== s.kind}
          <div class="text-muted-foreground px-3 pt-2 pb-1 text-[11px] font-semibold uppercase">{t(s.kind === "person" ? "search.sec_people" : s.kind === "file" ? "search.sec_files" : "search.sec_meetings")}</div>
        {/if}
        {#if s.kind === "attach"}<div class="bg-border my-1 h-px"></div>{/if}
        <div
          role="option"
          aria-selected={i === active}
          tabindex="-1"
          class="flex cursor-pointer items-center gap-2.5 px-3 py-1.5 {i === active ? 'bg-accent' : 'hover:bg-accent'}"
          onmousedown={(e) => { e.preventDefault(); pick(s); }}
          onmouseenter={() => (active = i)}
        >
          {#if s.kind === "person"}
            <Avatar name={s.label} email={s.email} class="size-7 text-[10px]" />
            <span class="min-w-0 flex-1"><span class="block truncate">{s.label}</span><span class="text-muted-foreground block truncate text-[12px]">{s.sub}</span></span>
            <span class="text-muted-foreground shrink-0 text-[11px]">{s.count ? t("search.n_messages", s.count) : ""}</span>
          {:else if s.kind === "file"}
            <span class="flex size-7 shrink-0 items-center justify-center rounded-md bg-amber-500/15 text-amber-700 dark:text-amber-400"><Paperclip class="size-4" /></span>
            <span class="min-w-0 flex-1"><span class="block truncate">{s.label}</span><span class="text-muted-foreground block truncate text-[12px]">{s.sub}</span></span>
          {:else if s.kind === "event"}
            <span class="bg-primary/10 text-primary flex size-7 shrink-0 items-center justify-center rounded-md"><CalendarIcon class="size-4" /></span>
            <span class="min-w-0 flex-1"><span class="block truncate">{s.label}</span><span class="text-muted-foreground block truncate text-[12px]">{s.sub}</span></span>
          {:else if s.kind === "attach"}
            <Paperclip class="text-muted-foreground size-4 shrink-0" /><span class="truncate">{t("search.in_attachments", s.label)}</span>
          {:else if s.kind === "all"}
            <Inbox class="text-muted-foreground size-4 shrink-0" /><span class="truncate">{t("search.in_all_mailboxes", s.label)}</span>
          {:else}
            <Search class="text-muted-foreground size-4 shrink-0" /><span class="truncate">{t("search.search_for", s.label)}</span>
            <span class="text-muted-foreground ml-auto shrink-0 text-[11px]">Enter</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
