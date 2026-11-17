// Arayüz ve okuma yazı tipi (Seçenekler > Genel ve Posta > Okuma Bölmesi).
// Varsayılan arayüz yazı tipi pakete gömülü Noto Sans'tır: her bilgisayarda aynı görünür.
// Sistemde Windows yazı tipleri kurulu olsa bile "Segoe UI Variable" kendiliğinden seçilmez:
// WebKitGTK bu değişken yazı tipinin kalınlık eksenini her zaman uygulayamıyor ve tüm metin
// (okunmuş iletiler dahil) kalın görünebiliyor. İsteyen yazı tipini Seçenekler'den seçer.
// Aynı nedenle gömülü Noto Sans da değişken değil, kalınlık başına ayrı dosyadır (400/500/600/700; +layout.svelte).

export const UI_FONT_DEFAULT = `"Noto Sans", "Ubuntu", "Cantarell", "DejaVu Sans", "Liberation Sans", system-ui, sans-serif`;

/** Kendi yazı tipi olmayan iletilerin okuma yazı tipi (Outlook varsayılanı Calibri 11 pt) */
export const READ_FONT_DEFAULT = `Calibri, Carlito, "Noto Sans", sans-serif`;
export const READ_SIZES = ["9", "10", "11", "12", "14", "16", "18"];

function quoted(name: string): string {
  return `"${name.replace(/["\\]/g, "")}"`;
}

/** Ayar değerinden CSS yazı tipi sırası: "" varsayılan, "system" masaüstünün yazı tipi, ya da aile adı */
export function uiFontStack(setting: string): string {
  if (!setting) return UI_FONT_DEFAULT;
  if (setting === "system") return `system-ui, ${UI_FONT_DEFAULT}`;
  return `${quoted(setting)}, ${UI_FONT_DEFAULT}`;
}

export function readFontStack(setting: string): string {
  return setting ? `${quoted(setting)}, ${READ_FONT_DEFAULT}` : READ_FONT_DEFAULT;
}

export function applyUiFont(setting: string) {
  document.documentElement.style.setProperty("--ui-font", uiFontStack(setting));
}
