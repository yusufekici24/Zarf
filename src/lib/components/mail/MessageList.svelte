<script lang="ts">
  import Avatar from "$lib/components/common/Avatar.svelte";
  import Loader from "@lucide/svelte/icons/loader-circle";
  // İleti listesi: sanal kaydırma, tarih grupları, çoklu seçim, sürükle-bırak, sağ tık menüsü.
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import { app } from "$lib/stores/app.svelte";
  import { mail } from "$lib/stores/mail.svelte";
  import { untrack } from "svelte";
  import { api } from "$lib/api";
  import { i18n, t } from "$lib/i18n.svelte";
  import { dateGroup, displayName, fmtListDate, splitAddresses, splitNameAddr, startOfDay, addDays, toTs } from "$lib/format";
  import type { MessageRow } from "$lib/types";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import Flag from "@lucide/svelte/icons/flag";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import AlertCircle from "@lucide/svelte/icons/circle-alert";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import Reply from "@lucide/svelte/icons/reply";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import MailOpen from "@lucide/svelte/icons/mail-open";
  import Mail from "@lucide/svelte/icons/mail";
  import FileText from "@lucide/svelte/icons/file-text";
  import Copy from "@lucide/svelte/icons/copy";
  import Printer from "@lucide/svelte/icons/printer";
  import ReplyAll from "@lucide/svelte/icons/reply-all";
  import Forward from "@lucide/svelte/icons/forward";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import MailSearch from "@lucide/svelte/icons/mail-search";
  import Zap from "@lucide/svelte/icons/zap";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import MailX from "@lucide/svelte/icons/mail-x";
  import ShieldBan from "@lucide/svelte/icons/shield-ban";
  import Archive from "@lucide/svelte/icons/archive";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  // Görünüm: ileti listesi yazı boyutu ve önizleme satırı (Outlook "İleti Önizlemesi")
  const FS = $derived(({ small: 12, normal: 13, large: 15, xlarge: 17 } as Record<string, number>)[app.setting("list_font", "normal")] ?? 13);
  const PREVIEW = $derived(Math.max(0, Math.min(3, Number(app.setting("preview_lines", "1")) || 0)));
  /** Satır yüksekliği: dolgu + ad ve konu satırları + önizleme satırları */
  const ROW = $derived(Math.round(15 + FS * 3 + PREVIEW * (FS - 1) * 1.35));
  const HEAD = 28;
  /** Açılmış konuşmadaki ileti satırı (gönderen + özet) */
  const CHILD = $derived(Math.round(12 + FS * 2.6));
  // Okunmamış: kalın (varsayılan) ya da yalnızca renkli (Seçenekler > Posta > Okuma Bölmesi)
  const BOLD = $derived(app.setting("unread_bold", "1") === "1");
  // Konuşma satırında aç/kapa oku (OWA gibi iletiler alt alta)
  const EXPANDABLE = $derived(mail.conversations && app.setting("conversation_expand", "1") === "1");

  type Item = { kind: "head"; label: string } | { kind: "row"; m: MessageRow } | { kind: "child"; m: MessageRow; head: number };

  const grouped = $derived(app.setting("group_by_date", "1") === "1" && mail.sortBy === "date");

  const items = $derived.by<Item[]>(() => {
    const out: Item[] = [];
    let last = "";
    for (const m of mail.messages) {
      if (grouped) {
        const g = m.is_pinned ? t("group.pinned") : dateGroup(m.date);
        if (g !== last) {
          out.push({ kind: "head", label: g });
          last = g;
        }
      }
      out.push({ kind: "row", m });
      const kids = EXPANDABLE ? mail.expanded[m.id] : undefined;
      // En yeni üstte (OWA listesi gibi)
      if (kids) for (const c of [...kids].sort((a, b) => b.date - a.date)) out.push({ kind: "child", m: c, head: m.id });
    }
    return out;
  });

  const offsets = $derived.by(() => {
    const o: number[] = [];
    let y = 0;
    for (const it of items) {
      o.push(y);
      y += it.kind === "head" ? HEAD : it.kind === "child" ? CHILD : ROW;
    }
    o.push(y);
    return o;
  });

  /** Konuşmadaki iletinin klasörü başka (ör. Gönderilmiş Öğeler) ise adı */
  function otherFolder(c: MessageRow): string {
    return c.folder_id === mail.folderId ? "" : (mail.folder(c.folder_id)?.display ?? "");
  }

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let height = $state(600);

  function findIndex(y: number): number {
    let lo = 0;
    let hi = items.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (offsets[mid] <= y) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  // Görünen aralık: kaydırma her pikselde değil, satır kümesi değişince yeniden hesaplanır
  // (kaydırırken bileşen ağacı yeniden işlenmez → akıcı kaydırma)
  const range = $derived.by(() => {
    if (!items.length) return "0:0";
    // Hızlı/akıcı kaydırmada boş satır görünmesin: üstte ve altta geniş tampon; aralık 8'er
    // satırlık adımlarla değişir (her satırda yeniden hesaplanmaz)
    const s = Math.max(0, (findIndex(scrollTop) & ~7) - 16);
    const e = Math.min(items.length, ((findIndex(scrollTop + height) + 8) & ~7) + 32);
    return `${s}:${e}`;
  });
  const visible = $derived.by(() => {
    const [s, e] = range.split(":").map(Number);
    return items.slice(s, e).map((it, i) => ({ it, top: offsets[s + i], idx: s + i }));
  });

  // Satır başına biçimlendirme (tarih, ad, kategoriler) bir kez hesaplanır
  type RowInfo = { who: string; date: string; cats: string[]; name: string; addr: string };
  let rowCache = new WeakMap<MessageRow, RowInfo>();
  let cacheKey = "";
  function info(m: MessageRow): RowInfo {
    const k = `${mail.sentLike}|${new Date().getDate()}`;
    if (k !== cacheKey) {
      cacheKey = k;
      rowCache = new WeakMap();
    }
    let r = rowCache.get(m);
    if (!r) {
      const p = avatarOf(m);
      r = { who: who(m), date: fmtListDate(m.date), cats: m.categories ? m.categories.split(",").filter(Boolean) : [], name: p.name, addr: p.addr };
      rowCache.set(m, r);
    }
    return r;
  }
  const showAvatars = $derived(app.setting("show_avatars", "1") === "1");
  const selectedSet = $derived(new Set(mail.selected));

  let rafPending = false;
  function onscroll() {
    if (rafPending) return;
    rafPending = true;
    requestAnimationFrame(() => {
      rafPending = false;
      scrollTop = scroller?.scrollTop ?? 0;
      rememberAnchor();
      // Liste sonuna yaklaşınca: önce yerelden sonraki sayfa, sonra sunucudan
      const total = offsets[offsets.length - 1] ?? 0;
      if (scrollTop + height > total - 1500) void mail.loadMore();
    });
  }

  // Seçilen öğe görünür kalsın — yalnızca seçim değiştiğinde (liste eşitleme ile
  // yenilendiğinde kaydırma konumuna dokunulmaz; aşağı kaydırırken yukarı atmaz)
  let lastSel: number | undefined;
  $effect(() => {
    const id = mail.selected[mail.selected.length - 1];
    if (id === lastSel) return;
    lastSel = id;
    if (id == null) return;
    untrack(() => {
      if (!scroller) return;
      const idx = items.findIndex((x) => x.kind === "row" && x.m.id === id);
      if (idx < 0) return;
      const top = offsets[idx];
      if (top < scroller.scrollTop) scroller.scrollTop = top;
      else if (top + ROW > scroller.scrollTop + height) scroller.scrollTop = top + ROW - height;
    });
  });

  // Liste yenilenince (yeni posta üste eklendi vb.) görünen ilk ileti yerinde kalır
  let anchor: { id: number; delta: number } | null = null;
  function rememberAnchor() {
    if (!scroller || scrollTop <= 0) {
      anchor = null;
      return;
    }
    const i = findIndex(scrollTop);
    for (let k = i; k < Math.min(items.length, i + 3); k++) {
      const it = items[k];
      if (it.kind === "row") {
        anchor = { id: it.m.id, delta: offsets[k] - scrollTop };
        return;
      }
    }
  }
  $effect(() => {
    void items;
    untrack(() => {
      if (!scroller || !anchor) return;
      const idx = items.findIndex((x) => x.kind === "row" && x.m.id === anchor!.id);
      if (idx < 0) return;
      const want = Math.max(0, offsets[idx] - anchor.delta);
      if (Math.abs(want - scroller.scrollTop) > 1) {
        scroller.scrollTop = want;
        scrollTop = want;
      }
    });
  });

  // Mesaj sayısı listenin yüksekliğinden azsa (ilk sayfa) sonrakini hemen iste
  $effect(() => {
    const total = offsets[offsets.length - 1] ?? 0;
    if (items.length && total < height + 400) untrack(() => void mail.loadMore());
  });

  // ---------------------------------------------------------------- sürükle-bırak
  let pressed: { id: number; x: number; y: number } | null = null;

  function onpointerdown(e: PointerEvent, m: MessageRow) {
    if (e.button !== 0) return;
    pressed = { id: m.id, x: e.clientX, y: e.clientY };
  }

  function onwindowmove(e: PointerEvent) {
    if (!pressed) return;
    if (!mail.drag) {
      if (Math.abs(e.clientX - pressed.x) + Math.abs(e.clientY - pressed.y) < 8) return;
      const ids = mail.selected.includes(pressed.id) ? [...mail.selected] : [pressed.id];
      const first = mail.messages.find((x) => x.id === ids[0]);
      mail.drag = { ids, label: ids.length > 1 ? t("mail.n_items", ids.length) : first?.subject || t("mail.no_subject"), x: e.clientX, y: e.clientY };
      // Açık yeni ileti pencereleri sürükleme süresince öne alınır (bırakılabilsin)
      overCompose = false;
      void api.dragMessages(e.screenX, e.screenY, [], "start").catch(() => {});
    } else {
      mail.drag.x = e.clientX;
      mail.drag.y = e.clientY;
    }
    // İmlecin altındaki yeni ileti penceresi (varsa) vurgulanır
    const now = Date.now();
    if (now - lastHover > 70) {
      lastHover = now;
      void api.dragMessages(e.screenX, e.screenY, [], "move").then((t) => (overCompose = !!t)).catch(() => {});
    }
  }
  let lastHover = 0;
  let overCompose = $state(false);

  function onwindowup(e: PointerEvent) {
    const d = mail.drag;
    pressed = null;
    if (!d) return;
    mail.drag = null;
    // Yeni ileti penceresine bırakıldıysa .msg eki olur (Outlook gibi); değilse klasöre taşı
    const x = e.clientX;
    const y = e.clientY;
    const ctrl = e.ctrlKey;
    const target0 = document.elementFromPoint(x, y);
    void api
      .dragMessages(e.screenX, e.screenY, d.ids, "drop")
      .then((label) => {
        if (label) return;
        const el = target0?.closest("[data-folder-id]") as HTMLElement | null;
        if (el) void mail.move(Number(el.dataset.folderId), d.ids, ctrl);
      })
      .catch(() => {});
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      mail.moveSelection(1, e.shiftKey);
      e.preventDefault();
    } else if (e.key === "ArrowUp") {
      mail.moveSelection(-1, e.shiftKey);
      e.preventDefault();
    } else if (e.key === "Enter" && mail.selected.length === 1) {
      mail.open(mail.selected[0]);
    } else if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
      mail.selectAll();
      e.preventDefault();
    }
  }

  // Avatar: gönderen (gönderilmişlerde ilk alıcı)
  function avatarOf(m: MessageRow): { name: string; addr: string } {
    const src = mail.sentLike ? (splitAddresses(m.to_addr)[0] ?? m.to_addr) : m.sender;
    const { name, addr } = splitNameAddr(src);
    return { name: name || addr || src, addr };
  }

  function who(m: MessageRow): string {
    if (mail.sentLike) return splitAddresses(m.to_addr).map(displayName).join(", ") || m.to_addr;
    return displayName(m.sender);
  }

  function contextSelect(m: MessageRow) {
    if (!mail.selected.includes(m.id)) void mail.select(m.id);
  }

  const today = () => toTs(startOfDay(new Date())) + 17 * 3600;
  // ------------------------------------------------------------- sağ tık menüsü
  const one = $derived(mail.selected.length === 1 ? mail.messages.find((x) => x.id === mail.selected[0]) : undefined);
  const allRead = $derived(mail.selected.every((id) => { const x = mail.messages.find((x) => x.id === id); return !x || !mail.unread(x); }));
  const senderName = $derived(one ? displayName(one.sender) || splitNameAddr(one.sender).addr : "");

  /** Yanıt/iletme önekleri atılmış konu (konuşmayı bulmak için) */
  function baseSubject(s: string): string {
    return s.replace(/^(\s*(re|fw|fwd|ynt|ilt|ileti|yanıt|aw|wg|sv|tr|rv)\s*(\[\d+\])?\s*:\s*)+/i, "").trim();
  }

  /** Kopyala: seçili iletiler (üst bilgiler ve metin) panoya */
  async function copyItems() {
    const parts: string[] = [];
    for (const id of mail.selected.slice(0, 50)) {
      const m = await api.getMessage(id).catch(() => null);
      if (!m) continue;
      const head = [`${t("mail.from")}: ${m.sender}`, `${t("mail.date")}: ${new Date(m.date * 1000).toLocaleString()}`, `${t("mail.to")}: ${m.to_addr}`];
      if (m.cc) head.push(`Cc: ${m.cc}`);
      head.push(`${t("mail.subject")}: ${m.subject}`);
      parts.push(head.join("\n") + "\n\n" + (m.body_text || m.text || "").trim());
    }
    if (parts.length) await navigator.clipboard.writeText(parts.join("\n\n" + "-".repeat(40) + "\n\n")).catch(() => {});
  }

  $effect(() => {
    const h = () => void copyItems();
    window.addEventListener("lynora:copy-items", h);
    // Şeritteki "Yoksay" düğmesi
    const ig = () => void ignoreConversation();
    window.addEventListener("lynora:ignore", ig);
    return () => {
      window.removeEventListener("lynora:copy-items", h);
      window.removeEventListener("lynora:ignore", ig);
    };
  });

  /** Hızlı Yazdır: ileti okuma bölmesinde gösterilip doğrudan yazdırılır */
  async function quickPrint() {
    const id = mail.selected[0];
    if (id == null) return;
    if (mail.current?.id !== id) await mail.show(id);
    setTimeout(() => window.dispatchEvent(new Event("lynora:print")), 500);
  }

  /** İlişkili Bul: bu konuşmadaki ya da bu gönderenden gelen iletiler (tüm klasörlerde) */
  function findRelated(by: "conversation" | "sender") {
    if (!one) return;
    const val = by === "sender" ? splitNameAddr(one.sender).addr : baseSubject(one.subject).replace(/"/g, "");
    if (!val) return;
    // Outlook gibi: arama kutusuna sorgu yazılır (kimden:… / konu:"…"), × ile normale dönülür
    const tr = i18n.current === "tr";
    const q = by === "sender" ? `${tr ? "kimden" : "from"}:${val}` : `${tr ? "konu" : "subject"}:"${val}"`;
    if (mail.searchAllPrev == null) mail.searchAllPrev = mail.searchAll;
    mail.searchAll = true;
    app.search = q;
  }

  /** Yoksay: konuşmadaki iletiler silinir, gelecekte gelenler de doğrudan Silinmiş Öğeler'e gider */
  async function ignoreConversation() {
    if (!one) return;
    const subj = baseSubject(one.subject);
    if (!subj) return;
    if (!(await app.confirm(t("ctx.ignore_title"), t("ctx.ignore_text", subj), { ok: t("ctx.ignore"), destructive: true }))) return;
    try {
      await api.saveRule({ id: 0, name: t("ctx.ignore_rule", subj), enabled: true, account_id: one.account_id, field: "subject", op: "contains", value: subj, action: "delete", action_arg: "", stop: true });
    } catch (e) {
      app.error(e);
      return;
    }
    mail.selected = mail.messages.filter((m) => baseSubject(m.subject) === subj).map((m) => m.id);
    await mail.delete();
  }

  /** Kurallar: bu gönderenden gelenleri her zaman seçilen klasöre taşı */
  async function alwaysMove(folderId: number) {
    if (!one) return;
    const addr = splitNameAddr(one.sender).addr;
    try {
      await api.saveRule({ id: 0, name: t("ctx.always_move_rule", addr), enabled: true, account_id: one.account_id, field: "from", op: "contains", value: addr, action: "move", action_arg: String(folderId), stop: true });
      const ids = mail.messages.filter((m) => splitNameAddr(m.sender).addr.toLowerCase() === addr.toLowerCase()).map((m) => m.id);
      await mail.move(folderId, ids);
    } catch (e) {
      app.error(e);
    }
  }

  /** Hızlı Adımlar */
  async function quickDone() {
    const ids = mail.ids();
    await mail.setRead(ids, true);
    await mail.flag(false);
    await mail.moveToRole("archive");
  }
  async function replyAndDelete() {
    mail.reply("reply");
    await mail.delete();
  }

  const allFolders = $derived(app.folderGroups.flatMap((g) => g.folders.map((f) => ({ f, account: g.account?.email ?? t("folder.local") }))));
</script>

<svelte:window
  onpointermove={onwindowmove}
  onpointerup={onwindowup}
  onblur={() => {
    if (mail.drag) {
      mail.drag = null;
      pressed = null;
      void api.dragMessages(0, 0, [], "cancel").catch(() => {});
    }
  }}
/>

<ContextMenu.Root>
  <ContextMenu.Trigger class="relative min-h-0 flex-1">
    <div
      bind:this={scroller}
      bind:clientHeight={height}
      class="absolute inset-0 overflow-y-auto overscroll-contain outline-none"
      style="will-change: scroll-position"
      tabindex="0"
      role="listbox"
      aria-multiselectable="true"
      {onscroll}
      {onkeydown}
    >
      {#if !items.length}
        <div class="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 text-sm">
          <MailOpen class="size-8 opacity-40" />
          {mail.loading ? t("common.loading") : t("mail.empty_folder")}
        </div>
      {/if}
      <div style="height: {offsets[offsets.length - 1] ?? 0}px;contain:strict" class="relative">
        {#each visible as { it, top } (it.kind === "row" ? "r" + it.m.id : it.kind === "child" ? `c${it.head}:${it.m.id}` : "h" + it.label)}
          {#if it.kind === "head"}
            <div class="text-foreground absolute top-0 right-0 left-0 flex items-center px-3 text-xs font-semibold" style="transform:translateY({top}px);height:{HEAD}px">{it.label}</div>
          {:else if it.kind === "child"}
            <!-- Açılmış konuşmadaki ileti (OWA): girintili, gönderen ve özet; tıklayınca o ileti açılır -->
            {@const c = it.m}
            {@const sel = selectedSet.has(c.id)}
            {@const where = otherFolder(c)}
            <div
              role="option"
              tabindex="-1"
              aria-selected={sel}
              data-testid="thread-item"
              class="absolute top-0 right-0 left-0 flex cursor-default items-stretch border-b pr-2 {sel ? 'bg-(--primary-soft) dark:bg-primary/25' : 'bg-[#fafafa] hover:bg-(--primary-faint) dark:bg-white/3 dark:hover:bg-accent/60'}"
              style="transform:translateY({top}px);height:{CHILD}px;contain:layout paint style"
              onclick={() => mail.select(c.id, undefined, true)}
              onkeydown={(e) => e.key === "Enter" && void mail.select(c.id, undefined, true)}
              ondblclick={() => mail.open(c.id)}
            >
              <span class="ml-5 w-[3px] shrink-0 {c.is_read ? 'bg-border' : 'bg-primary'}"></span>
              <div class="min-w-0 flex-1 py-1 pl-3">
                <div class="flex items-center gap-1">
                  <span class="truncate {c.is_read ? '' : BOLD ? 'font-bold' : 'text-primary'}" style="font-size:{FS - 1}px">{displayName(c.sender) || t("mail.no_sender")}</span>
                  <span class="flex-1"></span>
                  {#if c.has_attachments}<Paperclip class="text-muted-foreground size-3" />{/if}
                  <span class="text-muted-foreground shrink-0 text-[11px]">{fmtListDate(c.date)}</span>
                </div>
                <div class="text-muted-foreground truncate" style="font-size:{FS - 2}px">{#if where}<span class="text-primary/80">{where}</span>{" · "}{/if}{c.snippet}</div>
              </div>
            </div>
          {:else}
            {@const m = it.m}
            {@const sel = selectedSet.has(m.id)}
            {@const ri = info(m)}
            {@const cats = ri.cats}
            {@const count = mail.conversations ? mail.threadCount(m) : 1}
            {@const read = !mail.unread(m)}
            <div
              role="option"
              tabindex="-1"
              aria-selected={sel}
              class="group absolute top-0 right-0 left-0 flex cursor-default gap-2 border-b py-1.5 pr-2 {sel ? 'bg-(--primary-soft) dark:bg-primary/25' : 'hover:bg-(--primary-faint) dark:hover:bg-accent/60'}"
              style="transform:translateY({top}px);height:{ROW}px;contain:layout paint style"
              onclick={(e) => mail.select(m.id, e)}
              ondblclick={() => mail.open(m.id)}
              onpointerdown={(e) => onpointerdown(e, m)}
              oncontextmenu={() => contextSelect(m)}
            >
              <!-- Outlook: soldaki dikdörtgen — okunmamışta mavi; satırın üzerine gelince belirir,
                   tıklayınca okunmadı (mavi) / tekrar tıklayınca okundu -->
              <button
                type="button"
                class="group/bar flex w-2.5 shrink-0 cursor-pointer items-stretch py-px pl-[2px]"
                title={read ? t("mail.mark_unread") : t("mail.mark_read")}
                aria-label={read ? t("mail.mark_unread") : t("mail.mark_read")}
                onpointerdown={(e) => e.stopPropagation()}
                ondblclick={(e) => e.stopPropagation()}
                onclick={(e) => { e.stopPropagation(); void mail.setRead([m.id], !read); }}
              >
                <span class="w-[5px] flex-none {read ? 'group-hover:bg-[#c8c8c8] group-hover/bar:bg-primary/70 dark:group-hover:bg-white/25' : 'bg-primary group-hover/bar:bg-primary/75'}"></span>
              </button>
              {#if EXPANDABLE}
                <!-- Konuşmayı listede aç/kapat (OWA) -->
                {#if count > 1}
                  {@const open = !!mail.expanded[m.id]}
                  <button
                    type="button"
                    class="text-muted-foreground hover:text-foreground -ml-1 flex w-4 shrink-0 items-start pt-1"
                    title={open ? t("mail.collapse_conversation") : t("mail.expand_conversation")}
                    aria-label={open ? t("mail.collapse_conversation") : t("mail.expand_conversation")}
                    aria-expanded={open}
                    onpointerdown={(e) => e.stopPropagation()}
                    ondblclick={(e) => e.stopPropagation()}
                    onclick={(e) => { e.stopPropagation(); void mail.toggleExpand(m.id); }}
                  >
                    {#if open}<ChevronDown class="size-4" />{:else}<ChevronRight class="size-4" />{/if}
                  </button>
                {:else}
                  <span class="-ml-1 w-4 shrink-0"></span>
                {/if}
              {/if}
              {#if showAvatars}
                <Avatar name={ri.name} email={ri.addr} class="mt-0.5 size-8 text-xs" />
              {/if}
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-1">
                  <span class="truncate {read ? '' : BOLD ? 'font-bold' : 'text-primary'}" style="font-size:{FS}px">{ri.who || t("mail.no_sender")}</span>
                  {#if count > 1}<span class="text-muted-foreground text-xs">({count})</span>{/if}
                  <span class="flex-1"></span>
                  {#if m.is_answered}<Reply class="text-muted-foreground size-3.5" />{/if}
                  {#if m.has_attachments}<Paperclip class="text-muted-foreground size-3.5" />{/if}
                  {#if m.importance === 2}<AlertCircle class="size-3.5 text-red-600" />{:else if m.importance === 0}<ArrowDown class="size-3.5 text-blue-500" />{/if}
                  <span class="text-muted-foreground shrink-0 text-xs {read ? '' : BOLD ? 'text-primary font-semibold' : 'text-primary'}">{ri.date}</span>
                </div>
                <div class="flex items-center gap-1">
                  {#if m.is_draft && !mail.sentLike}<span class="text-xs font-semibold text-red-600">[{t("mail.draft")}]</span>{/if}
                  {#if !PREVIEW}{#each cats as c (c)}<span class="size-2.5 shrink-0 rounded-sm" style="background:{app.categoryColor(c)}" title={c}></span>{/each}{/if}
                  <span class="truncate {read ? '' : BOLD ? 'text-primary font-semibold' : 'text-primary'}" style="font-size:{FS}px">{m.subject || t("mail.no_subject")}</span>
                  <span class="flex-1"></span>
                  <div class="hidden shrink-0 items-center gap-0.5 group-hover:flex">
                    <button class="hover:bg-background rounded p-0.5" title={t("common.delete")} onclick={(e) => { e.stopPropagation(); mail.selected = [m.id]; mail.delete(); }}><Trash2 class="size-3.5" /></button>
                    <button class="hover:bg-background rounded p-0.5" title={read ? t("mail.mark_unread") : t("mail.mark_read")} onclick={(e) => { e.stopPropagation(); mail.setRead([m.id], !read); }}>
                      {#if read}<Mail class="size-3.5" />{:else}<MailOpen class="size-3.5" />{/if}
                    </button>
                  </div>
                  <!-- Raptiye (Outlook "Sabitle"): sabitliyse hep görünür, tıklayınca kaldırılır -->
                  <button class="shrink-0 rounded p-0.5 {m.is_pinned ? 'text-primary' : 'text-muted-foreground opacity-0 group-hover:opacity-100'}" title={m.is_pinned ? t("mail.unpin") : t("mail.pin")} onclick={(e) => { e.stopPropagation(); void mail.togglePin(m.id); }}>
                    <Pin class="size-3.5 {m.is_pinned ? 'fill-current' : ''}" />
                  </button>
                  <button class="shrink-0 rounded p-0.5 {m.is_flagged ? 'text-red-600' : 'text-muted-foreground opacity-0 group-hover:opacity-100'}" title={t("mail.flag")} onclick={(e) => { e.stopPropagation(); mail.toggleFlag(m.id); }}>
                    <Flag class="size-3.5 {m.is_flagged ? 'fill-current' : ''}" />
                  </button>
                </div>
                {#if PREVIEW}
                  <div class="flex items-start gap-1">
                    {#each cats as c (c)}<span class="mt-[3px] size-2.5 shrink-0 rounded-sm" style="background:{app.categoryColor(c)}" title={c}></span>{/each}
                    <span class="text-muted-foreground min-w-0 overflow-hidden" style="font-size:{FS - 1}px;line-height:1.35;display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:{PREVIEW};word-break:break-word">{m.snippet}</span>
                  </div>
                {/if}
              </div>
            </div>
          {/if}
        {/each}
      </div>
      {#if mail.loadingMore}
        <div class="text-muted-foreground flex items-center justify-center gap-2 py-3 text-xs"><Loader class="size-3.5 animate-spin" />{t("common.loading")}</div>
      {:else if mail.loadingOlder}
        <div class="text-muted-foreground flex items-center justify-center gap-2 py-3 text-xs"><Loader class="size-3.5 animate-spin" />{t("mail.loading_older")}</div>
      {:else if mail.localDone && mail.canLoadOlder && items.length}
        <button class="text-primary w-full py-3 text-center text-xs hover:underline" onclick={() => mail.loadOlder()}>{t("mail.load_older")}</button>
      {/if}
    </div>
  </ContextMenu.Trigger>
  <ContextMenu.Content class="w-72">
    {#if mail.selected.length}
      <ContextMenu.Item onclick={copyItems}><Copy class="size-4 text-[#444]" />{t("ctx.copy")}<ContextMenu.Shortcut>Ctrl+C</ContextMenu.Shortcut></ContextMenu.Item>
      <ContextMenu.Item disabled={mail.selected.length !== 1} onclick={quickPrint}><Printer class="size-4 text-[#2f7d32]" />{t("ctx.quick_print")}</ContextMenu.Item>
      <ContextMenu.Separator />
      <ContextMenu.Item onclick={() => mail.reply("reply")}><Reply class="size-4 text-[#8e44ad]" />{t("mail.reply")}<ContextMenu.Shortcut>Ctrl+R</ContextMenu.Shortcut></ContextMenu.Item>
      <ContextMenu.Item onclick={() => mail.reply("reply_all")}><ReplyAll class="size-4 text-[#8e44ad]" />{t("mail.reply_all")}<ContextMenu.Shortcut>Ctrl+Shift+R</ContextMenu.Shortcut></ContextMenu.Item>
      <ContextMenu.Item onclick={() => mail.reply("forward")}><Forward class="size-4 text-primary" />{t("mail.forward")}<ContextMenu.Shortcut>Ctrl+F</ContextMenu.Shortcut></ContextMenu.Item>
      <ContextMenu.Separator />
      {#if allRead}
        <ContextMenu.Item onclick={() => mail.setRead(mail.ids(), false)}><Mail class="size-4 text-[#444]" />{t("mail.mark_unread")}<ContextMenu.Shortcut>Ctrl+U</ContextMenu.Shortcut></ContextMenu.Item>
      {:else}
        <ContextMenu.Item onclick={() => mail.setRead(mail.ids(), true)}><MailOpen class="size-4 text-[#444]" />{t("mail.mark_read")}<ContextMenu.Shortcut>Ctrl+Q</ContextMenu.Shortcut></ContextMenu.Item>
      {/if}
      {#if mail.messages.find((x) => x.id === mail.ids()[0])?.is_pinned}
        <ContextMenu.Item onclick={() => void mail.togglePin()}><PinOff class="size-4 text-primary" />{t("mail.unpin")}</ContextMenu.Item>
      {:else}
        <ContextMenu.Item onclick={() => void mail.togglePin()}><Pin class="size-4 text-primary" />{t("mail.pin")}</ContextMenu.Item>
      {/if}
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger><LayoutGrid class="mr-2 size-4 text-primary" />{t("ctx.categorize")}</ContextMenu.SubTrigger>
        <ContextMenu.SubContent class="w-52">
          {#each app.categories as c (c.name)}
            <ContextMenu.Item onclick={() => mail.toggleCategory(c.name)}><span class="size-3 rounded-sm" style="background:{c.color}"></span>{c.name}</ContextMenu.Item>
          {/each}
          <ContextMenu.Separator />
          <ContextMenu.Item onclick={async () => { await api.setCategories(mail.ids(), []); mail.refresh(); }}>{t("mail.clear_categories")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => app.openSettings("categories")}>{t("mail.all_categories")}</ContextMenu.Item>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger><Flag class="mr-2 size-4 text-[#c50f1f]" />{t("ctx.followup")}</ContextMenu.SubTrigger>
        <ContextMenu.SubContent class="w-52">
          <ContextMenu.Item onclick={() => mail.flag(true, today())}>{t("flag.today")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.flag(true, today() + 86400)}>{t("flag.tomorrow")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.flag(true, toTs(addDays(startOfDay(new Date()), Math.max(0, 5 - new Date().getDay()))) + 17 * 3600)}>{t("flag.this_week")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.flag(true, toTs(addDays(startOfDay(new Date()), 8 - (new Date().getDay() || 7))) + 17 * 3600)}>{t("flag.next_week")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.flag(true, null)}>{t("flag.no_date")}</ContextMenu.Item>
          <ContextMenu.Separator />
          <ContextMenu.Item onclick={() => mail.flag(false)}>{t("flag.complete")}</ContextMenu.Item>
          <ContextMenu.Separator />
          <ContextMenu.Item onclick={() => mail.setImportance(2)}>{t("importance.high")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.setImportance(1)}>{t("importance.normal")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.setImportance(0)}>{t("importance.low")}</ContextMenu.Item>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
      <ContextMenu.Separator />
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger disabled={!one}><MailSearch class="mr-2 size-4 text-[#444]" />{t("ctx.find_related")}</ContextMenu.SubTrigger>
        <ContextMenu.SubContent class="w-64">
          <ContextMenu.Item onclick={() => findRelated("conversation")}>{t("ctx.related_conversation")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => findRelated("sender")}>{t("ctx.related_sender")}</ContextMenu.Item>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger><Zap class="mr-2 size-4 text-[#f7a800]" />{t("ctx.quick_steps")}</ContextMenu.SubTrigger>
        <ContextMenu.SubContent class="w-60">
          <ContextMenu.Item onclick={quickDone}>{t("ctx.qs_done")}</ContextMenu.Item>
          <ContextMenu.Item disabled={!one} onclick={replyAndDelete}>{t("ctx.qs_reply_delete")}</ContextMenu.Item>
          <ContextMenu.Item disabled={!one} onclick={() => window.dispatchEvent(new CustomEvent("lynora:meeting-from-mail", { detail: one?.id }))}>{t("mail.reply_meeting")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.forwardAsAttachment()}>{t("mail.forward_attachment")}</ContextMenu.Item>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
      <ContextMenu.Separator />
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger disabled={!one}><ListFilter class="mr-2 size-4 text-primary" />{t("ctx.rules")}</ContextMenu.SubTrigger>
        <ContextMenu.SubContent class="w-72">
          <ContextMenu.Sub>
            <ContextMenu.SubTrigger><span class="truncate">{t("ctx.always_move", senderName)}</span></ContextMenu.SubTrigger>
            <ContextMenu.SubContent class="max-h-96 w-64 overflow-y-auto">
              {#each allFolders as { f, account } (f.id)}
                <ContextMenu.Item onclick={() => alwaysMove(f.id)}><span class="truncate">{f.display}</span><span class="text-muted-foreground ml-auto truncate text-xs">{account}</span></ContextMenu.Item>
              {/each}
            </ContextMenu.SubContent>
          </ContextMenu.Sub>
          <ContextMenu.Item onclick={() => app.openRulesFor(one?.sender ?? "")}>{t("rules.create_from")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => app.openSettings("rules")}>{t("ctx.manage_rules")}</ContextMenu.Item>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger><FolderInput class="mr-2 size-4 text-primary" />{t("mail.move")}</ContextMenu.SubTrigger>
        <ContextMenu.SubContent class="max-h-96 w-64 overflow-y-auto">
          {#each allFolders as { f, account } (f.id)}
            <ContextMenu.Item onclick={() => mail.move(f.id)}><span class="truncate">{f.display}</span><span class="text-muted-foreground ml-auto truncate text-xs">{account}</span></ContextMenu.Item>
          {/each}
          <ContextMenu.Separator />
          <ContextMenu.Sub>
            <ContextMenu.SubTrigger>{t("mail.copy_to")}</ContextMenu.SubTrigger>
            <ContextMenu.SubContent class="max-h-96 w-64 overflow-y-auto">
              {#each allFolders as { f, account } (f.id)}
                <ContextMenu.Item onclick={() => mail.move(f.id, mail.ids(), true)}><span class="truncate">{f.display}</span><span class="text-muted-foreground ml-auto truncate text-xs">{account}</span></ContextMenu.Item>
              {/each}
            </ContextMenu.SubContent>
          </ContextMenu.Sub>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
      <ContextMenu.Separator />
      <ContextMenu.Item disabled={!one} onclick={ignoreConversation}><MailX class="size-4 text-[#444]" />{t("ctx.ignore")}</ContextMenu.Item>
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger><ShieldBan class="mr-2 size-4 text-[#c50f1f]" />{t("ctx.junk")}</ContextMenu.SubTrigger>
        <ContextMenu.SubContent class="w-60">
          <ContextMenu.Item onclick={() => mail.blockSender()}>{t("mail.block_sender")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.moveToRole("junk")}>{t("mail.mark_junk")}</ContextMenu.Item>
          <ContextMenu.Item onclick={() => mail.moveToRole("inbox")}>{t("mail.not_junk")}</ContextMenu.Item>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
      <ContextMenu.Separator />
      <ContextMenu.Item onclick={() => mail.delete()}><Trash2 class="size-4 text-[#444]" />{t("common.delete")}<ContextMenu.Shortcut>Del</ContextMenu.Shortcut></ContextMenu.Item>
      <ContextMenu.Item onclick={() => mail.moveToRole("archive")}><Archive class="size-4 text-[#444]" />{t("ctx.archive")}<ContextMenu.Shortcut>Backspace</ContextMenu.Shortcut></ContextMenu.Item>
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>

{#if mail.drag}
  <div class="bg-primary text-primary-foreground pointer-events-none fixed z-[100] max-w-64 truncate rounded-md px-3 py-1.5 text-xs shadow-lg" style="left:{mail.drag.x + 12}px;top:{mail.drag.y + 8}px">
    {mail.drag.label}{#if overCompose} · {t("compose.attach")}{/if}
  </div>
{/if}
