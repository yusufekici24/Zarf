// İleti gövdesinde tablo ve resim düzeni: sütunlar harf harf bölünmez (geniş tablo yatay kayar),
// boşluksuz uzun metin hücrede kırılır, ölçülü resimler oranını korur
import { expect, test, type Page } from "@playwright/test";
import { fixtures, installMock, type MockMessage } from "./mock-backend";

const NOW = Math.floor(Date.UTC(2026, 10, 5, 9, 0) / 1000);
const GIF = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
const svg = (w: number, h: number) => `data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' width='${w}' height='${h}'%3E%3Crect width='${w}' height='${h}' fill='%23cfe2ff'/%3E%3C/svg%3E`;
const cols = ["Hesap No", "Üye", "Borç", "Alacak", "Bakiye", "Valör", "Takas", "Saklama", "Komisyon", "Durum"];
const WIDE = `<table id="wide" border="1" cellspacing="0" cellpadding="4"><tr>${cols.map((c) => `<th>${c}</th>`).join("")}</tr>
<tr>${cols.map((_, i) => `<td class="c${i}">${i === 1 ? "Örnek" : "12.345.678,90"}</td>`).join("")}</tr></table>`;
const LONG = `<table id="long" border="1" cellspacing="0" width="100%"><tr><td>1</td>
<td id="url">https://izleme.kurum.com.tr/isler/2026/11/10/rapor?kimlik=4f8a9c1e-77aa-4d0c-9d0b-33c1a2b4c5d6&amp;ayrinti=tam&amp;sayfa=1&amp;dil=tr&amp;bicim=tablo&amp;sirala=tarih</td><td id="st">Başarılı</td></tr></table>`;
const IMAGES = `<p><img id="shot" width="1600" height="400" style="width:16.6667in;height:4.1667in" src="${svg(1600, 400)}"></p>
<table id="news" width="300" cellpadding="0" cellspacing="0"><tr><td><img id="spacer" src="${GIF}" width="300" height="1" style="display:block"></td></tr>
<tr><td><img id="logo" src="${svg(240, 60)}" width="120" height="30"></td></tr></table>`;
const msg = (id: number, subject: string, html: string): MockMessage => ({
  id, folder_id: 1, subject, sender: "Rapor <rapor@kurum.com.tr>", date: NOW - id * 60, snippet: subject, is_read: true, html,
});
const MESSAGES = [msg(1, "Geniş tablo", WIDE), msg(2, "Uzun adres", LONG), msg(3, "Resimler", IMAGES)];

async function show(page: Page, subject: string) {
  await installMock(page, fixtures({ messages: MESSAGES }));
  await page.goto("/");
  await page.getByText(subject).first().click();
  return page.frameLocator('iframe[title="message"]').first();
}
const box = async (page: Page, sel: string) => (await page.frameLocator('iframe[title="message"]').first().locator(sel).boundingBox())!;

test("geniş tablo: hücreler tek satır kalır, tablo yatay kayar", async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 700 });
  const f = await show(page, "Geniş tablo");
  await expect(f.locator("#wide")).toBeVisible();
  const one = (await box(page, "#wide th:nth-child(3)")).height;
  // Sayı sütunu bölünmez (eskiden "12.345.678,9 / 0" diye harf harf kırılıyordu)
  expect((await box(page, "#wide td.c2")).height).toBeLessThan(one * 1.3);
  const [sw, cw] = await f.locator("html").evaluate((h) => [h.scrollWidth, h.clientWidth]);
  expect(sw).toBeGreaterThan(cw);
});

test("boşluksuz uzun adres hücrenin içinde kırılır, tablo bölmeye sığar", async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 700 });
  const f = await show(page, "Uzun adres");
  await expect(f.locator("#long")).toBeVisible();
  const [sw, cw] = await f.locator("html").evaluate((h) => [h.scrollWidth, h.clientWidth]);
  expect(sw).toBeLessThanOrEqual(cw + 1);
  // Yanındaki olağan sözcük bölünmez
  expect((await box(page, "#st")).height).toBe((await box(page, "#url")).height);
  expect((await box(page, "#st")).width).toBeGreaterThan(40);
});

test("ölçülü resimler: sığmayan ekran görüntüsü oranını korur, ara resim ince kalır", async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 700 });
  const f = await show(page, "Resimler");
  await expect(f.locator("#logo")).toBeVisible();
  const shot = await box(page, "#shot");
  const pane = await f.locator("html").evaluate((h) => h.clientWidth);
  expect(shot.width).toBeLessThan(pane);
  expect(shot.width / shot.height).toBeCloseTo(4, 1);
  const spacer = await box(page, "#spacer");
  expect([Math.round(spacer.width), Math.round(spacer.height)]).toEqual([300, 1]);
  const logo = await box(page, "#logo");
  expect([Math.round(logo.width), Math.round(logo.height)]).toEqual([120, 30]);
});
