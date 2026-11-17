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

// ---------------------------------------------------------------- açılışta yazı tipi
// Gömülü Noto Sans kalınlık ve harf kümesi başına ayrı dosyadır (Türkçe ğ ş ı İ "latin-ext" kümesinde) ve
// `font-display: swap` ile yüklenir: tarayıcı dosyayı metin ekrana dizilince ister, gelene kadar yedek
// yazı tipini (Ubuntu/DejaVu, daha geniş) kullanır. WebKitGTK gizli pencerede dizim yapmadığı için dosyalar
// ancak pencere görününce iniyor; açılıştan kısa süre sonra bütün yazılar Noto Sans'a geçiyor ve ekran
// "yakınlaşmış, yazı tipi değişmiş" gibi görünüyordu. Kullanılan yüzler burada açıkça yükletilir ve pencere
// bunlar gelince gösterilir (+page.svelte).
export const UI_FONT_FACES = ["400", "500", "600", "700", "italic 400", "italic 700"];
/** Hem temel Latin hem Türkçe harflerin kümesi istensin diye */
const FONT_SAMPLE = "Aa Zz 09 ĞğŞşİıÇçÖöÜü";

let fontsReady: Promise<void> | null = null;

/** Arayüz yazı tipi dosyaları yüklendi (en fazla `timeoutMs` beklenir; yüklenemezse pencere yine açılır) */
export function uiFontsReady(timeoutMs = 2500): Promise<void> {
  if (fontsReady) return fontsReady;
  const fonts = typeof document !== "undefined" ? document.fonts : undefined;
  if (!fonts?.load) return (fontsReady = Promise.resolve());
  // Stil dosyası henüz eklenmediyse `load` eşleşen yüz bulamaz ve hemen döner: önce yüzlerin gelmesi beklenir
  const declared = () => [...fonts].some((f) => f.family.replace(/["']/g, "") === "Noto Sans");
  const all = (async () => {
    for (let i = 0; i < 80 && !declared(); i++) await new Promise((r) => setTimeout(r, 25));
    await Promise.all(UI_FONT_FACES.map((f) => fonts.load(`${f} 16px "Noto Sans"`, FONT_SAMPLE).catch(() => [])));
  })();
  fontsReady = Promise.race([all, new Promise<void>((r) => setTimeout(r, timeoutMs))]);
  return fontsReady;
}
