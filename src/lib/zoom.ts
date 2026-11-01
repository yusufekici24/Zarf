// Ekran çözünürlüğüne göre arayüz ölçeği (yazılar, simgeler, pencereler). "auto" seçiliyse
// ekranın mantıksal boyutuna göre seçilir; kullanıcı Ayarlar'dan ya da Ctrl + / Ctrl - /
// Ctrl 0 ile değiştirebilir. Sayfa yakınlaştırması (webview zoom) kullanılır, CSS zoom değil:
// açılır menülerin konumları bozulmaz. Uygulama açıkken çözünürlük değişirse ölçek ve pencere
// boyutu, uygulama yeniden açılmış gibi yeni ekrana göre ayarlanır.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "@tauri-apps/api/core";

export const SCALES = ["0.9", "1", "1.1", "1.25", "1.5"];

/** Ekranın mantıksal boyutu: arka uçtan (sayfa yakınlaştırması window.screen'i etkileyebilir) */
let screenW = 0;
let screenH = 0;

async function readScreen(): Promise<[number, number]> {
  try {
    const r = await invoke<[number, number] | null>("screen_info");
    if (r && r[0] > 0) return r;
  } catch {
    /* tarayıcıda */
  }
  return [window.screen?.width || 0, window.screen?.height || 0];
}

/** Ekrana göre önerilen ölçek */
export function autoScale(): number {
  const w = Math.max(screenW || window.screen?.width || 0, screenH || window.screen?.height || 0);
  if (w >= 3200) return 1.5;
  if (w >= 2500) return 1.25;
  if (w >= 1900) return 1.1;
  if (w > 0 && w < 1400) return 0.9;
  return 1;
}

export function resolveScale(setting: string): number {
  const v = Number(setting);
  return setting && setting !== "auto" && v > 0.5 && v < 3 ? v : autoScale();
}

let current = 0;
/** Şu an uygulanan sayfa yakınlaştırması */
export function currentScale(): number {
  return current || 1;
}
export async function applyScale(setting: string, force = false) {
  if (!screenW) [screenW, screenH] = await readScreen();
  const z = resolveScale(setting);
  if (!force && Math.abs(z - current) < 0.001) return;
  current = z;
  await invoke("set_ui_scale", { scale: z }).catch(() => {});
  try {
    await getCurrentWebview().setZoom(z);
  } catch {
    // Tarayıcıda (geliştirme) ya da eski çalışma ortamında: CSS ile
    document.documentElement.style.zoom = String(z);
  }
}

/** Pencereyi ekrana göre açılış boyutuna getirir */
export function fitWindow() {
  return invoke("fit_window").catch(() => {});
}

/**
 * Çözünürlük değişikliğini izler: değişince ölçek yeniden hesaplanır ve pencere yeni
 * ekrana sığacak şekilde boyutlandırılır. Döndürülen fonksiyon izlemeyi durdurur.
 */
export function watchScreen(getSetting: () => string): () => void {
  let busy = false;
  // Seyrek denetim (her pencerede çalışır): gizli/simge durumundaki pencerede atlanır
  const timer = setInterval(async () => {
    if (busy || document.hidden) return;
    busy = true;
    try {
      const [w, h] = await readScreen();
      if (w > 0 && (Math.abs(w - screenW) > 1 || Math.abs(h - screenH) > 1)) {
        const first = !screenW;
        screenW = w;
        screenH = h;
        if (!first) {
          await applyScale(getSetting(), true);
          await fitWindow();
        }
      }
    } finally {
      busy = false;
    }
  }, 6000);
  return () => clearInterval(timer);
}

/** Bir kademe büyüt/küçült (Ctrl + / Ctrl -) */
export function stepScale(setting: string, dir: number): string {
  const z = resolveScale(setting);
  const list = SCALES.map(Number);
  let i = list.findIndex((x) => Math.abs(x - z) < 0.001);
  if (i < 0) i = list.findIndex((x) => x > z) - (dir > 0 ? 1 : 0);
  const n = Math.max(0, Math.min(list.length - 1, i + dir));
  return String(list[n]);
}
