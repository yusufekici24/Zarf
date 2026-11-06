<script lang="ts" module>
  import { SvelteSet } from "svelte/reactivity";
  /** Grup (dağıtım listesi) olduğu bilinen adresler — tüm alanlarda ortak */
  export const knownGroups = new SvelteSet<string>();
  const checked = new Set<string>();

  /** Toplantı odaları (bir kez sunucudan alınır) */
  let roomsPromise: Promise<{ list: string; name: string; email: string }[]> | null = null;
  export function roomCache(): Promise<{ list: string; name: string; email: string }[]> {
    roomsPromise ??= import("$lib/api").then((m) => m.api.listRooms()).catch(() => {
      roomsPromise = null;
      return [];
    });
    return roomsPromise;
  }

  // Alıcıları alanlar arasında (Kime ↔ Bilgi ↔ Gizli) ve alan içinde sürükle-bırak
  type FieldApi = { get: () => string[]; set: (list: string[]) => void };
  const fields = new Map<string, FieldApi>();
  let nextField = 1;
  export const dnd = $state<{ entry: string; from: string; index: number; x: number; y: number; over: string; at: number } | null>({ entry: "", from: "", index: -1, x: 0, y: 0, over: "", at: -1 }) as {
    entry: string;
    from: string;
    index: number;
    x: number;
    y: number;
    over: string;
    at: number;
  };

  /** İmlecin altındaki alan ve ekleme konumu */
  function hitTest(x: number, y: number): { field: string; at: number } {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const root = el?.closest<HTMLElement>("[data-addr-field]");
    if (!root) return { field: "", at: -1 };
    const field = root.dataset.addrField ?? "";
    const chips = Array.from(root.querySelectorAll<HTMLElement>("[data-chip]"));
    let at = chips.length;
    for (let i = 0; i < chips.length; i++) {
      const r = chips[i].getBoundingClientRect();
      const sameRow = y >= r.top - 2 && y <= r.bottom + 2;
      if ((sameRow && x < r.left + r.width / 2) || y < r.top - 2) {
        at = i;
        break;
      }
    }
    return { field, at };
  }

  function dropEntry() {
    const d = dnd;
    const src = fields.get(d.from);
    const dst = fields.get(d.over);
    if (!src || !dst || d.index < 0) return;
    const sList = src.get();
    const item = sList[d.index];
    if (item === undefined) return;
    if (d.from === d.over) {
      const list = [...sList];
      list.splice(d.index, 1);
      const at = d.at > d.index ? d.at - 1 : d.at;
      list.splice(Math.max(0, Math.min(list.length, at)), 0, item);
      src.set(list);
    } else {
      const addr = (x: string) => (x.match(/<([^>]+)>/)?.[1] ?? x).trim().toLowerCase();
      src.set(sList.filter((_, j) => j !== d.index));
      const dList = dst.get().filter((x) => addr(x) !== addr(item));
      dList.splice(Math.max(0, Math.min(dList.length, d.at)), 0, item);
      dst.set(dList);
    }
  }
</script>

<script lang="ts">
  // Outlook gibi alıcı alanı: yazdıkça kişiler, son kullanılanlar ve kurum dizininden (GAL)
  // öneri; tamamlanan adresler "chip" olarak gösterilir. Değer virgülle ayrılmış metindir.
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { toast } from "svelte-sonner";
  import Plus from "@lucide/svelte/icons/plus";
  import { t } from "$lib/i18n.svelte";
  import { splitAddresses, displayName, isExchangeDn, fold } from "$lib/format";
  import { people } from "$lib/stores/people.svelte";
  import X from "@lucide/svelte/icons/x";
  import Avatar from "$lib/components/common/Avatar.svelte";
  import User from "@lucide/svelte/icons/user";
  import History from "@lucide/svelte/icons/history";
  import Building2 from "@lucide/svelte/icons/building-2";
  import Users from "@lucide/svelte/icons/users";
  import DoorOpen from "@lucide/svelte/icons/door-open";

  let {
    value = $bindable(""),
    placeholder = "",
    id = undefined,
    onvalue,
    outlook = false,
    collapse = 0,
    rooms = false,
    pills = false,
  }: {
    value?: string;
    placeholder?: string;
    id?: string;
    onvalue?: (v: string) => void;
    /** Outlook ileti penceresi görünümü (altı çizili adlar) */
    outlook?: boolean;
    /** Odakta değilken en fazla bu kadar kişi gösterilir, kalanı "+N diğer" */
    collapse?: number;
    /** Yalnızca toplantı odaları önerilir (Outlook "Odalar" alanı) */
    rooms?: boolean;
    /** Yeni ileti penceresi: alıcılar fotoğraflı/baş harfli kişi hapı olarak (× ile kaldırılır) */
    pills?: boolean;
  } = $props();

  type Suggestion = { name: string; email: string; source: string };

  let text = $state("");
  let open = $state(false);
  let active = $state(0);
  let suggestions = $state<Suggestion[]>([]);
  let input: HTMLInputElement | undefined = $state();
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const EMAIL_RE = /^[^\s@<>,;]+@[^\s@<>,;]+\.[^\s@<>,;]+$/;

  const fieldId = `f${nextField++}`;
  $effect(() => {
    fields.set(fieldId, { get: () => chips, set: (list) => setChips(list) });
    return () => fields.delete(fieldId);
  });

  // Sürükleme: 5 pikselden fazla kayınca başlar; bırakılınca hedef alana/konuma taşınır
  let press: { i: number; x: number; y: number } | null = null;
  let suppressClick = false;
  function chipDown(e: PointerEvent, i: number) {
    if (e.button !== 0 || editing != null) return;
    if ((e.target as HTMLElement).closest("[data-no-drag]")) return;
    press = { i, x: e.clientX, y: e.clientY };
    window.addEventListener("pointermove", chipMove);
    window.addEventListener("pointerup", chipUp, { once: true });
  }
  function chipMove(e: PointerEvent) {
    if (!press) return;
    if (!dnd.entry) {
      if (Math.abs(e.clientX - press.x) + Math.abs(e.clientY - press.y) < 6) return;
      dnd.entry = chips[press.i];
      dnd.from = fieldId;
      dnd.index = press.i;
    }
    dnd.x = e.clientX;
    dnd.y = e.clientY;
    const h = hitTest(e.clientX, e.clientY);
    dnd.over = h.field;
    dnd.at = h.at;
  }
  function chipUp() {
    window.removeEventListener("pointermove", chipMove);
    press = null;
    if (dnd.entry) {
      suppressClick = true;
      setTimeout(() => (suppressClick = false), 0);
      if (dnd.over) dropEntry();
      dnd.entry = "";
      dnd.from = "";
      dnd.index = -1;
      dnd.over = "";
      dnd.at = -1;
    }
  }

  /** Tamamlanmış alıcılar (değer metninden) */
  const chips = $derived(splitAddresses(value).filter((s) => s.trim()));
  let focusWithin = $state(false);
  let expandedAll = $state(false);
  // Outlook: odakta değilken en fazla iki satır ad ve "+N diğer" (sığan kadar, ölçülerek)
  let fit = $state(Infinity);
  let measuring = $state(false);
  const showAll = $derived(!collapse || focusWithin || expandedAll || chips.length <= 1);
  const visibleChips = $derived(showAll || measuring ? chips : chips.slice(0, Math.min(fit, outlook ? chips.length : collapse)));

  function measure() {
    if (!root || showAll) {
      measuring = false;
      return;
    }
    const els = Array.from(root.querySelectorAll<HTMLElement>("[data-chip]"));
    const tops = [...new Set(els.map((e) => e.offsetTop))].sort((a, b) => a - b);
    const maxTop = tops[1] ?? tops[0] ?? 0;
    let n = els.filter((e) => e.offsetTop <= maxTop).length;
    if (n < els.length && n > 0) {
      // "+N diğer" için ikinci satırın sonunda yer kalsın
      const last = els[n - 1];
      if (last.offsetLeft + last.offsetWidth + 96 > root.clientWidth) n--;
    }
    fit = Math.max(1, n);
    measuring = false;
  }
  $effect(() => {
    if (!outlook || !collapse || !root) return;
    void chips;
    void showAll;
    const run = () => {
      if (showAll) return;
      measuring = true;
      requestAnimationFrame(() => requestAnimationFrame(measure));
    };
    run();
    let w = root.clientWidth;
    const ro = new ResizeObserver(() => {
      if (root && Math.abs(root.clientWidth - w) > 4) {
        w = root.clientWidth;
        run();
      }
    });
    ro.observe(root);
    return () => ro.disconnect();
  });
  const hiddenCount = $derived(chips.length - visibleChips.length);

  /** Adı yoksa dizindeki görünen ad (ör. grup adı), yoksa adres */
  function nameOf(entry: string): string {
    const n = displayName(entry);
    if (n && !n.includes("@")) return n;
    const a = addrOf(entry);
    return (a.includes("@") && people.name(a)) || n || a;
  }

  function addrOf(entry: string): string {
    const m = entry.match(/<([^>]+)>/);
    return (m ? m[1] : entry).trim();
  }

  function valid(entry: string): boolean {
    // Kişi grupları birden çok adres içerebilir
    return splitAddresses(entry).every((e) => EMAIL_RE.test(addrOf(e)) || isExchangeDn(addrOf(e)));
  }

  function setChips(list: string[]) {
    value = list.join(", ");
    onvalue?.(value);
  }

  function query(q: string) {
    clearTimeout(timer);
    if (!q.trim()) {
      suggestions = [];
      return;
    }
    const my = ++seq;
    if (rooms) {
      const words = fold(q.trim()).split(/\s+/).filter(Boolean);
      const pick = (all: { name: string; email: string }[]) =>
        all.filter((r) => words.every((w) => fold(`${r.name} ${r.email}`).includes(w))).slice(0, 50).map((r) => ({ name: r.name, email: r.email, source: "room" }));
      void roomCache().then((all) => {
        if (my === seq) suggestions = pick(all);
      });
      // Listede olmayan odalar için sunucuda da aranır (Outlook "Tüm Odalar")
      if (q.trim().length >= 2) {
        timer = setTimeout(() => {
          void api
            .listRooms(q.trim())
            .then((all) => {
              if (my === seq) suggestions = pick(all);
            })
            .catch(() => {});
        }, 350);
      }
      return;
    }
    // Yerel öneriler hemen, dizin (GAL) araması kısa bir beklemeden sonra
    void api.suggestRecipients(q, false).then((r) => {
      if (my === seq) suggestions = r;
    });
    if (q.trim().length >= 3) {
      timer = setTimeout(() => {
        void api.suggestRecipients(q, true).then((r) => {
          if (my === seq) suggestions = r;
        });
      }, 350);
    }
  }

  function commitText() {
    const raw = text.trim().replace(/[,;]+$/, "");
    if (!raw) return;
    // Tek bir öneriye birebir uyuyorsa onu kullan (ör. yalnızca ad yazıldı)
    const exact = suggestions.find((s) => s.email.toLowerCase() === raw.toLowerCase() || s.name.toLowerCase() === raw.toLowerCase());
    // Oda alanı: yalnızca listedeki odalar eklenir (tek eşleşme varsa o, yoksa yazılan atılmaz
    // ama geçersiz oda da oluşturulmaz)
    if (rooms && !exact) {
      if (suggestions.length === 1) {
        setChips([...chips, entryOf(suggestions[0])]);
        text = "";
        suggestions = [];
      } else if (raw.includes("@")) {
        setChips([...chips, ...splitAddresses(raw)]);
        text = "";
      }
      return;
    }
    const entries = exact ? [entryOf(exact)] : splitAddresses(raw);
    setChips([...chips, ...entries]);
    text = "";
    suggestions = [];
    if (!exact) resolveBare(entries);
  }

  function entryOf(s: Suggestion): string {
    if (s.source === "group") return s.email;
    return s.name && s.name !== s.email ? `${s.name.replace(/[",]/g, "")} <${s.email}>` : s.email;
  }

  // Alandaki adreslerden grup olanları (bir kez) sunucuya sor: Outlook gibi "+" gösterilir
  $effect(() => {
    const addrs = chips.map((c) => addrOf(c).toLowerCase()).filter((a) => a.includes("@") && !checked.has(a));
    if (!addrs.length || !app.accounts.some((a) => a.kind === "exchange")) return;
    addrs.forEach((a) => checked.add(a));
    void api
      .groupAddresses(addrs)
      .then((g) => g.forEach((x) => knownGroups.add(x.toLowerCase())))
      .catch(() => addrs.forEach((a) => checked.delete(a)));
  });

  let expanding = $state<number | null>(null);
  /** Grubu üyelerine aç: üyeler grubun yerine aynı alana eklenir */
  async function expandGroup(i: number) {
    const c = chips[i];
    expanding = i;
    try {
      const members = await api.expandGroup(addrOf(c));
      const have = new Set(chips.map((x) => addrOf(x).toLowerCase()));
      const add = members.filter((m) => !have.has(addrOf(m).toLowerCase()));
      setChips([...chips.slice(0, i), ...add, ...chips.slice(i + 1)]);
    } catch (e) {
      toast.error(String(e));
    } finally {
      expanding = null;
    }
  }

  function choose(s: Suggestion) {
    if (s.source === "dl") knownGroups.add(s.email.toLowerCase());
    setChips([...chips, entryOf(s)]);
    text = "";
    suggestions = [];
    open = false;
    input?.focus();
  }

  function remove(i: number) {
    setChips(chips.filter((_, j) => j !== i));
    input?.focus();
  }

  // Çift tıklama: alıcı yerinde düzenlenir (silinmez); Enter/odak kaybı kaydeder, Esc vazgeçer
  let editing = $state<number | null>(null);
  let editText = $state("");
  /** Düzenlenen alıcının özgün hali (adres değişmezse adıyla geri konur) */
  let editOrig = "";
  function edit(i: number) {
    editing = i;
    editOrig = chips[i];
    // Yalnızca adres düzenlenir; ad, kaydedince rehberden/dizinden yeniden bulunur
    const a = addrOf(chips[i]);
    editText = a.includes("@") ? a : chips[i];
    queueMicrotask(() => {
      const el = root?.querySelector<HTMLInputElement>("[data-chip-edit]");
      el?.focus();
      el?.select();
    });
  }
  function commitEdit() {
    if (editing == null) return;
    const i = editing;
    editing = null;
    const orig = editOrig;
    const repl = splitAddresses(editText.replace(/;/g, ",")).filter((x) => x.trim()).map((x) => (addrOf(x).toLowerCase() === addrOf(orig).toLowerCase() && isBare(x) ? orig : x));
    setChips([...chips.slice(0, i), ...repl, ...chips.slice(i + 1)]);
    resolveBare(repl);
  }

  /** Yalnızca adres (ad yok) */
  const isBare = (c: string) => !/<[^>]+>/.test(c) && c.includes("@");

  /**
   * Adı olmayan adresleri rehberde ve kurum dizininde arar; bulunursa kişi ya da grup adıyla
   * gösterilir (ör. "destek@kurum.com.tr" → "Teknik Destek")
   */
  function resolveBare(entries: string[]) {
    for (const e of entries.filter(isBare)) {
      const addr = addrOf(e).toLowerCase();
      void api
        .suggestRecipients(addr, true)
        .then((r) => {
          const hit = r.find((x) => x.email.toLowerCase() === addr && x.name && x.name.toLowerCase() !== addr);
          if (!hit) return;
          if (hit.source === "dl") knownGroups.add(addr);
          const named = `${hit.name.replace(/[",]/g, "")} <${hit.email}>`;
          setChips(chips.map((c) => (isBare(c) && addrOf(c).toLowerCase() === addr ? named : c)));
        })
        .catch(() => {});
    }
  }
  function editKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      commitEdit();
      input?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      editing = null;
      input?.focus();
    }
  }
  /** Ada tek tıklama: kişi kartı (Outlook) */
  function card(e: MouseEvent, c: string) {
    e.stopPropagation();
    if (suppressClick) return;
    people.openCard(c, e);
  }

  async function forget(e: MouseEvent, s: Suggestion) {
    e.preventDefault();
    e.stopPropagation();
    await api.forgetRecipient(s.email);
    suggestions = suggestions.filter((x) => x !== s);
  }

  function oninput() {
    // Virgül/noktalı virgül yazılınca adres tamamlanır
    if (/[,;]\s*$/.test(text)) {
      commitText();
      return;
    }
    open = true;
    active = 0;
    query(text);
  }

  function onkeydown(e: KeyboardEvent) {
    const list = open ? suggestions : [];
    if (e.key === "ArrowDown" && list.length) {
      active = (active + 1) % list.length;
      e.preventDefault();
    } else if (e.key === "ArrowUp" && list.length) {
      active = (active - 1 + list.length) % list.length;
      e.preventDefault();
    } else if ((e.key === "Enter" || e.key === "Tab") && list.length && text.trim()) {
      choose(list[active]);
      e.preventDefault();
    } else if (e.key === "Enter" && text.trim()) {
      commitText();
      e.preventDefault();
    } else if (e.key === "Backspace" && !text && chips.length) {
      edit(chips.length - 1);
      e.preventDefault();
    } else if (e.key === "Delete" && list.length && e.shiftKey && list[active]?.source === "recent") {
      void forget(new MouseEvent("click"), list[active]);
      e.preventDefault();
    } else if (e.key === "Escape" && open) {
      open = false;
      e.stopPropagation();
    }
  }

  function onpaste(e: ClipboardEvent) {
    const s = e.clipboardData?.getData("text") ?? "";
    if (/[,;\n]/.test(s)) {
      e.preventDefault();
      setChips([...chips, ...splitAddresses(s.replace(/\n/g, ","))]);
    }
  }

  let root: HTMLDivElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();
  let maxH = $state(288);
  let placeUp = $state(false);
  /** Liste pencereye göre konumlanır (fixed): küçük kaydırmalı kutuların içinde kırpılmaz */
  let pos = $state({ left: 0, top: 0, bottom: 0, width: 320 });

  /** "fixed" öğenin konum aldığı kutu: dönüşümlü (transform) bir üst öğe varsa o kutu (ör.
   *  ortalanmış iletişim kutusu), yoksa pencere. Yoksa liste alanın altında değil kaymış açılır. */
  function containingBox(): { left: number; top: number; height: number } {
    for (let el = root?.parentElement; el && el !== document.body; el = el.parentElement) {
      const cs = getComputedStyle(el);
      if (cs.transform !== "none" || cs.filter !== "none" || cs.perspective !== "none" || /paint|layout|strict|content/.test(cs.contain) || cs.willChange.includes("transform")) {
        const b = el.getBoundingClientRect();
        return { left: b.left, top: b.top, height: b.height };
      }
    }
    return { left: 0, top: 0, height: window.innerHeight };
  }

  function place() {
    if (!root) return;
    const r = root.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 12;
    const above = r.top - 12;
    placeUp = below < 200 && above > below;
    maxH = Math.max(120, Math.min(320, placeUp ? above : below));
    const width = Math.min(512, Math.max(280, r.width));
    const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
    const cb = containingBox();
    pos = { left: left - cb.left, top: r.bottom + 4 - cb.top, bottom: cb.top + cb.height - r.top + 4, width };
  }

  // Liste açılınca pencerede kalan alana göre yükseklik, yön ve konum
  $effect(() => {
    if (!open || !suggestions.length || !root) return;
    void text;
    void chips.length;
    place();
  });

  // Klavyeyle seçilen öneri görünür kalsın
  $effect(() => {
    const i = active;
    listEl?.querySelector(`[data-idx="${i}"]`)?.scrollIntoView({ block: "nearest" });
  });

  const ICON: Record<string, typeof User> = { room: DoorOpen, contact: User, recent: History, directory: Building2, group: Users, dl: Users };
  const LABEL: Record<string, string> = { room: "calendar.room", contact: "compose.src_contact", recent: "compose.src_recent", directory: "compose.src_directory", group: "compose.src_group", dl: "compose.src_group" };
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={root}
  role="group"
  data-addr-field={fieldId}
  class="relative flex min-h-8 min-w-0 flex-1 flex-wrap items-center py-0.5 {dnd.entry && dnd.over === fieldId ? 'bg-primary/5 rounded-sm ring-1 ring-primary/50' : ''} {outlook ? 'gap-x-0 gap-y-0' : 'gap-1'} {outlook && showAll ? 'max-h-28 overflow-y-auto' : ''} {outlook && measuring ? (pills ? 'max-h-[80px] overflow-hidden' : 'max-h-[54px] overflow-hidden') : ''}"
  onfocusin={() => (focusWithin = true)}
  onfocusout={(e) => { if (!root?.contains(e.relatedTarget as Node)) focusWithin = false; }}
  onclick={(e) => { if (e.target === root) input?.focus(); }}
>
  {#each visibleChips as c, i (i + c)}
    {@const ok = valid(c)}
    {@const group = knownGroups.has(addrOf(c).toLowerCase())}
    {#if editing === i}
      <!-- Yerinde düzenleme -->
      <input
        data-chip-edit
        class="border-primary h-6 rounded-sm border bg-transparent px-1 text-[13px] outline-none"
        style="width:{Math.min(420, Math.max(120, editText.length * 7.2 + 16))}px"
        bind:value={editText}
        onkeydown={editKey}
        onblur={commitEdit}
      />
    {:else if outlook && pills}
      <!-- Kişi hapı: fotoğraf/baş harfler, ad ve ×. Tek tık kişi kartı, çift tık düzenleme,
           sürükleyerek başka alana ya da sıraya taşınır. -->
      {#if dnd.entry && dnd.over === fieldId && dnd.at === i}<span class="bg-primary mx-px inline-block h-5 w-0.5 self-center rounded"></span>{/if}
      <span
        data-chip
        role="listitem"
        class="my-0.5 mr-1.5 inline-flex h-7 max-w-full items-center gap-1.5 rounded-[4px] border border-black/10 pr-0.5 pl-0.5 text-[13px] dark:border-white/15 {ok ? 'bg-[#f3f6fa] hover:bg-(--primary-hover) dark:bg-white/5 dark:hover:bg-white/10' : 'border-destructive text-destructive bg-destructive/10 border'} {dnd.entry && dnd.from === fieldId && dnd.index === i ? 'opacity-40' : ''}"
        title={c}
        onpointerdown={(e) => chipDown(e, i)}
      >
        {#if group}
          <button type="button" data-no-drag class="text-primary hover:bg-primary/15 rounded p-0.5" title={t("compose.expand_group")} disabled={expanding === i} onclick={(e) => { e.stopPropagation(); void expandGroup(i); }}>
            <Plus class="size-4 {expanding === i ? 'animate-spin' : ''}" />
          </button>
        {:else}
          <Avatar name={nameOf(c)} email={addrOf(c)} class="size-6 text-[10px]" />
        {/if}
        <button
          type="button"
          data-card-trigger
          class="truncate text-left {ok ? (group ? 'font-semibold' : '') : 'decoration-wavy underline'}"
          onclick={(e) => card(e, c)}
          ondblclick={(e) => { e.stopPropagation(); people.card = null; edit(i); }}
          onkeydown={(e) => { if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); remove(i); } }}
        >{nameOf(c)}</button>
        <button
          type="button"
          data-no-drag
          class="text-muted-foreground hover:text-foreground hover:bg-black/10 dark:hover:bg-white/15 flex size-6 shrink-0 items-center justify-center rounded-[3px]"
          aria-label={t("compose.remove_recipient")}
          title={t("compose.remove_recipient")}
          onclick={(e) => { e.stopPropagation(); remove(i); }}
        ><X class="size-3.5" /></button>
      </span>
    {:else if outlook}
      <!-- Outlook: "Ad; Ad;" sıkı dizilir. Üzerine gelince ad hafifçe vurgulanır ve köşesinde
           yer kaplamayan küçük × çıkar. Tek tık kişi kartı, çift tık düzenleme, sürükleyerek
           başka alana (Kime/Bilgi/Gizli) ya da başka sıraya taşınır. -->
      {#if dnd.entry && dnd.over === fieldId && dnd.at === i}<span class="bg-primary mx-px inline-block h-4 w-0.5 self-center rounded"></span>{/if}
      <span
        data-chip
        role="listitem"
        class="group/chip relative my-px inline-flex max-w-full items-center rounded-sm text-[13px] hover:bg-(--primary-hover) dark:hover:bg-white/10 {dnd.entry && dnd.from === fieldId && dnd.index === i ? 'opacity-40' : ''}"
        title={c}
        onpointerdown={(e) => chipDown(e, i)}
      >
        {#if group}
          <button type="button" data-no-drag class="text-primary hover:bg-primary/15 rounded p-px" title={t("compose.expand_group")} disabled={expanding === i} onclick={(e) => { e.stopPropagation(); void expandGroup(i); }}>
            <Plus class="size-3.5 {expanding === i ? 'animate-spin' : ''}" />
          </button>
        {/if}
        <button
          type="button"
          data-card-trigger
          class="truncate px-0.5 text-left underline decoration-1 underline-offset-2 {ok ? (group ? 'font-semibold' : '') : 'text-destructive decoration-wavy'}"
          onclick={(e) => card(e, c)}
          ondblclick={(e) => { e.stopPropagation(); people.card = null; edit(i); }}
          onkeydown={(e) => { if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); remove(i); } }}
        >{nameOf(c)}</button><span class="pr-1">;</span>
        <button
          type="button"
          data-no-drag
          class="bg-background text-muted-foreground hover:text-foreground absolute -top-1.5 -right-0.5 z-10 hidden size-3.5 items-center justify-center rounded-full border shadow-sm group-hover/chip:flex"
          aria-label={t("compose.remove_recipient")}
          title={t("compose.remove_recipient")}
          onclick={(e) => { e.stopPropagation(); remove(i); }}
        ><X class="size-2.5" /></button>
      </span>
    {:else}
      <span
        class="flex max-w-72 items-center gap-1 rounded-full border py-0.5 pr-1 pl-2 text-xs {ok ? 'bg-secondary' : 'border-destructive text-destructive bg-destructive/10'}"
        title={c}
        ondblclick={(e) => { e.stopPropagation(); people.card = null; edit(i); }}
      >
        {#if group}
          <button type="button" class="hover:bg-primary/15 text-primary -ml-1 rounded-full p-0.5" title={t("compose.expand_group")} disabled={expanding === i} onclick={(e) => { e.stopPropagation(); void expandGroup(i); }}>
            <Plus class="size-3 {expanding === i ? 'animate-spin' : ''}" />
          </button>
        {/if}
        <span class="truncate">{nameOf(c)}</span>
        <button type="button" class="hover:bg-muted-foreground/20 rounded-full p-0.5" aria-label={t("common.delete")} onclick={(e) => { e.stopPropagation(); remove(i); }}>
          <X class="size-3" />
        </button>
      </span>
    {/if}
  {/each}
  {#if dnd.entry && dnd.over === fieldId && dnd.at >= visibleChips.length}<span class="bg-primary mx-px inline-block h-4 w-0.5 self-center rounded"></span>{/if}
  {#if dnd.entry && dnd.from === fieldId}
    <!-- Sürüklenen alıcının hayaleti -->
    <div class="bg-popover pointer-events-none fixed z-[100] max-w-72 truncate rounded-sm border px-2 py-0.5 text-[13px] shadow-md" style="left:{dnd.x + 12}px;top:{dnd.y + 8}px">{nameOf(dnd.entry)}</div>
  {/if}
  {#if hiddenCount > 0}
    <!-- mousedown'da açılır: tıklama odağı alana taşıyınca düğme kaybolduğundan click hiç gelmiyordu -->
    <button
      type="button"
      class="text-primary px-1 text-[13px] font-semibold hover:underline"
      onmousedown={(e) => { e.preventDefault(); expandedAll = true; }}
      onclick={() => (expandedAll = true)}
    >+{hiddenCount} {t("mail.n_others")}</button>
  {/if}
  <input
    {id}
    bind:this={input}
    bind:value={text}
    placeholder={chips.length ? "" : placeholder}
    autocomplete="off"
    class="h-7 flex-1 bg-transparent px-1 text-sm outline-none {hiddenCount > 0 ? 'w-2 min-w-2' : ''}"
    style={hiddenCount > 0 ? undefined : `min-width:${chips.length ? Math.max(24, Math.min(320, text.length * 7.5 + 16)) : 128}px`}
    {oninput}
    {onkeydown}
    {onpaste}
    onblur={() => setTimeout(() => { open = false; commitText(); }, 150)}
  />
  {#if open && suggestions.length && text.trim()}
    <!-- Öneri listesi pencereye sığar: belirli bir yükseklikten sonra kaydırılır,
         alt kenara yakınsa yukarı doğru açılır -->
    <div
      bind:this={listEl}
      class="bg-popover text-popover-foreground fixed z-[95] overflow-y-auto overscroll-contain rounded-md border shadow-md"
      style="max-height:{maxH}px;left:{pos.left}px;width:{pos.width}px;{placeUp ? `bottom:${pos.bottom}px` : `top:${pos.top}px`}"
    >
      {#each suggestions as s, i (s.email + i)}
        {@const Icon = ICON[s.source] ?? User}
        <div
          data-idx={i}
          role="option"
          aria-selected={i === active}
          tabindex="-1"
          class="group flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left text-sm {i === active ? 'bg-accent' : 'hover:bg-accent'}"
          onmousedown={(e) => { e.preventDefault(); choose(s); }}
        >
          <Icon class="text-muted-foreground size-4 shrink-0" />
          <div class="min-w-0 flex-1">
            <div class="truncate font-medium">{s.name || s.email}</div>
            {#if s.name && s.source !== "group"}<div class="text-muted-foreground truncate text-xs">{s.email}</div>{/if}
          </div>
          <span class="text-muted-foreground shrink-0 text-[10px] uppercase">{t(LABEL[s.source] ?? "compose.src_contact")}</span>
          {#if s.source === "recent"}
            <button type="button" class="hover:bg-muted rounded p-0.5 opacity-0 group-hover:opacity-100" title={t("compose.forget_recipient")} onmousedown={(e) => forget(e, s)}>
              <X class="size-3.5" />
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
