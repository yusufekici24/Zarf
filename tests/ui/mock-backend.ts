// Arayüz testleri için sahte Tauri arka ucu: ön yüz gerçek arka uç yerine bunu çağırır.
// Sayfa yüklenmeden önce (page.addInitScript) kurulur; Rust tarafı çalışmaz, veriler aşağıdaki
// örnek posta kutusundan gelir. Testler çağrıları `window.__mock.calls` ile denetler.
import type { Page } from "@playwright/test";
import tr from "../../locales/tr.json" with { type: "json" };

export interface MockMessage {
  id: number;
  folder_id: number;
  subject: string;
  sender: string;
  date: number;
  snippet: string;
  is_read: boolean;
  has_attachments?: boolean;
  html: string;
}

export interface Fixtures {
  window: string;
  locales: Record<string, string>;
  settings: Record<string, string>;
  messages: MockMessage[];
  storage: { size: number; free: number; incremental: boolean };
}

const NOW = Math.floor(Date.UTC(2026, 10, 5, 9, 0) / 1000);

export const SAMPLE_MESSAGES: MockMessage[] = [
  {
    id: 1,
    folder_id: 1,
    subject: "Bütçe toplantısı",
    sender: "Ayşe Yılmaz <ayse.yilmaz@kurum.com.tr>",
    date: NOW - 600,
    snippet: "Yarınki bütçe toplantısı saat 10:00'da",
    is_read: false,
    html: "<p>Merhaba,</p><p>Yarınki <b>bütçe toplantısı</b> saat 10:00'da.</p>",
  },
  {
    id: 2,
    folder_id: 1,
    subject: "Aylık rapor",
    sender: "Mehmet Kaya <mehmet.kaya@kurum.com.tr>",
    date: NOW - 7200,
    snippet: "Ekte ekim ayı raporu",
    is_read: true,
    has_attachments: true,
    html: "<p>Ekte ekim ayı raporu var.</p>",
  },
  {
    id: 3,
    folder_id: 2,
    subject: "RE: Proje takvimi",
    sender: "Ben <ben@kurum.com.tr>",
    date: NOW - 86400,
    snippet: "Takvim uygun",
    is_read: true,
    html: "<p>Takvim uygun.</p>",
  },
];

export function fixtures(over: Partial<Fixtures> = {}): Fixtures {
  return {
    window: "main",
    locales: tr as Record<string, string>,
    settings: { language: "tr", start_module: "mail", reading_pane: "right", sync_on_start: "0" },
    messages: SAMPLE_MESSAGES,
    storage: { size: 1_520_000_000, free: 210_000_000, incremental: false },
    ...over,
  };
}

export async function installMock(page: Page, f: Fixtures) {
  await page.addInitScript((f: Fixtures) => {
    type Args = Record<string, unknown>;
    const calls: { cmd: string; args: Args }[] = [];
    const unknown = new Set<string>();
    const callbacks = new Map<number, (data: unknown) => void>();
    const listeners = new Map<string, number[]>();
    let nextId = 1;
    const account = {
      id: 1, name: "Kurum", email: "ben@kurum.com.tr", display_name: "Ben", imap_host: "imap.kurum.com.tr", imap_port: 993,
      imap_security: "ssl", smtp_host: "smtp.kurum.com.tr", smtp_port: 465, smtp_security: "ssl", username: "ben", signature: "",
      sync_limit: 500, auto_reply_enabled: false, auto_reply_text: "", kind: "imap", auth_method: "password", ews_url: "",
      auth_state: "ok", auth_error: "", caldav_url: "", carddav_url: "", shared_of: null,
    };
    const folder = (id: number, name: string, role: string, position: number) => {
      const own = f.messages.filter((m) => m.folder_id === id);
      return {
        id, account_id: 1, name, remote_name: name, role, parent_id: null, uidvalidity: 1, position, favorite: role === "inbox",
        full_sync: true, display: name, unread: own.filter((m) => !m.is_read).length, total: own.length,
      };
    };
    const folders = [folder(1, "Gelen Kutusu", "inbox", 0), folder(2, "Gönderilmiş Öğeler", "sent", 1), folder(3, "Silinmiş Öğeler", "trash", 2)];
    const row = (m: (typeof f.messages)[number]) => ({
      id: m.id, folder_id: m.folder_id, account_id: 1, uid: m.id, subject: m.subject, sender: m.sender, to_addr: "Ben <ben@kurum.com.tr>",
      date: m.date, snippet: m.snippet, is_read: m.is_read, is_flagged: false, is_draft: false, is_answered: false, importance: 1,
      categories: "", has_attachments: !!m.has_attachments, message_id: `<m${m.id}@kurum.com.tr>`, in_reply_to: "", thread_count: 1, is_pinned: false,
    });
    const fold = (s: string) => s.toLocaleLowerCase("tr").normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/ı/g, "i");
    const handlers: Record<string, (a: Args) => unknown> = {
      get_locales: () => ({ current: "tr", langs: { tr: f.locales } }),
      set_language: () => "tr",
      get_settings: () => f.settings,
      set_setting: (a) => {
        f.settings[a.key as string] = a.value as string;
        return null;
      },
      startup_args: () => [],
      default_app_status: () => true,
      list_recovery: () => [],
      list_categories: () => [],
      net_state: () => "online",
      app_info: () => ({ version: "1.3.0", data_dir: "/home/test/.local/share/lynora", readpst: false }),
      list_accounts: () => [account],
      folder_tree: () => ({
        groups: [{ account, folders }],
        favorites: folders.filter((x) => x.favorite),
        outbox: null,
        multi: false,
        unread_total: f.messages.filter((m) => !m.is_read && m.folder_id === 1).length,
      }),
      folder_counts: () => Object.fromEntries(folders.map((x) => [x.id, [x.total, x.unread]])),
      list_messages: (a) => {
        const q = fold(String(a.search ?? "").trim());
        const rows = f.messages
          .filter((m) => a.folderId == null || m.folder_id === a.folderId)
          .filter((m) => !q || q.split(/\s+/).every((w) => fold(`${m.subject} ${m.sender} ${m.snippet}`).includes(w)))
          .sort((x, y) => y.date - x.date)
          .map(row);
        return { rows, total: rows.length };
      },
      get_message: (a) => {
        const m = f.messages.find((x) => x.id === a.id)!;
        return {
          ...row(m), cc: "", bcc: "", refs: "", body_text: m.snippet, body_html: m.html, pending: "", html: m.html, text: m.snippet,
          attachments: [], reply_to: "", read_receipt_to: "", has_remote: false, has_calendar: false, has_vcard: false,
          folder_role: folders.find((x) => x.id === m.folder_id)?.role ?? "", external: false, security: "",
        };
      },
      storage_info: () => f.storage,
      compact_database: () => {
        const before = f.storage.size;
        f.storage = { size: before - f.storage.free, free: 0, incremental: true };
        return { before, after: f.storage.size };
      },
      "plugin:event|listen": (a) => {
        const list = listeners.get(a.event as string) ?? [];
        list.push(a.handler as number);
        listeners.set(a.event as string, list);
        return a.handler;
      },
      "plugin:window|scale_factor": () => 1,
      "plugin:window|inner_size": () => ({ width: 1400, height: 900 }),
      "plugin:window|outer_size": () => ({ width: 1400, height: 900 }),
      "plugin:window|outer_position": () => ({ x: 0, y: 0 }),
      "plugin:window|inner_position": () => ({ x: 0, y: 0 }),
      "plugin:window|is_maximized": () => false,
      "plugin:window|is_fullscreen": () => false,
      "plugin:window|is_focused": () => true,
      "plugin:window|is_visible": () => true,
      "plugin:window|theme": () => "light",
      "plugin:window|current_monitor": () => ({ name: "test", size: { width: 1920, height: 1080 }, position: { x: 0, y: 0 }, scaleFactor: 1, workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 } } }),
      "plugin:window|primary_monitor": () => ({ name: "test", size: { width: 1920, height: 1080 }, position: { x: 0, y: 0 }, scaleFactor: 1, workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 } } }),
    };
    const w = window as unknown as Record<string, unknown>;
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: f.window }, currentWebview: { windowLabel: f.window, label: f.window } },
      plugins: { path: { sep: "/", delimiter: ":" } },
      transformCallback(cb: (data: unknown) => void) {
        const id = nextId++;
        callbacks.set(id, cb);
        return id;
      },
      unregisterCallback(id: number) {
        callbacks.delete(id);
      },
      convertFileSrc: (p: string, protocol = "asset") => `${protocol}://localhost/${encodeURIComponent(p)}`,
      async invoke(cmd: string, args: Args = {}) {
        calls.push({ cmd, args });
        const h = handlers[cmd];
        if (h) return h(args);
        unknown.add(cmd);
        return null;
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_e: string, id: number) => callbacks.delete(id) };
    w.__mock = {
      calls,
      unknown,
      /** Ön yüz bu olayı dinliyor mu */
      listening: (event: string) => (listeners.get(event) ?? []).length > 0,
      /** Arka uçtan gelen olay (ör. ayarlar penceresine içerik) */
      emit(event: string, payload: unknown) {
        for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
      },
    };
  }, f);
}

/** Sayfadaki sahte arka uca yapılan çağrılar */
export async function calls(page: Page, cmd: string): Promise<Record<string, unknown>[]> {
  return page.evaluate((cmd) => (window as unknown as { __mock: { calls: { cmd: string; args: Record<string, unknown> }[] } }).__mock.calls.filter((c) => c.cmd === cmd).map((c) => c.args), cmd);
}
