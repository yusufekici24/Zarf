// Görünüm: arayüz ve okuma yazı tipi, okunmamış iletilerin kalın gösterilmesi, takvim eşitleme hatası
import { expect, test, type Page } from "@playwright/test";
import { calls, fixtures, installMock, type Fixtures } from "./mock-backend";

const BASE = { language: "tr", start_module: "mail", reading_pane: "right", sync_on_select: "0" };

async function open(page: Page, over: Partial<Fixtures> = {}) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await installMock(page, fixtures({ ...over, settings: { ...BASE, ...over.settings } }));
  await page.goto("/");
  return errors;
}

const bodyFont = (page: Page) => page.evaluate(() => getComputedStyle(document.body).fontFamily);

test("varsayılan arayüz yazı tipi gömülü Noto Sans: kurulu Segoe UI kendiliğinden seçilmez", async ({ page }) => {
  const errors = await open(page);
  await expect(page.getByText("Bütçe toplantısı").first()).toBeVisible();
  const f = await bodyFont(page);
  expect(f.startsWith('"Noto Sans Variable"'), f).toBe(true);
  expect(f).not.toContain("Segoe");
  expect(errors).toEqual([]);
});

test("seçilen arayüz ve okuma yazı tipi uygulanır", async ({ page }) => {
  await open(page, { settings: { ui_font: "DejaVu Sans", read_font: "Liberation Serif", read_font_size: "14" } });
  await expect.poll(() => bodyFont(page)).toMatch(/^"DejaVu Sans", "Noto Sans Variable"/);
  await page.getByText("Bütçe toplantısı").first().click();
  const frame = page.frameLocator('iframe[title="message"]').first();
  await expect(frame.locator("body")).toContainText("saat 10:00");
  const style = await frame.locator("body").evaluate((b) => [getComputedStyle(b).fontFamily, getComputedStyle(b).fontSize]);
  expect(style[0]).toMatch(/^"Liberation Serif", Calibri/);
  // 14 pt = 18,67 px
  expect(parseFloat(style[1])).toBeCloseTo(18.67, 1);
});

test("okunmamış iletiler kalın ya da yalnızca renkli gösterilir", async ({ page }) => {
  await open(page);
  const unread = page.getByRole("option").filter({ hasText: "Bütçe toplantısı" }).getByText("Ayşe Yılmaz");
  await expect(unread).toHaveClass(/font-bold/);
});

test("okunmamışlar kalın gösterilmeyebilir", async ({ page }) => {
  await open(page, { settings: { unread_bold: "0" } });
  const unread = page.getByRole("option").filter({ hasText: "Bütçe toplantısı" }).getByText("Ayşe Yılmaz");
  await expect(unread).toHaveClass(/text-primary/);
  await expect(unread).not.toHaveClass(/font-bold/);
});

test("Seçenekler: yazı tipi listesi bu bilgisayarın yazı tiplerinden gelir ve Tamam ile kaydedilir", async ({ page }) => {
  const errors = await open(page, { window: "w-settings" });
  await page.waitForFunction(() => (window as unknown as { __mock: { listening: (e: string) => boolean } }).__mock.listening("window://assign"));
  await page.evaluate(() =>
    (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("window://assign", { kind: "settings", payload: { tab: "general" } }),
  );
  const select = page.locator("select").filter({ has: page.locator('option[value="DejaVu Sans"]') });
  await expect(select).toHaveCount(1);
  await expect(select.locator("option").first()).toHaveText("Varsayılan (Noto Sans)");
  await select.selectOption("DejaVu Sans");
  await page.getByRole("button", { name: "Tamam" }).click();
  await expect.poll(async () => (await calls(page, "set_setting")).find((a) => a.key === "ui_font")?.value).toBe("DejaVu Sans");
  expect(errors).toEqual([]);
});

test("takvim eşitlenemezse takvimde neden yazar ve yeniden denenebilir", async ({ page }) => {
  const errors = await open(page, {
    settings: { start_module: "calendar" },
    calendarErrors: [{ account: 1, error: "Exchange: You have exceeded the maximum number of objects (ErrorExceededFindCountLimit)" }],
  });
  const bar = page.getByTestId("calendar-sync-error");
  await expect(bar).toContainText("ben@kurum.com.tr takvimi eşitlenemedi: Exchange: You have exceeded");
  await bar.getByRole("button", { name: "Yeniden dene" }).click();
  await expect.poll(async () => (await calls(page, "sync_account")).map((a) => a.id)).toContain(1);
  expect(errors).toEqual([]);
});
