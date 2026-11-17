// Arayüz performans ölçümü → ~/.local/share/lynora/perf.log (src-tauri/src/perf.rs, komut `perf_log`).
// Her zaman açık ve hafiftir; yalnızca yavaş olaylar yazılır (kişisel içerik yok):
// - kilit: ana iş parçacığı bu kadar süre hiçbir şey işleyemedi (tıklama, çizim bekledi)
// - tıklama_gecikmesi: tıklama olayı işlenmeye başlayana kadar geçen süre
// - tıklama_çizim: tıklamadan ekranın güncellenmesine kadar geçen süre
// - açılış: pencere gösterildikten sonra arayüzün ilk kez boşa çıktığı an ("tıklanabilir")
// - ileti_çiz: iletiye tıklamadan okuma bölmesinde gösterilene kadar (indirme + çizim)
import { invoke } from "@tauri-apps/api/core";

const BLOCK_MS = 300;
const CLICK_DELAY_MS = 150;
const CLICK_PAINT_MS = 300;
const queue: string[] = [];
let started = false;

export function perfNote(line: string) {
  queue.push(`${line} t=${Math.round(performance.now())}`);
  if (queue.length > 200) queue.splice(0, queue.length - 200);
}

function flush() {
  if (!queue.length) return;
  const lines = queue.splice(0, 50);
  void invoke("perf_log", { lines }).catch(() => {});
}

/** Bir sonraki çizimden sonra (requestAnimationFrame gizli pencerede çalışmaz: zaman aşımıyla) */
export function afterPaint(): Promise<void> {
  return new Promise((resolve) => {
    let done = false;
    const end = () => {
      if (!done) {
        done = true;
        resolve();
      }
    };
    requestAnimationFrame(() => setTimeout(end, 0));
    setTimeout(end, 3000);
  });
}

export function startPerfMonitor() {
  if (started || typeof window === "undefined") return;
  started = true;

  // Kilitlenme: 100 ms'lik zamanlayıcı çok geç gelirse ana iş parçacığı o kadar meşguldü. Gizli pencerede
  // WebKit tekrarlanan zamanlayıcıları kısar (1 sn'lik dilimler): yalnızca görünürken sayılır ve görünürlük
  // değişince sayaç sıfırlanır (yedek pencere gösterildiği an gelen kısılmış tik "kilit" sayılmasın).
  let last = performance.now();
  let longest = 0;
  // Önceki tik gizliyken geldiyse aradaki boşluk kısmadandır (pencere gösterilince kısılmış tik, görünürlük
  // olayından önce gelebiliyor): sayılmaz
  let wasVisible = document.visibilityState === "visible";
  document.addEventListener("visibilitychange", () => {
    last = performance.now();
    wasVisible = false;
  });
  setInterval(() => {
    const now = performance.now();
    const gap = now - last - 100;
    last = now;
    const counted = wasVisible;
    wasVisible = document.visibilityState === "visible";
    if (!counted || !wasVisible) return;
    if (gap > longest) longest = gap;
    if (gap >= BLOCK_MS) perfNote(`kilit ms=${Math.round(gap)}`);
  }, 100);

  // Tıklama: olayın zamanı ile işlenmeye başladığı an arasındaki fark ve ekranın güncellenmesi
  window.addEventListener(
    "pointerdown",
    (e) => {
      const begin = performance.now();
      const delay = begin - e.timeStamp;
      if (delay >= CLICK_DELAY_MS && delay < 60000) perfNote(`tıklama_gecikmesi ms=${Math.round(delay)}`);
      void afterPaint().then(() => {
        const total = performance.now() - (delay >= 0 && delay < 60000 ? e.timeStamp : begin);
        if (total >= CLICK_PAINT_MS) perfNote(`tıklama_çizim ms=${Math.round(total)}`);
      });
    },
    { capture: true, passive: true },
  );

  setInterval(flush, 5000);
  window.addEventListener("beforeunload", flush);
}

/** Ana pencere gösterildi: arayüzün boşa çıktığı ilk anı (500 ms kilitsiz) ölçüp açılış satırını yazar */
export function measureStartup(revealAt: number) {
  let last = performance.now();
  let quietSince = last;
  let longest = 0;
  const timer = setInterval(() => {
    const now = performance.now();
    const gap = now - last - 50;
    last = now;
    if (gap > 100) {
      quietSince = now;
      longest = Math.max(longest, gap);
    }
    if (now - quietSince >= 500 || now - revealAt > 20000) {
      clearInterval(timer);
      perfNote(`açılış gösterildi_ms=${Math.round(revealAt)} tıklanabilir_ms=${Math.round(quietSince)} en_uzun_kilit_ms=${Math.round(longest)} çekirdek=${navigator.hardwareConcurrency} ölçek=${devicePixelRatio}`);
      flush();
    }
  }, 50);
}
