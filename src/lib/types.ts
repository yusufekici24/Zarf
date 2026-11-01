// Rust arka ucundaki veri modellerinin TypeScript karşılıkları.

export interface Account {
  id: number;
  /** Paylaşılan posta kutusu: kimliği kullanılan ana hesap */
  shared_of?: number | null;
  name: string;
  email: string;
  display_name: string;
  imap_host: string;
  imap_port: number;
  imap_security: string;
  smtp_host: string;
  smtp_port: number;
  smtp_security: string;
  username: string;
  signature: string;
  sync_limit: number;
  auto_reply_enabled: boolean;
  auto_reply_text: string;
  /** "imap" | "exchange" */
  kind: string;
  /** "password" | "kerberos" | "ntlm" | "oauth" */
  auth_method: string;
  ews_url: string;
  /** "ok" | "failed" (kimlik hatasında duraklatıldı) */
  auth_state: string;
  auth_error: string;
}

export interface Discovery {
  kind: string;
  email: string;
  display_name: string;
  ews_url: string;
  office365: boolean;
  imap_host: string;
  imap_port: number;
  imap_security: string;
  smtp_host: string;
  smtp_port: number;
  smtp_security: string;
  auth_method: string;
  username: string;
  source: string;
  auth_failed: boolean;
  need_email: boolean;
  need_credentials: boolean;
  log: string[];
}

export interface Folder {
  id: number;
  account_id: number | null;
  name: string;
  remote_name: string | null;
  role: string | null;
  parent_id: number | null;
  uidvalidity: number;
  position: number;
  favorite: boolean;
  /** Sunucudaki tüm başlıklar alındı */
  full_sync: boolean;
  display: string;
  unread: number;
  total: number;
}

export interface FolderGroup {
  account: Account | null;
  folders: Folder[];
}

export interface MessageRow {
  id: number;
  folder_id: number;
  account_id: number | null;
  uid: number | null;
  subject: string;
  sender: string;
  to_addr: string;
  date: number;
  snippet: string;
  is_read: boolean;
  is_flagged: boolean;
  is_draft: boolean;
  is_answered: boolean;
  importance: number;
  categories: string;
  has_attachments: boolean;
  message_id: string;
  in_reply_to: string;
  thread_count?: number;
  is_pinned?: boolean;
}

export interface Attachment {
  index: number;
  name: string;
  mime: string;
  size: number;
  cid: string;
  inline: boolean;
}

export interface MessageView extends MessageRow {
  cc: string;
  bcc: string;
  refs: string;
  body_text: string;
  body_html: string;
  pending: string;
  html: string;
  text: string;
  attachments: Attachment[];
  reply_to: string;
  read_receipt_to: string;
  has_remote: boolean;
  has_calendar: boolean;
  has_vcard: boolean;
  folder_role: string;
  external: boolean;
}

export interface OutAttachment {
  name: string;
  mime: string;
  size?: number;
  path?: string | null;
  source_msg?: number | null;
  source_index?: number | null;
}

export interface ComposeData {
  account_id: number | null;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  html: string;
  in_reply_to: string;
  references: string;
  attachments: OutAttachment[];
  draft_id: number | null;
  importance: number;
  /** Taslakta seçili "Kimden" adresi (takma ad) */
  from_addr?: string;
  /** Yanıtlanan iletinin alıcıları: hangi takma ada geldiği buradan anlaşılır */
  recipients?: string;
}

export interface Contact {
  id: number;
  first_name: string;
  last_name: string;
  display_name: string;
  email: string;
  email2: string;
  phone_mobile: string;
  phone_work: string;
  phone_home: string;
  company: string;
  job_title: string;
  department: string;
  address: string;
  website: string;
  birthday: string;
  notes: string;
  categories: string;
  is_favorite: boolean;
  /** Exchange'den eşitlenen kişi (salt okunur) */
  account_id?: number | null;
  remote_id?: string | null;
}

export interface ContactGroup {
  id: number;
  name: string;
  members: string;
  /** Exchange ile eşitlenen grubun hesabı */
  account_id?: number | null;
}

export interface Calendar {
  id: number;
  name: string;
  color: string;
  visible: boolean;
  account_id?: number | null;
  /** Başkasının takvimi: posta kutusu adresi */
  owner?: string;
  /** "" Takvimlerim | "team" | "other" | "shared" */
  grp?: string;
  /** "" | "remote" (salt okunur) | "birthdays" */
  kind?: string;
}

export interface CalEvent {
  id: number;
  calendar_id: number;
  uid: string;
  title: string;
  location: string;
  description: string;
  start: number;
  end: number;
  all_day: boolean;
  recurrence: string;
  recur_interval: number;
  recur_until: number | null;
  exdates: string;
  reminder_minutes: number;
  category: string;
  show_as: string;
  attendees: string;
  optional_attendees?: string;
  resources?: string;
  is_private: boolean;
  /** Exchange yinelenen serinin ana öğesi (yinelenen toplantı) */
  master_id?: string | null;
  /** Exchange öğe kimliği (başkasının takviminde de gelir) */
  remote_id?: string | null;
  /** Düzenleyen ("Ad <adres>") */
  organizer?: string;
  is_cancelled?: boolean;
  is_meeting?: boolean;
  /** "organizer" | "accept" | "tentative" | "decline" | "none" | "" */
  response?: string;
}

export interface Occurrence {
  event: CalEvent;
  start: number;
  end: number;
}

export interface Task {
  id: number;
  title: string;
  notes: string;
  start_date: number | null;
  due_date: number | null;
  reminder: number | null;
  reminder_done: boolean;
  priority: number;
  status: string;
  percent: number;
  category: string;
  message_id: number | null;
  completed_at: number | null;
  /** Exchange ile eşitlenen görevin hesabı */
  account_id?: number | null;
}

export interface Note {
  id: number;
  body: string;
  color: string;
  category: string;
  modified: number;
  /** Exchange ile eşitlenen notun hesabı */
  account_id?: number | null;
}

export interface Rule {
  id: number;
  name: string;
  enabled: boolean;
  account_id: number | null;
  field: string;
  op: string;
  value: string;
  action: string;
  action_arg: string;
  stop: boolean;
  /** Exchange sunucu kuralı (sunucuda çalışır) */
  remote_id?: string | null;
  /** Outlook'ta oluşturulmuş, basit düzenleyiciye sığmayan kural */
  complex?: boolean;
  /** Yerel kuralın tam modeli (LocalSpec, JSON) */
  spec?: string;
}

/** Yerel kuralın koşulu ya da eylemi */
export interface LocalPart {
  kind: string;
  op: string;
  values: string[];
}

/** Bu bilgisayarda çalışan kuralın tam modeli */
export interface LocalSpec {
  conditions: LocalPart[];
  exceptions: LocalPart[];
  actions: LocalPart[];
  match_any: boolean;
}

export interface Category {
  name: string;
  color: string;
}

export interface Reminder {
  key: string;
  kind: "event" | "task";
  item_id: number;
  title: string;
  location: string;
  start: number;
  end: number;
  all_day: boolean;
  is_meeting: boolean;
  organizer: string;
  /** Çevrimiçi toplantı bağlantısı (Teams, Zoom...) */
  join_url: string;
}

export type ModuleName = "mail" | "calendar" | "contacts" | "tasks" | "notes";

export interface RulePart {
  kind: string;
  values: string[];
}

export interface RuleSpec {
  conditions: RulePart[];
  exceptions: RulePart[];
  actions: RulePart[];
}

export interface ListOrder {
  by: string;
  desc: boolean;
  conversations: boolean;
}
