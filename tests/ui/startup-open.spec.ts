// Açılış: pencere gömülü yazı tipi yüklenince gösterilir. İleti açma: gövde gecikirse başlıklar ve önizleme hemen.
import { expect, test } from "@playwright/test";
import { calls, fixtures, installMock } from "./mock-backend";

test("ana pencere, kullanılan bütün Noto Sans yüzleri (Türkçe harfler dahil) yüklendikten sonra gösterilir", async ({ page }) => {
  await installMock(page, fixtures());
  // Pencere gösterildiği anda yazı tiplerinin durumu kaydedilir
  await page.addInitScript(() => {
    const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a?: unknown) => Promise<unknown> }; __atReveal?: boolean[] };
    const orig = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === "reveal_window") w.__atReveal = ["400", "500", "600", "700", "italic 400", "italic 700"].map((f) => document.fonts.check(`${f} 16px "Noto Sans"`, "ĞğŞşİı"));
      return orig(cmd, args);
    };
  });
  await page.goto("/");
  await expect.poll(async () => (await calls(page, "reveal_window")).length).toBeGreaterThan(0);
  expect(await page.evaluate(() => (window as unknown as { __atReveal: boolean[] }).__atReveal)).toEqual([true, true, true, true, true, true]);
});

test("gövdesi henüz inmemiş ileti: başlık ve önizleme hemen, gövde gelince yerine geçer", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await installMock(page, fixtures());
  await page.goto("/");
  await expect(page.getByText("Bütçe toplantısı").first()).toBeVisible();
  // Gövde indirme 1,5 sn sürsün (Exchange'den ilk açılış)
  await page.evaluate(() => {
    const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a?: Record<string, unknown>) => Promise<unknown> }; __release?: () => void };
    const orig = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === "get_message" && args?.id === 1) await new Promise<void>((r) => (w.__release = r));
      return orig(cmd, args);
    };
  });
  await page.getByText("Bütçe toplantısı").first().click();
  const loading = page.getByTestId("body-loading");
  await expect(loading).toBeVisible();
  await expect(loading).toContainText("İleti indiriliyor");
  await expect(loading).toContainText("Yarınki bütçe toplantısı saat 10:00'da");
  // Başlıklar (gönderen) da görünür; ileti henüz okundu sayılmaz
  await expect(page.getByText("Ayşe Yılmaz").last()).toBeVisible();
  expect(await calls(page, "set_read")).toEqual([]);

  await page.evaluate(() => (window as unknown as { __release: () => void }).__release());
  await expect(loading).toHaveCount(0);
  await expect(page.frameLocator('iframe[title="message"]').first().locator("b")).toHaveText("bütçe toplantısı");
  expect(errors).toEqual([]);
});

test("gövdesi hazır ileti geçici görünüm göstermeden açılır", async ({ page }) => {
  await installMock(page, fixtures());
  await page.goto("/");
  await page.evaluate(() => {
    const w = window as unknown as { __seen: boolean; __obs: MutationObserver };
    w.__seen = false;
    w.__obs = new MutationObserver(() => { if (document.querySelector('[data-testid="body-loading"]')) w.__seen = true; });
    w.__obs.observe(document.body, { childList: true, subtree: true });
  });
  await page.getByText("Aylık rapor").first().click();
  await expect(page.frameLocator('iframe[title="message"]').first().locator("body")).toContainText("ekim ayı raporu");
  expect(await page.evaluate(() => (window as unknown as { __seen: boolean }).__seen)).toBe(false);
});

test("performans günlüğü: açılış satırı ve arayüz kilitlenmesi perf.log'a gider", async ({ page }) => {
  await installMock(page, fixtures());
  await page.goto("/");
  await expect(page.getByText("Bütçe toplantısı").first()).toBeVisible();
  const lines = async () => (await calls(page, "perf_log")).flatMap((a) => a.lines as string[]);
  await expect.poll(async () => (await lines()).some((l) => /^açılış gösterildi_ms=\d+ tıklanabilir_ms=\d+/.test(l)), { timeout: 10000 }).toBe(true);
  // Ana iş parçacığı 600 ms meşgul
  await page.evaluate(() => {
    const end = performance.now() + 600;
    while (performance.now() < end) { /* meşgul */ }
  });
  await expect.poll(async () => (await lines()).some((l) => /^kilit ms=(5|6|7)\d\d/.test(l)), { timeout: 12000 }).toBe(true);
});
