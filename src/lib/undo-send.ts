// Gönderimi geri alma (Outlook "Gönderimi geri al"): Seçenekler > Posta > Gönderme > `undo_send` (saniye).
// İleti bu süre boyunca Giden Kutusu'nda bekler; arka uç süre dolunca tam zamanında gönderir
// (src-tauri/src/sync.rs `send_later`). Ana penceredeki bildirim geri sayar: "Geri al" iletiyi taslağa
// döndürür, "Şimdi gönder" beklemeyi keser. İmzalı/şifreli ve zamanlanmış iletiler bekletilmez.

export const UNDO_SEND_OPTIONS = ["0", "5", "10", "20", "30"];
export const UNDO_SEND_DEFAULT = "10";

/** Bildirimde gösterilecek kalan saniye (en az 1; süre dolunca bildirim kapanır) */
export function secondsLeft(dueMs: number, nowMs = Date.now()): number {
  return Math.max(1, Math.ceil((dueMs - nowMs) / 1000));
}
