// Kullanıcı etkinliği arka uca (en çok saniyede bir): yedek pencere kullanıcı boştayken kurulur
// (src-tauri/src/windows.rs `spare_scheduler`). Kurulum GTK döngüsünü kısa süre dondurduğu için
// tıklama/yazma sırasında yapılmaz.
import { invoke } from "@tauri-apps/api/core";

let started = false;

export function startActivityReport() {
  if (started || typeof window === "undefined") return;
  started = true;
  let last = 0;
  const report = () => {
    const now = Date.now();
    if (now - last < 1000) return;
    last = now;
    void invoke("app_activity").catch(() => {});
  };
  for (const ev of ["pointerdown", "keydown", "wheel"]) window.addEventListener(ev, report, { capture: true, passive: true });
}
