// Outlook "Kırtasiye ve Yazı Tipleri": yeni iletiler, yanıtlar/iletmeler ve düz metin için
// varsayılan yazı tipi (ad, punto, renk, kalın, italik). Ayarlarda JSON olarak saklanır.

export type FontSpec = { family: string; size: number; color: string; bold: boolean; italic: boolean };

export const FONT_DEFAULTS: Record<"font_new" | "font_reply" | "font_plain", FontSpec> = {
  font_new: { family: "Calibri", size: 11, color: "", bold: false, italic: false },
  font_reply: { family: "Calibri", size: 11, color: "", bold: false, italic: false },
  font_plain: { family: "Consolas", size: 10, color: "", bold: false, italic: false },
};
export type FontKey = keyof typeof FONT_DEFAULTS;

/** Her yerde (Windows Outlook dahil) görünen yazı tipleri */
export const COMMON_FONTS = ["Calibri", "Aptos", "Arial", "Times New Roman", "Georgia", "Verdana", "Tahoma", "Trebuchet MS", "Courier New", "Consolas", "Segoe UI"];
export const FONT_SIZES = [8, 9, 10, 10.5, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];

export function parseFont(json: string | undefined, key: FontKey): FontSpec {
  const def = FONT_DEFAULTS[key];
  try {
    const v = JSON.parse(json || "null");
    if (v && typeof v === "object") return { ...def, ...v, size: Number(v.size) || def.size };
  } catch {
    /* varsayılan */
  }
  return { ...def };
}

/** Yazı tipinin ardından yedekler: alıcıda ya da bu bilgisayarda yoksa benzerine düşer */
export function fontStack(name: string): string {
  const serif = /serif|times|georgia|garamond|cambria|liberation serif|dejavu serif|noto serif/i.test(name) && !/sans/i.test(name);
  const mono = /mono|courier|consol|code/i.test(name);
  const fb = mono ? "'Courier New', 'Liberation Mono', monospace" : serif ? "'Times New Roman', 'Liberation Serif', serif" : "Calibri, Carlito, 'Noto Sans', Arial, sans-serif";
  return `'${name.replace(/'/g, "")}', ${fb}`;
}

/** Satır içi CSS (düzenleyici ve gönderilen HTML) */
export function fontCss(f: FontSpec, withColor = true): string {
  return [
    `font-family:${fontStack(f.family)}`,
    `font-size:${f.size}pt`,
    withColor && f.color ? `color:${f.color}` : "",
    f.bold ? "font-weight:bold" : "",
    f.italic ? "font-style:italic" : "",
  ]
    .filter(Boolean)
    .join(";");
}
