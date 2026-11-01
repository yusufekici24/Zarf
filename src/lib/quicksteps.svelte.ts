// Hızlı Adımlar (Outlook): tek tıkla birden çok eylem (taşı, bayrakla, ilet, yanıtla ve sil...).
// Adımlar yerelde ayar olarak saklanır; ana pencerede ve ileti penceresinde aynı liste kullanılır.
import { api } from "$lib/api";
import { app } from "$lib/stores/app.svelte";
import { t } from "$lib/i18n.svelte";
import { addDays, startOfDay, toTs } from "$lib/format";
import type { MessageRow } from "$lib/types";

export type QsFlag = "today" | "tomorrow" | "next_week" | "no_date";
export type QsAction =
  | { type: "move"; folder: number | null }
  | { type: "copy"; folder: number | null }
  | { type: "archive" }
  | { type: "delete" }
  | { type: "read" }
  | { type: "unread" }
  | { type: "flag"; when: QsFlag }
  | { type: "complete" }
  | { type: "clear_flag" }
  | { type: "categorize"; name: string }
  | { type: "importance"; level: number }
  | { type: "reply" }
  | { type: "reply_all" }
  | { type: "forward"; to: string }
  | { type: "forward_attachment"; to: string }
  | { type: "new_mail"; to: string; cc: string; subject: string }
  | { type: "meeting" };
export type QsActionType = QsAction["type"];

export interface QuickStep {
  id: string;
  name: string;
  icon: string;
  tooltip: string;
  actions: QsAction[];
}

/** Eylem türleri (düzenleyicideki sırayla, Outlook'taki gruplara göre) */
export const ACTION_TYPES: QsActionType[] = [
  "move", "copy", "archive", "delete",
  "read", "unread", "flag", "complete", "clear_flag", "categorize", "importance",
  "reply", "reply_all", "forward", "forward_attachment", "new_mail", "meeting",
];

export function blankAction(type: QsActionType): QsAction {
  switch (type) {
    case "move":
    case "copy":
      return { type, folder: null };
    case "flag":
      return { type, when: "today" };
    case "categorize":
      return { type, name: app.categories[0]?.name ?? "" };
    case "importance":
      return { type, level: 2 };
    case "forward":
    case "forward_attachment":
      return { type, to: "" };
    case "new_mail":
      return { type, to: "", cc: "", subject: "" };
    default:
      return { type } as QsAction;
  }
}

/** Ayarlanmamış alanı olan eylem (ilk kullanımda sorulur) */
export function incomplete(a: QsAction): boolean {
  if (a.type === "move" || a.type === "copy") return a.folder == null;
  if (a.type === "forward" || a.type === "forward_attachment") return !a.to.trim();
  if (a.type === "new_mail") return !a.to.trim();
  if (a.type === "categorize") return !a.name;
  return false;
}
export const needsSetup = (s: QuickStep) => !s.actions.length || s.actions.some(incomplete);

/** Outlook'un hazır hızlı adımları */
function defaults(): QuickStep[] {
  return [
    { id: "move", name: t("qs.d_move"), icon: "move", tooltip: t("qs.d_move_tip"), actions: [{ type: "move", folder: null }, { type: "read" }] },
    { id: "manager", name: t("qs.d_manager"), icon: "forward", tooltip: t("qs.d_manager_tip"), actions: [{ type: "forward", to: "" }] },
    { id: "team", name: t("qs.d_team"), icon: "team", tooltip: t("qs.d_team_tip"), actions: [{ type: "new_mail", to: "", cc: "", subject: "" }] },
    { id: "done", name: t("qs.d_done"), icon: "done", tooltip: t("qs.d_done_tip"), actions: [{ type: "read" }, { type: "complete" }, { type: "archive" }] },
    { id: "reply_delete", name: t("qs.d_reply_delete"), icon: "reply_delete", tooltip: t("qs.d_reply_delete_tip"), actions: [{ type: "reply" }, { type: "delete" }] },
  ];
}

class QuickSteps {
  get list(): QuickStep[] {
    const raw = app.setting("quick_steps", "");
    if (!raw) return defaults();
    try {
      const v = JSON.parse(raw);
      return Array.isArray(v) ? v : defaults();
    } catch {
      return defaults();
    }
  }

  async save(list: QuickStep[]) {
    await app.setSetting("quick_steps", JSON.stringify(list));
  }

  async upsert(step: QuickStep) {
    const list = this.list;
    const i = list.findIndex((s) => s.id === step.id);
    if (i >= 0) list[i] = step;
    else list.push(step);
    await this.save(list);
  }

  async reset() {
    await app.setSetting("quick_steps", "");
  }

  /** Adımı seçili iletilere uygular. Döner: iletiler listeden kalktı mı (taşındı/silindi) */
  async run(step: QuickStep, rows: MessageRow[]): Promise<boolean> {
    const items = rows.filter((r) => r.id > 0);
    if (!items.length) return false;
    const ids = items.map((r) => r.id);
    let gone = false;
    // Önce ileti pencereleri açılır (kaynak ileti silinmeden/taşınmadan önce içeriği alınır)
    for (const a of step.actions) {
      try {
        if (a.type === "reply" || a.type === "reply_all" || a.type === "forward") {
          for (const id of ids.slice(0, 5)) {
            const d = await api.composePrefill(a.type, id);
            if (a.type === "forward" && a.to.trim()) d.to = a.to;
            app.compose({ mode: a.type, data: d, attachments: d.attachments });
          }
        } else if (a.type === "forward_attachment") {
          const atts = [];
          for (const id of ids) {
            const f = await api.messageTempFile(id, "msg");
            atts.push({ name: f.name, mime: f.mime, size: f.size, path: f.path });
          }
          app.compose({ mode: "new", data: { to: a.to, subject: `${t("compose.fw_prefix")}: ${items[0].subject}` }, attachments: atts });
        } else if (a.type === "new_mail") {
          app.compose({ mode: "new", data: { to: a.to, cc: a.cc, subject: a.subject } });
        } else if (a.type === "meeting") {
          window.dispatchEvent(new CustomEvent("mailnix:meeting-from-mail", { detail: ids[0] }));
        }
      } catch (e) {
        app.error(e);
      }
    }
    // Sonra iletiye uygulanan eylemler (yazıldığı sırayla; taşıma/silme en sonda)
    const later: QsAction[] = [];
    for (const a of step.actions) {
      try {
        switch (a.type) {
          case "read":
          case "unread":
            await api.setRead(ids, a.type === "read");
            break;
          case "flag":
            await api.setFlagged(ids, true, flagDue(a.when));
            break;
          case "complete":
          case "clear_flag":
            await api.setFlagged(ids, false);
            break;
          case "categorize":
            for (const r of items) if (a.name && !(r.categories ?? "").split(",").includes(a.name)) await api.toggleCategory([r.id], a.name);
            break;
          case "importance":
            await api.setImportance(ids, a.level);
            break;
          case "copy":
            if (a.folder != null) await api.copyMessages(ids, a.folder);
            break;
          case "move":
          case "archive":
          case "delete":
            later.push(a);
            break;
        }
      } catch (e) {
        app.error(e);
      }
    }
    for (const a of later) {
      try {
        if (a.type === "move" && a.folder != null) await api.moveMessages(ids, a.folder);
        else if (a.type === "archive") await api.moveToRole(ids, "archive");
        else if (a.type === "delete") await api.deleteMessages(ids);
        gone = true;
        break; // ileti artık başka yerde: sonraki taşıma/silme uygulanmaz
      } catch (e) {
        app.error(e);
      }
    }
    app.bump("tasks");
    return gone;
  }
}

function flagDue(w: QsFlag): number | null {
  const today = toTs(startOfDay(new Date())) + 17 * 3600;
  if (w === "today") return today;
  if (w === "tomorrow") return today + 86400;
  if (w === "next_week") return toTs(addDays(startOfDay(new Date()), 8 - (new Date().getDay() || 7))) + 17 * 3600;
  return null;
}

export const quickSteps = new QuickSteps();
