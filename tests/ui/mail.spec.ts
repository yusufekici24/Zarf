// Ana pencere: posta kutusu, ileti okuma, arama ve klasör geçişi (sahte arka uçla)
import { expect, test } from "@playwright/test";
import { calls, fixtures, installMock } from "./mock-backend";

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { errors: string[] }).errors = errors;
  await installMock(page, fixtures());
  await page.goto("/");
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { errors: string[] }).errors, "sayfada yakalanmamış hata olmamalı").toEqual([]);
});

test("posta kutusu açılır: klasörler, iletiler ve okunmamış sayısı", async ({ page }) => {
  await expect(page.getByText("Bütçe toplantısı").first()).toBeVisible();
  await expect(page.getByText("Aylık rapor").first()).toBeVisible();
  await expect(page.getByText("Gönderilmiş Öğeler").first()).toBeVisible();
  await expect(page.getByText("Öğe: 2")).toBeVisible();
  await expect(page.getByText("Okunmamış: 1")).toBeVisible();
  // Gönderilmiş klasördeki ileti gelen kutusunda görünmez
  await expect(page.getByText("RE: Proje takvimi")).toHaveCount(0);
});

test("ileti seçilince gövdesi korumalı çerçevede görünür", async ({ page }) => {
  await page.getByText("Bütçe toplantısı").first().click();
  const body = page.frameLocator('iframe[title="message"]').first().locator("body");
  await expect(body).toContainText("saat 10:00");
  await expect(body.locator("b")).toHaveText("bütçe toplantısı");
  expect((await calls(page, "get_message")).map((a) => a.id)).toContain(1);
  // Çerçeve betik çalıştıramaz (yalnızca aynı köken + uyarı kutuları)
  await expect(page.locator('iframe[title="message"]').first()).toHaveAttribute("sandbox", "allow-same-origin allow-modals");
});

test("arama Türkçe harf duyarsız süzer ve arka uca iletilir", async ({ page }) => {
  await expect(page.getByText("Aylık rapor").first()).toBeVisible();
  const box = page.getByPlaceholder(/ara/i).first();
  await box.click();
  await box.fill("BUTCE");
  await box.press("Enter");
  await expect(page.getByText("Aylık rapor")).toHaveCount(0);
  await expect(page.getByText("Bütçe toplantısı").first()).toBeVisible();
  expect((await calls(page, "list_messages")).some((a) => String(a.search).trim() === "BUTCE")).toBe(true);
});

test("klasör değişince o klasörün iletileri listelenir", async ({ page }) => {
  await page.getByText("Gönderilmiş Öğeler").first().click();
  await expect(page.getByText("RE: Proje takvimi").first()).toBeVisible();
  await expect(page.getByText("Aylık rapor")).toHaveCount(0);
  expect((await calls(page, "list_messages")).some((a) => a.folderId === 2)).toBe(true);
});

test("yavaş klasör sayacı ileti listesi yenilenmesini engellemez", async ({ page }) => {
  await installMock(page, fixtures());
  await page.goto("/");
  await expect(page.getByText("Bütçe toplantısı").first()).toBeVisible();
  await page.evaluate(() => {
    const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args?: unknown) => Promise<unknown> }; __mock: { emit: (event: string, payload: unknown) => void }; releaseFolders: () => void; listRefreshed: boolean };
    const original = w.__TAURI_INTERNALS__.invoke;
    w.listRefreshed = false;
    const hold = new Promise<void>(resolve => { w.releaseFolders = resolve; });
    w.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === "folder_tree") await hold;
      if (cmd === "list_messages") w.listRefreshed = true;
      return original(cmd, args);
    };
    w.__mock.emit("mail://changed", {});
  });
  await expect.poll(() => page.evaluate(() => (window as unknown as { listRefreshed: boolean }).listRefreshed)).toBe(true);
  await page.evaluate(() => (window as unknown as { releaseFolders: () => void }).releaseFolders());
});
