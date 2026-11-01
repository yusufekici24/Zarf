// Yeni posta uyarı kartındaki düğmeler: kullanıcı Ayarlar > Posta'dan seçer
export type AlertAction = "reply" | "reply_all" | "forward" | "archive" | "delete" | "read" | "flag" | "mute";

/** Eylem → ad (çeviri anahtarı) */
export const ALERT_ACTIONS: Record<AlertAction, string> = {
  reply: "mail.reply",
  reply_all: "mail.reply_all",
  forward: "mail.forward",
  archive: "mail.archive",
  delete: "common.delete",
  read: "mail.mark_read",
  flag: "mail.flag",
  mute: "alerts.mute",
};

/** Varsayılan düğmeler: Yanıtla, Sil, Arşivle */
export const ALERT_BUTTON_DEFAULTS = ["reply", "delete", "archive"];

/** Ayarlanmış düğmeler ("none" seçilenler atlanır) */
export function alertButtons(setting: (key: string, def: string) => string): AlertAction[] {
  return ALERT_BUTTON_DEFAULTS.map((d, i) => setting(`alert_btn${i + 1}`, d)).filter((v): v is AlertAction => v in ALERT_ACTIONS);
}
