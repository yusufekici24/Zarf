// Konuşmalar (OWA gibi): listede alt alta açılan iletiler, okuma bölmesinde alt alta görünüm,
// sıra ve eski görünüm seçenekleri; konuşmada açılan ileti okundu olur
import { expect, test, type Page } from "@playwright/test";
import { calls, fixtures, installMock, type MockMessage } from "./mock-backend";

const NOW = Math.floor(Date.UTC(2026, 10, 5, 9, 0) / 1000);
const msg = (id: number, folder_id: number, subject: string, sender: string, ago: number, is_read: boolean, snippet: string): MockMessage => ({
  id, folder_id, subject, sender, date: NOW - ago, snippet, is_read, html: `<p>${snippet}</p>`,
});
const MESSAGES = [
  msg(11, 1, "Bütçe planı", "Ayşe Yılmaz <ayse@kurum.com.tr>", 3000, false, "İlk taslak"),
  msg(12, 2, "RE: Bütçe planı", "Ben <ben@kurum.com.tr>", 2000, true, "Teşekkürler"),
  msg(13, 1, "YNT: Bütçe planı", "Mehmet Kaya <mehmet@kurum.com.tr>", 1000, false, "Son hâli ekte"),
  msg(14, 1, "Toplantı", "Zeynep Demir <zeynep@kurum.com.tr>", 500, true, "Saat 14:00"),
];
const SETTINGS = { language: "tr", start_module: "mail", reading_pane: "right", sync_on_select: "0", conversations: "1" };

async function open(page: Page, settings: Record<string, string> = {}) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await installMock(page, fixtures({ messages: MESSAGES, settings: { ...SETTINGS, ...settings } }));
  await page.goto("/");
  await expect(page.getByText("YNT: Bütçe planı").first()).toBeVisible();
  return errors;
}

const body = (page: Page) => page.frameLocator('iframe[title="message"]').first().locator("body");
const readCalls = async (page: Page) => (await calls(page, "set_read")).filter((a) => a.value === true).flatMap((a) => a.ids as number[]);

test("listede konuşma açılır: Gönderilmiş dahil iletiler alt alta, tıklanan ileti açılır ve okundu olur", async ({ page }) => {
  const errors = await open(page);
  // Konuşma satırı: gönderilen yanıt dahil üç ileti (açılınca görünenlerle aynı sayı)
  await expect(page.getByText("(3)")).toBeVisible();
  await expect(page.getByTestId("thread-item")).toHaveCount(0);
  await page.getByRole("button", { name: "Konuşmayı genişlet" }).click();
  const items = page.getByTestId("thread-item");
  await expect(items).toHaveCount(3);
  // En yeni üstte; başka klasördeki yanıt klasör adıyla
  await expect(items.nth(0)).toContainText("Mehmet Kaya");
  await expect(items.nth(1)).toContainText("Gönderilmiş Öğeler · Teşekkürler");
  await expect(items.nth(2)).toContainText("Ayşe Yılmaz");
  await items.nth(2).click();
  await expect(body(page)).toContainText("İlk taslak");
  await expect.poll(() => readCalls(page)).toContain(11);
  await page.getByRole("button", { name: "Konuşmayı daralt" }).click();
  await expect(page.getByTestId("thread-item")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("okuma bölmesinde iletiler alt alta: gösterilen açık, diğerine tıklayınca o açılır ve okundu olur", async ({ page }) => {
  const errors = await open(page);
  await page.getByText("YNT: Bütçe planı").first().click();
  const conv = page.getByTestId("conversation");
  await expect(page.getByText("Konuşmada 3 ileti")).toBeVisible();
  await expect(conv.getByTestId("conversation-open")).toHaveCount(1);
  await expect(body(page)).toContainText("Son hâli ekte");
  const closed = conv.getByTestId("conversation-item");
  await expect(closed).toHaveCount(2);
  await expect(closed.nth(0)).toContainText("Ben");
  await expect(closed.nth(1)).toContainText("Ayşe Yılmaz");
  // Konuşma açılınca bu klasördeki okunmamışların hepsi okundu olur; satır kalın kalmaz
  await expect.poll(() => readCalls(page)).toEqual(expect.arrayContaining([13, 11]));
  await expect(page.getByRole("option", { name: /YNT: Bütçe planı/ }).locator(".font-bold")).toHaveCount(0);
  // Açık iletinin çerçevesi içeriği kadar: kısa ileti kısa kalır, kendi kendine uzamaz
  const frame = conv.getByTestId("conversation-open").locator('iframe[title="message"]');
  const h1 = await frame.evaluate((f) => f.getBoundingClientRect().height);
  await page.waitForTimeout(400);
  expect([h1, await frame.evaluate((f) => f.getBoundingClientRect().height)]).toEqual([h1, h1]);
  expect(h1).toBeLessThan(160);
  // Eski sürümde konuşmada açılan ileti okundu olmuyordu
  await closed.nth(1).click();
  await expect(body(page)).toContainText("İlk taslak");
  await expect(conv.getByTestId("conversation-item")).toHaveCount(2);
  await expect.poll(() => readCalls(page)).toContain(11);
  expect(errors).toEqual([]);
});

test("konuşma satırı: en üstteki okunmuş olsa da içinde okunmamış varsa kalın görünür; alt satır yalnızca kendini okur", async ({ page }) => {
  await installMock(page, fixtures({ messages: MESSAGES.map((m) => (m.id === 13 ? { ...m, is_read: true } : m)), settings: SETTINGS }));
  await page.goto("/");
  const head = page.getByRole("option", { name: /YNT: Bütçe planı/ }).first();
  await expect(head.locator(".font-bold").first()).toBeVisible();
  await page.getByRole("button", { name: "Konuşmayı genişlet" }).click();
  // Alt satırda okunmuş iletiye tıklamak konuşmayı okundu yapmaz
  await page.getByTestId("thread-item").nth(0).click();
  await expect(body(page)).toContainText("Son hâli ekte");
  await page.waitForTimeout(300);
  expect(await readCalls(page)).toEqual([]);
  await expect(head.locator(".font-bold").first()).toBeVisible();
  // Baş satırın "okundu" düğmesi konuşmanın tamamını okur
  await head.getByRole("button", { name: "Okundu olarak işaretle" }).first().click();
  await expect.poll(() => readCalls(page)).toContain(11);
  await expect(head.locator(".font-bold")).toHaveCount(0);
});

test("konuşma sırası: en yeni altta seçilince en eski ileti üstte", async ({ page }) => {
  await open(page, { conversation_order: "oldest" });
  await page.getByText("YNT: Bütçe planı").first().click();
  const closed = page.getByTestId("conversation").getByTestId("conversation-item");
  await expect(closed.nth(0)).toContainText("Ayşe Yılmaz");
  await expect(closed.nth(1)).toContainText("Ben");
});

test("eski görünüm seçeneği: tek ileti ve üstte geçiş düğmeleri; listede ok gizlenebilir", async ({ page }) => {
  await open(page, { conversation_layout: "single", conversation_expand: "0" });
  await expect(page.getByRole("button", { name: "Konuşmayı genişlet" })).toHaveCount(0);
  await page.getByText("YNT: Bütçe planı").first().click();
  await expect(body(page)).toContainText("Son hâli ekte");
  await expect(page.getByTestId("conversation")).toHaveCount(0);
  await page.getByRole("button", { name: /Ayşe Yılmaz ·/ }).click();
  await expect(body(page)).toContainText("İlk taslak");
  await expect.poll(() => readCalls(page)).toContain(11);
});
