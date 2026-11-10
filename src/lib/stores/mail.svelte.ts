// Posta modülü durumu.
import { api } from "$lib/api";
import { t } from "$lib/i18n.svelte";
import { app, type ComposeRequest } from "./app.svelte";
import { IS_MAIN } from "$lib/window";
import type { Folder, MessageRow, MessageView } from "$lib/types";
import { toast } from "svelte-sonner";

export const SEARCH_FOLDERS = [
  { key: "unread", label: "search.unread" },
  { key: "flagged", label: "search.flagged" },
  { key: "attachments", label: "search.attachments" },
  { key: "important", label: "search.important" },
];

const PAGE = 500;

export function normalizeSubject(s: string): string {
  let r = s.trim();
  for (;;) {
    const m = /^(re|fw|fwd|ynt|yanıt|ilt|i̇lt|ilet|aw|wg|tr|rv)\s*:\s*/i.exec(r);
    if (!m) break;
    r = r.slice(m[0].length);
  }
  return r.toLocaleLowerCase();
}

class MailState {
  folderId = $state<number | null>(null);
  searchFolder = $state<string | null>(null);
  filter = $state("all");
  searchAll = $state(false);
  /** İlişkili Bul öncesindeki "tüm posta kutuları" seçimi (arama temizlenince geri yüklenir) */
  searchAllPrev: boolean | null = null;
  /** Satırlar hiç yerinde değiştirilmez (her değişiklik yeni dizi atar): derin vekil gerekmez,
   *  binlerce satırda okuma ve atama ucuz kalır */
  messages = $state.raw<MessageRow[]>([]);
  selected = $state<number[]>([]);
  anchor = $state<number | null>(null);
  current = $state<MessageView | null>(null);
  thread = $state<MessageRow[]>([]);
  /** Listede açılmış konuşmalar (OWA gibi alt alta): baş satırın kimliği → konuşmadaki iletiler */
  expanded = $state.raw<Record<number, MessageRow[]>>({});
  loading = $state(false);
  sortBy = $state<"date" | "from" | "subject" | "importance" | "flag">("date");
  sortDesc = $state(true);
  drag = $state<{ ids: number[]; label: string; x: number; y: number } | null>(null);
  /** Daha eski iletiler yükleniyor (Exchange) */
  loadingOlder = $state(false);
  /** Sunucuda başka eski ileti kalmadığı bilinen klasörler */
  olderDone = $state<Record<number, boolean>>({});
  /** Sunucuda arama sürüyor */
  serverSearching = $state(false);
  private serverSearchKey = "";
  private readTimer: ReturnType<typeof setTimeout> | null = null;
  private loadSeq = 0;

  get conversations(): boolean {
    return app.setting("conversations", "0") === "1";
  }

  get readingPane(): string {
    return app.setting("reading_pane", "right");
  }

  folder(id: number | null = this.folderId): Folder | undefined {
    if (id == null) return undefined;
    for (const g of app.folderGroups) {
      const f = g.folders.find((x) => x.id === id);
      if (f) return f;
    }
    if (app.outbox?.id === id) return app.outbox;
    return undefined;
  }

  get sentLike(): boolean {
    const r = this.folder()?.role;
    return r === "sent" || r === "drafts" || r === "outbox";
  }

  get title(): string {
    if (this.searchFolder) return t(SEARCH_FOLDERS.find((s) => s.key === this.searchFolder)?.label ?? "");
    if (this.searchAll && app.search) return t("mail.search_all_title", app.search);
    return this.folder()?.display ?? "";
  }

  selectFolder(id: number) {
    if (this.inlineReply) void this.closeInline();
    this.folderId = id;
    this.searchFolder = null;
    this.selected = [];
    this.current = null;
    void this.load();
    // Outlook gibi: açılan klasör hemen eşitlenir (eşitleme kuyruğunun önüne alınır)
    if (app.setting("sync_on_select", "1") === "1") void api.setActiveFolder(id);
  }

  private isExchangeFolder(f: Folder | undefined): boolean {
    return !!f?.account_id && app.accounts.some((a) => a.id === f.account_id && a.kind === "exchange");
  }

  /** Liste sonunda daha eski iletiler getirilebilir mi */
  get canLoadOlder(): boolean {
    const f = this.folder();
    return !this.searchFolder && !app.search && !!f && this.isExchangeFolder(f) && !f.full_sync && !this.olderDone[f.id];
  }

  loadOlder() {
    const f = this.folder();
    if (!f || this.loadingOlder || !this.canLoadOlder) return;
    this.loadingOlder = true;
    void api.loadOlder(f.id).catch(() => (this.loadingOlder = false));
    // Yanıt gelmezse (bağlantı yok) göstergeyi kapat
    setTimeout(() => (this.loadingOlder = false), 30000);
  }

  onOlder(folder: number, more: boolean) {
    this.loadingOlder = false;
    if (!more) this.olderDone = { ...this.olderDone, [folder]: true };
    // Sunucudan gelen eski iletiler listenin sonuna eklenir
    if (folder === this.folderId) {
      this.localDone = false;
      void this.loadMore();
    }
  }

  /** Yerelde bulunanlara ek olarak Exchange sunucusunda da arar (eski iletiler dahil). */
  private async searchServer() {
    const q = app.search.trim();
    if (!q || this.searchFolder) return;
    const folderId = this.searchAll ? null : this.folderId;
    if (folderId != null && !this.isExchangeFolder(this.folder(folderId))) return;
    if (folderId == null && !app.accounts.some((a) => a.kind === "exchange")) return;
    const key = `${folderId}|${q}`;
    if (key === this.serverSearchKey) return;
    this.serverSearchKey = key;
    this.serverSearching = true;
    try {
      const added = await api.searchServer(folderId, q);
      if (added > 0 && app.search.trim() === q) await this.load();
    } catch {
      /* çevrimdışı: yerel sonuçlar yeterli */
    } finally {
      this.serverSearching = false;
    }
  }

  selectSearchFolder(key: string) {
    this.searchFolder = key;
    this.folderId = null;
    this.selected = [];
    this.current = null;
    void this.load();
  }

  /** Sunucudaki/veritabanındaki toplam satır (sayfalı liste) */
  total = $state(0);
  /** Yerel veritabanındaki tüm satırlar yüklendi */
  localDone = $state(false);
  loadingMore = $state(false);
  private refreshTimer: ReturnType<typeof setTimeout> | null = null;
  private refreshing = false;
  private refreshAgain = false;

  private order() {
    return { by: this.sortBy === "from" && this.sentLike ? "to" : this.sortBy, desc: this.sortDesc, conversations: this.conversations };
  }

  private query(offset: number, limit: number) {
    if (this.searchFolder) return api.listMessages(null, app.search, this.searchFolder, this.order(), offset, limit);
    if (this.searchAll && app.search) return api.listMessages(null, app.search, this.filter, this.order(), offset, limit);
    if (this.folderId != null) return api.listMessages(this.folderId, app.search, this.filter, this.order(), offset, limit);
    return Promise.resolve({ rows: [] as MessageRow[], total: 0 });
  }

  /**
   * Listeyi baştan yükler. `keep`: eşitleme sonrası yenileme — o ana kadar yüklenmiş
   * satır sayısı korunur (kaydırma konumu ve seçim bozulmaz).
   */
  async load(keep = false) {
    const seq = ++this.loadSeq;
    if (!keep) this.loading = true;
    try {
      const want = keep ? Math.min(Math.max(PAGE, this.messages.length), 5000) : PAGE;
      const r = await this.query(0, want);
      if (seq !== this.loadSeq) return;
      this.messages = keep ? this.reuse(r.rows) : r.rows;
      this.total = r.total;
      this.localDone = r.rows.length < want;
      if (app.search.trim()) void this.searchServer();
      else this.serverSearchKey = "";
      const ids = new Set(r.rows.map((x) => x.id));
      // Açık konuşmalar: klasör değişince kapanır, yenilemede (okundu, yeni yanıt) güncellenir
      if (!keep) this.expanded = {};
      else void this.refreshExpanded(ids);
      if (!keep) this.selected = this.selected.filter((id) => ids.has(id));
      if (this.current && !ids.has(this.current.id) && !this.current.external && !keep) {
        this.current = null;
        this.thread = [];
      }
    } finally {
      if (seq === this.loadSeq) this.loading = false;
    }
  }

  /** Yenilemede değişmeyen satırlar eski nesnesiyle kalır: listede yeniden biçimlenmez ve çizilmez */
  private reuse(rows: MessageRow[]): MessageRow[] {
    if (!this.messages.length) return rows;
    const old = new Map(this.messages.map((m) => [m.id, m]));
    return rows.map((r) => {
      const o = old.get(r.id);
      if (!o) return r;
      for (const k in r) if (r[k as keyof MessageRow] !== o[k as keyof MessageRow]) return r;
      return o;
    });
  }

  /** Eşitleme olayları için birleştirilmiş, gecikmeli yenileme (en fazla ~3/sn). */
  refresh() {
    if (this.refreshTimer) return;
    this.refreshTimer = setTimeout(async () => {
      this.refreshTimer = null;
      if (this.refreshing) {
        this.refreshAgain = true;
        return;
      }
      this.refreshing = true;
      try {
        await this.load(true);
      } finally {
        this.refreshing = false;
        if (this.refreshAgain) {
          this.refreshAgain = false;
          this.refresh();
        }
      }
    }, 300);
  }

  /** Liste sonuna gelindi: önce yerelden sonraki sayfa, yerel bitince sunucudan. */
  async loadMore() {
    if (this.loading || this.loadingMore) return;
    if (this.localDone) {
      this.loadOlder();
      return;
    }
    this.loadingMore = true;
    const seq = this.loadSeq;
    try {
      const r = await this.query(this.messages.length, PAGE);
      if (seq !== this.loadSeq) return;
      const have = new Set(this.messages.map((m) => m.id));
      const add = r.rows.filter((m) => !have.has(m.id));
      if (add.length) this.messages = [...this.messages, ...add];
      this.localDone = r.rows.length < PAGE;
    } finally {
      this.loadingMore = false;
    }
  }

  /** Veritabanında sıralanmış liste (sıralama/konuşma gruplaması sunucu tarafında) */
  get sorted(): MessageRow[] {
    return this.messages;
  }

  threadCount(m: MessageRow): number {
    return m.thread_count ?? 1;
  }

  /** Konuşma satırında, kendisi dışında (satırın altında gizli) okunmamış ileti sayısı */
  private hiddenUnread(m: MessageRow | undefined): number {
    return m && this.conversations ? Math.max(0, (m.thread_unread ?? 0) - (m.is_read ? 0 : 1)) : 0;
  }

  /** Liste satırı okunmamış mı görünür: konuşma satırı, içinde okunmamış ileti kaldıkça okunmamıştır */
  unread(m: MessageRow): boolean {
    return !m.is_read || this.hiddenUnread(m) > 0;
  }

  /** Konuşmayı listede aç/kapat (iletileri baş satırın altında) */
  async toggleExpand(head: number) {
    if (this.expanded[head]) {
      const { [head]: _, ...rest } = this.expanded;
      this.expanded = rest;
      return;
    }
    const rows = await api.conversation(head).catch(() => [] as MessageRow[]);
    if (rows.length > 1) this.expanded = { ...this.expanded, [head]: rows };
  }

  private async refreshExpanded(heads: Set<number>) {
    const open = Object.keys(this.expanded).map(Number);
    if (!open.length) return;
    const next: Record<number, MessageRow[]> = {};
    for (const h of open.filter((h) => heads.has(h))) {
      const rows = await api.conversation(h).catch(() => this.expanded[h] ?? []);
      if (rows.length > 1) next[h] = rows;
    }
    this.expanded = next;
  }

  // ------------------------------------------------------------- seçim
  /** child: listede açılmış konuşmanın alt satırı (yalnızca o ileti okundu olur) */
  async select(id: number, e?: MouseEvent | KeyboardEvent, child = false) {
    if (this.inlineReply && this.inlineReply.sourceId !== id) await this.closeInline();
    const list = this.sorted.map((m) => m.id);
    if (e && (e.ctrlKey || e.metaKey)) {
      this.selected = this.selected.includes(id) ? this.selected.filter((x) => x !== id) : [...this.selected, id];
      this.anchor = id;
    } else if (e && e.shiftKey && this.anchor != null) {
      const a = list.indexOf(this.anchor);
      const b = list.indexOf(id);
      const [s, en] = a < b ? [a, b] : [b, a];
      this.selected = list.slice(s, en + 1);
    } else {
      this.selected = [id];
      this.anchor = id;
    }
    if (this.selected.length === 1) await this.show(this.selected[0], !child);
  }

  selectAll() {
    this.selected = this.sorted.map((m) => m.id);
  }

  async show(id: number, whole = true) {
    if (this.readTimer) clearTimeout(this.readTimer);
    try {
      const v = await api.getMessage(id);
      if (this.selected.length !== 1 || this.selected[0] !== id) return;
      this.current = v;
      // Konuşma sorgusu yalnızca konuşmada birden çok ileti varsa (tüm tabloyu tarar)
      const row = this.messages.find((m) => m.id === id);
      this.thread = this.conversations && (row?.thread_count ?? 2) > 1 ? await api.conversation(id) : [];
      this.autoRead(v, () => this.selected.length === 1 && this.selected[0] === id, whole && this.hiddenUnread(row) > 0);
    } catch (e) {
      app.error(e);
    }
  }

  /** Okuma bölmesinde gösterilen ileti, ayardaki süre sonunda (hâlâ gösteriliyorsa) okundu olur */
  private autoRead(v: MessageView, still: () => boolean, whole = false) {
    if (this.readTimer) clearTimeout(this.readTimer);
    this.readTimer = null;
    // whole: konuşma satırı açıldı; kendisi okunmuş olsa da konuşmadaki okunmamışlar okundu olur
    if ((v.is_read && !whole) || v.external || app.setting("reading_pane", "right") === "off") return;
    const delay = Number(app.setting("mark_read_delay", "0"));
    if (delay < 0) return;
    this.readTimer = setTimeout(() => {
      if (still() && this.current?.id === v.id) void this.setRead([v.id], true, whole);
    }, delay * 1000);
  }

  /** Konuşmadaki başka bir iletiyi seçimi değiştirmeden göster (o ileti de okundu olur) */
  async showOnly(id: number) {
    try {
      const v = await api.getMessage(id);
      this.current = v;
      this.autoRead(v, () => true);
    } catch (e) {
      app.error(e);
    }
  }

  moveSelection(delta: number, extend = false) {
    const list = this.sorted;
    if (!list.length) return;
    const last = this.selected[this.selected.length - 1];
    let idx = list.findIndex((m) => m.id === last);
    idx = Math.max(0, Math.min(list.length - 1, idx < 0 ? 0 : idx + delta));
    const id = list[idx].id;
    if (extend) {
      if (!this.selected.includes(id)) this.selected = [...this.selected, id];
    } else {
      void this.select(id);
    }
  }

  /** Silme/taşıma sonrası sonraki iletiyi seç (Outlook davranışı) */
  private selectNextAfter(ids: number[]) {
    const list = this.sorted;
    const idxs = ids.map((id) => list.findIndex((m) => m.id === id)).filter((i) => i >= 0);
    if (!idxs.length) return;
    const after = list.slice(Math.max(...idxs) + 1).find((m) => !ids.includes(m.id));
    const before = list.slice(0, Math.min(...idxs)).reverse().find((m) => !ids.includes(m.id));
    const next = after ?? before;
    this.selected = [];
    this.current = null;
    if (next) void this.select(next.id);
  }

  // ------------------------------------------------------------- eylemler
  ids(): number[] {
    return this.selected.length ? [...this.selected] : this.current && !this.current.external ? [this.current.id] : [];
  }

  /** whole: konuşma satırı okundu yapılırken konuşmanın bu klasördeki bütün okunmamışları kapsanır */
  async setRead(ids: number[], value: boolean, whole = true) {
    const set = new Set(ids);
    if (value && whole) {
      for (const h of this.messages.filter((m) => set.has(m.id) && this.hiddenUnread(m) > 0)) {
        const rows = this.expanded[h.id] ?? (this.thread.some((x) => x.id === h.id) ? this.thread : await api.conversation(h.id).catch(() => [] as MessageRow[]));
        for (const r of rows) if (!r.is_read && (this.folderId == null || r.folder_id === h.folder_id)) set.add(r.id);
      }
      ids = [...set];
    }
    // Arayüz hemen güncellenir (Outlook gibi anında); sunucu isteği arkadan gider
    // Konuşma satırlarındaki okunmamış sayısı: durumu değişen (bilinen) iletiler kadar
    const changed = new Map<number, MessageRow>();
    for (const r of [...this.messages, ...this.thread, ...Object.values(this.expanded).flat()]) if (set.has(r.id) && r.is_read !== value) changed.set(r.id, r);
    const delta = (h: MessageRow) => [...changed.values()].filter((r) => r.thread_id === h.thread_id && (this.folderId == null || r.folder_id === h.folder_id)).length;
    const before = this.messages.filter((m) => set.has(m.id)).map((m) => [m.id, m.is_read] as const);
    // Elle "okunmadı" yapılan açık ileti, okundu zamanlayıcısıyla yeniden okundu olmasın
    if (!value && this.readTimer && this.current && set.has(this.current.id)) {
      clearTimeout(this.readTimer);
      this.readTimer = null;
    }
    const mark = (rows: MessageRow[]) => (rows.some((m) => set.has(m.id) && m.is_read !== value) ? rows.map((m) => (set.has(m.id) && m.is_read !== value ? { ...m, is_read: value } : m)) : rows);
    if (this.conversations && changed.size) {
      this.messages = this.messages.map((h) => {
        const n = h.thread_id ? delta(h) : 0;
        return n ? { ...h, thread_unread: Math.max(0, (h.thread_unread ?? 0) + (value ? -n : n)) } : h;
      });
    }
    this.messages = mark(this.messages);
    this.thread = mark(this.thread);
    if (Object.keys(this.expanded).length) this.expanded = Object.fromEntries(Object.entries(this.expanded).map(([k, rows]) => [k, mark(rows)]));
    if (this.current && set.has(this.current.id)) this.current = { ...this.current, is_read: value };
    try {
      await api.setRead(ids, value);
    } catch (e) {
      const old = new Map(before);
      this.messages = this.messages.map((m) => (old.has(m.id) ? { ...m, is_read: old.get(m.id)! } : m));
      app.error(e);
    }
  }

  async toggleRead() {
    const ids = this.ids();
    if (!ids.length) return;
    const first = this.messages.find((m) => m.id === ids[0]);
    await this.setRead(ids, first ? this.unread(first) : true);
  }

  async flag(value: boolean, due: number | null = null) {
    const ids = this.ids();
    if (!ids.length) return;
    await api.setFlagged(ids, value, due);
    this.messages = this.messages.map((m) => (ids.includes(m.id) ? { ...m, is_flagged: value } : m));
    app.bump("tasks");
  }

  /** Sabitle / Sabitlemeyi Kaldır (Outlook raptiyesi): sabitlenenler listenin en üstünde */
  async togglePin(id?: number) {
    const ids = id ? [id] : this.ids();
    const first = this.messages.find((m) => m.id === ids[0]);
    if (!first) return;
    const value = !first.is_pinned;
    this.messages = this.messages.map((m) => (ids.includes(m.id) ? { ...m, is_pinned: value } : m));
    await api.setPinned(ids, value).catch((e) => app.error(e));
    void this.load(true);
  }

  async toggleFlag(id?: number) {
    const ids = id ? [id] : this.ids();
    const first = this.messages.find((m) => m.id === ids[0]);
    if (!first) return;
    await api.setFlagged(ids, !first.is_flagged, null);
    this.messages = this.messages.map((m) => (ids.includes(m.id) ? { ...m, is_flagged: !first.is_flagged } : m));
    app.bump("tasks");
  }

  async delete(permanent = false) {
    const ids = this.ids();
    if (!ids.length) return;
    if (permanent && !(await app.confirm(t("mail.confirm_delete_permanent", ids.length), undefined, { ok: t("common.delete"), destructive: true })))
      return;
    // Onay penceresi hemen kapanır; liste anında güncellenir, silme arkada sürer
    if (permanent) await new Promise((r) => setTimeout(r, 0));
    this.selectNextAfter(ids);
    const set = new Set(ids);
    this.messages = this.messages.filter((m) => !set.has(m.id));
    void api.deleteMessages(ids, permanent).catch((e) => app.error(e));
  }

  async move(folderId: number, ids = this.ids(), copy = false) {
    if (!ids.length) return;
    if (copy) {
      await api.copyMessages(ids, folderId);
      toast.success(t("mail.copied", ids.length));
      return;
    }
    if (folderId === this.folderId) return;
    this.selectNextAfter(ids);
    this.messages = this.messages.filter((m) => !ids.includes(m.id));
    await api.moveMessages(ids, folderId);
  }

  async moveToRole(role: string) {
    const ids = this.ids();
    if (!ids.length) return;
    this.selectNextAfter(ids);
    this.messages = this.messages.filter((m) => !ids.includes(m.id));
    await api.moveToRole(ids, role);
  }

  async blockSender() {
    const ids = this.ids();
    if (!ids.length) return;
    try {
      const addr = await api.blockSender(ids);
      this.selectNextAfter(ids);
      toast.success(t("mail.blocked", addr));
    } catch (e) {
      app.error(e);
    }
  }

  async toggleCategory(name: string) {
    const ids = this.ids();
    if (!ids.length) return;
    await api.toggleCategory(ids, name);
    await this.load(true);
    if (this.current) this.current = await api.getMessage(this.current.id);
  }

  async setImportance(v: number) {
    const ids = this.ids();
    if (!ids.length) return;
    await api.setImportance(ids, v);
    this.messages = this.messages.map((m) => (ids.includes(m.id) ? { ...m, importance: v } : m));
  }

  /** Okuma bölmesinde açık satır içi yanıt (Outlook) */
  inlineReply = $state<ComposeRequest | null>(null);
  /** Satır içi yanıt açıkken başka işlem yapılırsa önce taslağı kaydedip kapatır */
  inlineGuard: (() => Promise<void>) | null = null;

  async closeInline() {
    if (this.inlineGuard) await this.inlineGuard();
    this.inlineReply = null;
  }

  reply(mode: "reply" | "reply_all" | "forward", id?: number) {
    const src = id ?? this.current?.id ?? this.selected[0];
    if (src == null) return;
    // Outlook gibi: okuma bölmesi açıksa yanıt bölmenin içinde açılır
    if (IS_MAIN && this.readingPane !== "off" && app.setting("reply_inline", "1") === "1" && this.current?.id === src) {
      void this.closeInline().then(() => (this.inlineReply = { mode, sourceId: src, key: Date.now() }));
      return;
    }
    app.compose({ mode, sourceId: src });
  }

  async forwardAsAttachment() {
    const ids = this.ids();
    if (!ids.length) return;
    const first = this.messages.find((m) => m.id === ids[0]);
    // Outlook gibi: iletiler .msg dosyası olarak eklenir
    const atts = [];
    for (const id of ids) {
      const f = await api.messageTempFile(id, "msg");
      atts.push({ name: f.name, mime: f.mime, size: f.size, path: f.path });
    }
    app.compose({ mode: "new", data: { subject: `${t("compose.fw_prefix")}: ${first?.subject ?? ""}` }, attachments: atts });
  }

  open(id: number) {
    const m = this.messages.find((x) => x.id === id);
    const f = this.folder(m?.folder_id ?? null);
    // Gönderilmiş Öğeler'deki ileti taslak bayrağı taşısa bile okuma penceresinde açılır
    if (m && ((m.is_draft && f?.role !== "sent" && f?.role !== "trash") || f?.role === "drafts")) {
      app.compose({ mode: "draft", sourceId: id });
      return;
    }
    if (m && f?.role === "outbox") {
      app.compose({ mode: "edit", sourceId: id });
      return;
    }
    // Okuma bölmesinde zaten yüklü ileti pencereye hazır verilir: pencere anında dolu açılır
    app.openMessage(id, m?.subject, this.current?.id === id ? $state.snapshot(this.current) : undefined);
  }
}

export const mail = new MailState();
