// Rust komutlarına ince, tipli sarmalayıcılar.
import { invoke } from "@tauri-apps/api/core";
import type {
  Account, CalEvent, Calendar, Category, ComposeData, Contact, ContactGroup, Discovery, Folder, FolderGroup,
  MessageRow, MessageView, Note, Occurrence, Reminder, Rule, RuleReport, RuleRun, Task, RuleSpec, ListOrder,
} from "./types";

export type BackupInfo = { app_version: string; created: string; accounts: string[]; counts: Record<string, number> };
export type MeetingOcc = { title: string; start: number; end: number; show_as: string; all_day: boolean; location: string };

export const api = {
  // dil / ayarlar
  getLocales: () => invoke<{ current: string; langs: Record<string, Record<string, string>> }>("get_locales"),
  setLanguage: (code: string) => invoke<string>("set_language", { code }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  startupArgs: () => invoke<string[]>("startup_args"),
  appInfo: () => invoke<{ version: string; data_dir: string; readpst: boolean }>("app_info"),

  // hesaplar
  listAccounts: () => invoke<Account[]>("list_accounts"),
  guessServers: (email: string) => invoke<Record<string, string | number>>("guess_servers", { email }),
  saveAccount: (account: Account, password: string | null, oauthToken: string | null = null) =>
    invoke<number>("save_account", { account, password, oauthToken }),
  retryAccount: (id: number) => invoke<void>("retry_account", { id }),
  systemIdentity: () =>
    invoke<{ username: string; full_name: string; principal: string; realm: string; domain: string; email_guess: string; kerberos: boolean; email_source: string; login: string }>("system_identity"),
  discoverAccount: (email: string, user: string, password: string) => invoke<Discovery>("discover_account", { email, username: user, password }),
  oauthAuthorize: (provider: string, exchange: boolean, email: string) => invoke<string>("oauth_authorize", { provider, exchange, email }),
  addSharedMailbox: (baseId: number, email: string, name: string) => invoke<number>("add_shared_mailbox", { baseId, email, name }),
  setAutoReply: (accountId: number, enabled: boolean, text: string) => invoke<void>("set_auto_reply", { accountId, enabled, text }),
  respondMeeting: (msgId: number, kind: "accept" | "tentative" | "decline", comment = "", send = true) => invoke<void>("respond_meeting", { msgId, kind, comment, send }),
  meetingInfo: (msgId: number) =>
    invoke<{
      title: string; start: number; end: number; all_day: boolean; location: string; organizer: string;
      attendees: string; optional: string; resources: string; recurrence: string; method: string; cancelled: boolean;
      response: string; event_id: number | null;
      conflicts: MeetingOcc[]; adjacent: MeetingOcc[]; day: MeetingOcc[];
    }>("meeting_info", { msgId }),
  suggestRecipients: (query: string, includeGal = true) =>
    invoke<{ name: string; email: string; source: string }[]>("suggest_recipients", { query, includeGal }),
  forgetRecipient: (email: string) => invoke<void>("forget_recipient", { email }),
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
  setFolderFavorite: (id: number, value: boolean) => invoke<void>("set_folder_favorite", { id, value }),
  deleteAccount: (id: number) => invoke<void>("delete_account", { id }),
  testAccount: (account: Account, password: string | null) =>
    invoke<{ imap_ok: boolean; smtp_ok: boolean; log: string }>("test_account", { account, password }),

  // klasörler
  folderTree: () =>
    invoke<{ groups: FolderGroup[]; favorites: Folder[]; outbox: Folder | null; multi: boolean; unread_total: number }>("folder_tree"),
  createFolder: (accountId: number | null, name: string, parentId: number | null) =>
    invoke<void>("create_folder", { accountId, name, parentId }),
  renameFolder: (id: number, name: string) => invoke<void>("rename_folder", { id, name }),
  deleteFolder: (id: number) => invoke<void>("delete_folder", { id }),
  emptyFolder: (id: number) => invoke<void>("empty_folder", { id }),
  markFolderRead: (id: number) => invoke<void>("mark_folder_read", { id }),
  syncFolder: (id: number) => invoke<void>("sync_folder", { id }),
  setActiveFolder: (id: number) => invoke<void>("set_active_folder", { id }),
  loadOlder: (folderId: number) => invoke<void>("load_older", { folderId }),
  searchServer: (folderId: number | null, query: string) => invoke<number>("search_server", { folderId, query }),
  syncAll: () => invoke<void>("sync_all"),
  syncAccount: (id: number) => invoke<void>("sync_account", { id }),

  // iletiler
  defaultAppStatus: () => invoke<boolean>("default_app_status"),
  setDefaultApp: () => invoke<void>("set_default_app"),
  /** Gizli oluşturulan pencereyi ilk çizimden sonra gösterir */
  revealWindow: () => invoke<void>("reveal_window"),
  /** Çevrimdışı adres defterini şimdi indirir (Dosya menüsü); döner: kayıt sayısı */
  refreshAddressBook: () => invoke<number>("refresh_address_book"),
  addressBookStatus: () => invoke<{ synced: number; count: number; busy: boolean }>("address_book_status"),
  /** Gösterilen pencerenin ilk karesi çizildi (saydam açılış biter) */
  windowPainted: () => invoke<void>("window_painted"),
  mainHidden: () => invoke<boolean>("main_hidden"),
  /** Bağlantıyı varsayılan uygulamayla açar (web, e-posta, telefon) */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  /** Eki varsayılan uygulamayla açar (çalıştırılabilir türler açılmaz) */
  openAttachment: (path: string) => invoke<void>("open_attachment", { path }),
  notify: (title: string, body: string, sound: boolean) => invoke<void>("desktop_notify", { title, body, sound }),
  /** Bir Lynora penceresi ekranda görünüyor mu (yeni posta uyarısı yalnızca görünmüyorken) */
  appVisible: () => invoke<boolean>("app_visible"),
  siblingIds: (folderId: number) => invoke<number[]>("sibling_ids", { folderId }),
  listMessages: (folderId: number | null, search: string, filter: string, order: ListOrder, offset = 0, limit = 500) =>
    invoke<{ rows: MessageRow[]; total: number }>("list_messages", { folderId, search, filter, order, offset, limit }),
  getMessage: (id: number) => invoke<MessageView>("get_message", { id }),
  conversation: (id: number) => invoke<MessageRow[]>("conversation", { id }),
  setRead: (ids: number[], value: boolean) => invoke<void>("set_read", { ids, value }),
  searchSuggest: (query: string) =>
    invoke<{ people: { name: string; email: string; count: number }[]; attachments: { id: number; name: string; subject: string; sender: string; date: number }[] }>("search_suggest", { query }),
  setPinned: (ids: number[], value: boolean) => invoke<void>("set_pinned", { ids, value }),
  setFlagged: (ids: number[], value: boolean, due: number | null = null) => invoke<void>("set_flagged", { ids, value, due }),
  setCategories: (ids: number[], categories: string[]) => invoke<void>("set_categories", { ids, categories }),
  toggleCategory: (ids: number[], category: string) => invoke<void>("toggle_category", { ids, category }),
  setImportance: (ids: number[], importance: number) => invoke<void>("set_importance", { ids, importance }),
  moveMessages: (ids: number[], folderId: number) => invoke<void>("move_messages", { ids, folderId }),
  copyMessages: (ids: number[], folderId: number) => invoke<void>("copy_messages", { ids, folderId }),
  moveToRole: (ids: number[], role: string) => invoke<void>("move_to_role", { ids, role }),
  deleteMessages: (ids: number[], permanent = false) => invoke<void>("delete_messages", { ids, permanent }),
  blockSender: (ids: number[]) => invoke<string>("block_sender", { ids }),
  saveAttachment: (msgId: number, index: number, path: string) => invoke<void>("save_attachment", { msgId, index, path }),
  saveAllAttachments: (msgId: number, dir: string) => invoke<number>("save_all_attachments", { msgId, dir }),
  attachmentTempPath: (msgId: number, index: number) => invoke<string>("attachment_temp_path", { msgId, index }),
  openAttachmentAsMessage: (msgId: number, index: number) => invoke<number>("open_attachment_as_message", { msgId, index }),
  importAttachmentCalendar: (msgId: number) => invoke<number>("import_attachment_calendar", { msgId }),
  importAttachmentVcard: (msgId: number) => invoke<number>("import_attachment_vcard", { msgId }),
  messageTempFile: (id: number, format: "msg" | "eml" = "msg") =>
    invoke<{ path: string; name: string; mime: string; size: number }>("message_temp_file", { id, format }),
  saveMessageAs: (id: number, path: string) => invoke<void>("save_message_as", { id, path }),
  importExternalMessage: (id: number, folderId: number | null = null) => invoke<number>("import_external_message", { id, folderId }),
  sendReadReceipt: (id: number) => invoke<void>("send_read_receipt", { id }),

  // yazma
  composePrefill: (mode: string, sourceId: number) => invoke<ComposeData>("compose_prefill", { mode, sourceId }),
  sendMail: (mail: unknown) => invoke<number>("send_mail", { mail }),
  saveDraft: (mail: unknown) => invoke<number>("save_draft", { mail }),
  discardDraft: (id: number) => invoke<void>("discard_draft", { id }),
  processOutbox: () => invoke<void>("process_outbox"),
  cancelOutbox: (id: number) => invoke<number>("cancel_outbox", { id }),
  retryOutbox: (id: number) => invoke<void>("retry_outbox", { id }),
  addressBook: () => invoke<[string, string][]>("address_book"),
  createBackup: (path: string, password: string) => invoke<BackupInfo>("create_backup", { path, password }),
  backupIsEncrypted: (path: string) => invoke<boolean>("backup_is_encrypted", { path }),
  prepareRestore: (path: string, password: string) => invoke<BackupInfo>("prepare_restore", { path, password }),
  cancelRestore: () => invoke<void>("cancel_restore"),
  finishRestore: () => invoke<void>("finish_restore"),
  saveRecovery: (id: string, payload: string) => invoke<void>("save_recovery", { id, payload }),
  listRecovery: () => invoke<string[]>("list_recovery"),
  removeRecovery: (id: string) => invoke<void>("remove_recovery", { id }),

  // dosyalar
  openFile: (path: string) => invoke<{ kind: string; id?: number; count?: number; appointment?: boolean }>("open_file", { path }),
  importMailFiles: (folderId: number, paths: string[]) => invoke<number>("import_mail_files", { folderId, paths }),
  exportMbox: (folderId: number, path: string) => invoke<number>("export_mbox", { folderId, path }),
  importPst: (path: string) => invoke<{ messages: number; contacts: number; events: number }>("import_pst", { path }),
  writeTempFile: (name: string, data: string) => invoke<string>("write_temp_file", { name, data }),
  imageDataUrl: (path: string) => invoke<string>("image_data_url", { path }),
  fileInfo: (path: string) => invoke<{ name: string; size: number; mime: string }>("file_info", { path }),

  // kişiler
  listContacts: (search = "") => invoke<Contact[]>("list_contacts", { search }),
  getContact: (id: number) => invoke<Contact | null>("get_contact", { id }),
  contactByEmail: (email: string) => invoke<Contact | null>("contact_by_email", { email }),
  saveContact: (contact: Contact) => invoke<number>("save_contact", { contact }),
  deleteContacts: (ids: number[]) => invoke<void>("delete_contacts", { ids }),
  listGroups: () => invoke<ContactGroup[]>("list_groups"),
  saveGroup: (group: ContactGroup) => invoke<number>("save_group", { group }),
  deleteGroup: (id: number) => invoke<void>("delete_group", { id }),
  importContacts: (path: string) => invoke<number>("import_contacts", { path }),
  exportContacts: (path: string, ids: number[] | null = null) => invoke<number>("export_contacts", { path, ids }),

  // takvim
  listCalendars: () => invoke<Calendar[]>("list_calendars"),
  saveCalendar: (calendar: Calendar) => invoke<number>("save_calendar", { calendar }),
  deleteCalendar: (id: number) => invoke<void>("delete_calendar", { id }),
  listOccurrences: (start: number, end: number) => invoke<Occurrence[]>("list_occurrences", { start, end }),
  searchEvents: (query: string) => invoke<CalEvent[]>("search_events", { query }),
  getEvent: (id: number) => invoke<CalEvent | null>("get_event", { id }),
  saveEvent: (event: CalEvent) => invoke<number>("save_event", { event }),
  deleteEvent: (id: number, mode: "one" | "series", occurrenceStart: number | null = null) =>
    invoke<void>("delete_event", { id, mode, occurrenceStart }),
  detachOccurrence: (id: number, occurrenceStart: number) => invoke<number>("detach_occurrence", { id, occurrenceStart }),
  importIcs: (path: string) => invoke<number>("import_ics", { path }),
  exportIcs: (path: string) => invoke<number>("export_ics", { path }),
  showMailAlerts: (items: unknown[]) => invoke<void>("show_mail_alerts", { items }),
  netState: () => invoke<"online" | "disconnected" | "work_offline">("net_state"),
  directoryPage: (source: string, query: string, offset: number) =>
    invoke<{ rows: { name: string; email: string; title: string; department: string; office: string; phone: string; kind: string }[]; more: boolean }>("directory_page", { source, query, offset }),
  addInternetCalendar: (url: string, name: string, grp: string) => invoke<number>("add_internet_calendar", { url, name, grp }),
  refreshInternetCalendars: (force = false) => invoke<number>("refresh_internet_calendars", { force }),
  remoteOccurrences: (calendarId: number, start: number, end: number) => invoke<Occurrence[]>("remote_occurrences", { calendarId, start, end }),
  refreshTeamCalendars: () => invoke<string>("refresh_team_calendars"),
  calendarIcsTemp: (start: number, end: number, name: string) =>
    invoke<{ name: string; mime: string; size: number; path: string }>("calendar_ics_temp", { start, end, name }),
  exportEvent: (id: number, path: string) => invoke<void>("export_event", { id, path }),
  listRooms: (query?: string) => invoke<{ list: string; name: string; email: string; office?: string }[]>("list_rooms", { query: query ?? null }),
  freeBusy: (emails: string[], start: number, end: number, interval = 30) =>
    invoke<Record<string, string>>("free_busy", { emails, start, end, interval }),
  mailboxUsage: () => invoke<{ used: number; quota: number; email: string } | null>("mailbox_usage"),
  freeBusyDetails: (emails: string[], start: number, end: number, interval = 30) =>
    invoke<Record<string, { merged: string; events: { start: number; end: number; show_as: string; title: string; location: string }[] }>>("free_busy_details", { emails, start, end, interval }),
  sendInvitation: (eventId: number, accountId: number, cancel = false) =>
    invoke<void>("send_invitation", { eventId, accountId, cancel }),

  // görevler
  listTasks: () => invoke<Task[]>("list_tasks"),
  getTask: (id: number) => invoke<Task | null>("get_task", { id }),
  saveTask: (task: Task) => invoke<number>("save_task", { task }),
  deleteTasks: (ids: number[]) => invoke<void>("delete_tasks", { ids }),
  exportTasks: (path: string) => invoke<number>("export_tasks", { path }),

  // notlar
  listNotes: () => invoke<Note[]>("list_notes"),
  saveNote: (note: Note) => invoke<number>("save_note", { note }),
  deleteNote: (id: number) => invoke<void>("delete_note", { id }),

  // kurallar / kategoriler
  listRules: () => invoke<Rule[]>("list_rules"),
  saveRule: (rule: Rule, force = false) => invoke<number>("save_rule", { rule, force }),
  deleteRule: (id: number, force = false) => invoke<void>("delete_rule", { id, force }),
  refreshServerRules: () => invoke<void>("refresh_server_rules"),
  serverRuleSpec: (id: number) => invoke<RuleSpec>("server_rule_spec", { id }),
  saveServerRule: (id: number, accountId: number, name: string, enabled: boolean, spec: RuleSpec, force = false) =>
    invoke<void>("save_server_rule", { id, accountId, name, enabled, spec, force }),
  previewRules: (run: RuleRun) => invoke<RuleReport>("preview_rules", { run }),
  runRules: (run: RuleRun) => invoke<RuleReport>("run_rules", { run }),
  cancelRuleRun: () => invoke<void>("cancel_rule_run"),
  listCategories: () => invoke<Category[]>("list_categories"),
  saveCategory: (oldName: string | null, name: string, color: string) => invoke<void>("save_category", { oldName, name, color }),
  deleteCategory: (name: string) => invoke<void>("delete_category", { name }),

  // hatırlatıcılar
  getReminders: () => invoke<Reminder[]>("get_reminders"),
  openWindow: (kind: "compose" | "read" | "settings" | "account" | "event", payload: unknown, title?: string) => invoke<string>("open_window", { kind, payload, title }),
  windowReady: () => invoke<{ kind: string; payload: unknown } | null>("window_ready"),
  focusMain: () => invoke<void>("focus_main"),
  /** İleti/yeni ileti penceresini kapatır (gizlenip sonraki açılış için hazır bekler) */
  recycleWindow: () => invoke<void>("recycle_window"),
  dragMessages: (x: number, y: number, ids: number[], phase: "start" | "move" | "drop" | "cancel") =>
    invoke<string | null>("drag_messages", { x, y, ids, phase }),
  systemFonts: () => invoke<string[]>("system_fonts"),
  groupAddresses: (emails: string[]) => invoke<string[]>("group_addresses", { emails }),
  expandGroup: (email: string) => invoke<string[]>("expand_group", { email }),
  contactCard: (email: string) =>
    invoke<{ name: string; email: string; kind: string; job_title: string; department: string; company: string; office: string; manager: string; phones: [string, string][]; members: string[] }>("contact_card", { email }),
  displayNames: (emails: string[]) => invoke<Record<string, string>>("display_names", { emails }),
  setUnreadBadge: (count: number) => invoke<void>("set_unread_badge", { count }),
  dismissReminder: (key: string, snoozeMinutes: number | null = null, until: number | null = null) => invoke<void>("dismiss_reminder", { key, snoozeMinutes, until }),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
