// Geri al / yinele (Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z) ve gönderimi geri alma süresi
import { expect, test, type Page } from "@playwright/test";
import { calls, fixtures, installMock } from "./mock-backend";

async function compose(page: Page, html = "<div>Merhaba</div>", settings: Record<string, string> = {}) {
  await installMock(page, fixtures({ window: "w-compose", settings: { language: "tr", ...settings } }));
  await page.goto("/");
  await page.waitForFunction(() => (window as unknown as { __mock: { listening: (e: string) => boolean } }).__mock.listening("window://assign"));
  await page.evaluate(
    (html) =>
      (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("window://assign", {
        kind: "compose",
        payload: { key: 1, mode: "new", data: { to: "Ayşe Yılmaz <ayse.yilmaz@kurum.com.tr>", subject: "Deneme", html } },
      }),
    html,
  );
  const editor = page.locator(".rich-editor");
  await expect(editor).toContainText("Merhaba");
  return editor;
}
const text = (page: Page) => page.locator(".rich-editor").evaluate((el) => (el as HTMLElement).innerText.replace(/\n+/g, "|").trim());

test("yazılan metin Ctrl+Z ile geri alınır, Ctrl+Y ve Ctrl+Shift+Z ile yinelenir; ara verince yeni adım", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const editor = await compose(page);
  await editor.getByText("Merhaba").click();
  await page.keyboard.press("End");
  await page.keyboard.type(" dünya");
  await page.waitForTimeout(1400);
  await page.keyboard.type(" nasılsın");
  expect(await text(page)).toBe("Merhaba dünya nasılsın");

  await page.keyboard.press("Control+z");
  expect(await text(page)).toBe("Merhaba dünya");
  await page.keyboard.press("Control+z");
  expect(await text(page)).toBe("Merhaba");
  // Başlangıçtan geriye gidilmez
  await page.keyboard.press("Control+z");
  expect(await text(page)).toBe("Merhaba");

  await page.keyboard.press("Control+y");
  expect(await text(page)).toBe("Merhaba dünya");
  await page.keyboard.press("Control+Shift+z");
  expect(await text(page)).toBe("Merhaba dünya nasılsın");
  // İmleç yazılan metnin sonunda: yazmaya kaldığı yerden devam edilir
  await page.keyboard.type("?");
  expect(await text(page)).toBe("Merhaba dünya nasılsın?");
  // Geri alındıktan sonra yazılırsa yinelenecek adımlar düşer
  await page.keyboard.press("Control+z");
  await page.keyboard.type("!");
  await page.keyboard.press("Control+y");
  expect(await text(page)).toBe("Merhaba dünya nasılsın!");
  expect(errors).toEqual([]);
});

test("Enter ayrı adımdır; silme yazmadan ayrı geri alınır", async ({ page }) => {
  const editor = await compose(page);
  await editor.getByText("Merhaba").click();
  await page.keyboard.press("End");
  await page.keyboard.type(" ekip");
  await page.keyboard.press("Enter");
  await page.keyboard.type("Yeni satır");
  await page.keyboard.press("Backspace");
  await page.keyboard.press("Backspace");
  expect(await text(page)).toBe("Merhaba ekip|Yeni sat");
  await page.keyboard.press("Control+z");
  expect(await text(page)).toBe("Merhaba ekip|Yeni satır");
  await page.keyboard.press("Control+z");
  expect(await text(page)).toBe("Merhaba ekip|");
  await page.keyboard.press("Control+z");
  expect(await text(page)).toBe("Merhaba ekip");
  await page.keyboard.press("Control+z");
  expect(await text(page)).toBe("Merhaba");
});

test("DOM'u doğrudan değiştiren komutlar (harf dönüşümü, satır aralığı) da geri alınır ve yinelenir", async ({ page }) => {
  const editor = await compose(page, "<div>Merhaba ekip</div><div>İkinci</div>");
  await page.locator('[data-ribbon-tab="format"]').click();
  await editor.getByText("Merhaba ekip").click({ clickCount: 3 });
  await page.locator(".ribbon-panel").getByRole("button", { name: "Büyük/küçük harf değiştir" }).click();
  await page.getByRole("menuitem", { name: "BÜYÜK HARF" }).click();
  await expect(editor).toContainText("MERHABA EKİP");
  await editor.getByText("İkinci").click({ clickCount: 3 });
  await page.locator(".ribbon-panel").getByRole("button", { name: "Satır aralığı" }).click();
  await page.getByRole("menuitem", { name: "2,0" }).click();
  const lh = () => editor.getByText("İkinci").evaluate((el) => (el.closest("div") as HTMLElement).style.lineHeight);
  expect(await lh()).toBe("2");

  await editor.click();
  await page.keyboard.press("Control+z");
  expect(await lh()).toBe("");
  await page.keyboard.press("Control+z");
  await expect(editor).toContainText("Merhaba ekip");
  await page.keyboard.press("Control+y");
  await expect(editor).toContainText("MERHABA EKİP");
  await page.keyboard.press("Control+y");
  expect(await lh()).toBe("2");
  // Başlık çubuğundaki düğmeler aynı geçmişi kullanır
  await page.getByTitle("Geri al (Ctrl+Z)").click();
  expect(await lh()).toBe("");
  await page.getByTitle("Yinele (Ctrl+Y)").click();
  expect(await lh()).toBe("2");
});

test("geçmiş: imleç konumu, boyut sınırı ve satır başı (saf fonksiyonlar)", async ({ page }) => {
  await installMock(page, fixtures());
  await page.goto("/");
  const r = await page.evaluate(async (path) => {
    const m: typeof import("../../src/lib/editor-history") = await import(/* @vite-ignore */ path);
    const h = new m.EditHistory(3, 1000);
    h.reset("a");
    h.record("ab", [2, 2]);
    h.record("abc", [3, 3]);
    h.record("abcd", [4, 4]); // en eski adım düşer (3 adım sınırı)
    const undo = [h.undo()?.html, h.undo()?.html, h.undo()?.html];
    const big = new m.EditHistory(100, 10);
    big.reset("");
    big.record("x".repeat(6), null);
    big.record("y".repeat(6), null); // toplam uzunluk sınırı: eski adım düşer
    // Seçim ↔ metin konumu; boş satırdaki imleç aynı satıra döner
    const root = document.createElement("div");
    root.innerHTML = "<div>abc</div><div><br></div><div>de<b>f</b></div>";
    document.body.append(root);
    const sel = window.getSelection()!;
    const at = (node: Node, off: number) => {
      const rg = document.createRange();
      rg.setStart(node, off);
      rg.collapse(true);
      sel.removeAllRanges();
      sel.addRange(rg);
      return m.readSelection(root);
    };
    const empty = root.children[1];
    const pos = [at(root.children[0].firstChild!, 2), at(empty, 0), at(root.querySelector("b")!.firstChild!, 1)];
    m.writeSelection(root, pos[1]);
    const back = sel.getRangeAt(0).startContainer === empty;
    root.remove();
    return {
      undo,
      canUndo: h.canUndo,
      bigSize: big.size,
      pos,
      back,
      kinds: ["insertText", "deleteContentBackward", "insertParagraph", "insertFromPaste", undefined].map((k) => m.inputKind(k)),
      closes: [m.closesGroup(null, "type"), m.closesGroup({ kind: "type", at: 0 }, "type", 100), m.closesGroup({ kind: "type", at: 0 }, "delete", 100), m.closesGroup({ kind: "type", at: 0 }, "type", 5000), m.closesGroup({ kind: "type", at: 0 }, "", 100)],
    };
  }, "/src/lib/editor-history.ts");
  expect(r.undo).toEqual(["abc", "ab", undefined]);
  expect(r.canUndo).toBe(false);
  expect(r.bigSize).toBe(2);
  // abc=3, boş satırın <br>'si 1, de=2, f'nin ilk harfi 1
  expect(r.pos).toEqual([[2, 2], [3, 3], [7, 7]]);
  expect(r.back).toBe(true);
  expect(r.kinds).toEqual(["type", "delete", "", "", ""]);
  expect(r.closes).toEqual([false, false, true, true, true]);
});

test("Konu satırında Ctrl+Z geri alır, Ctrl+Y ve Ctrl+Shift+Z yineler", async ({ page }) => {
  await compose(page);
  const subject = page.locator("input.compose-field").last();
  await expect(subject).toHaveValue("Deneme");
  await subject.click();
  await page.keyboard.press("End");
  await page.keyboard.type(" 2");
  await page.keyboard.press("Control+z");
  await expect(subject).toHaveValue("Deneme");
  await page.keyboard.press("Control+y");
  await expect(subject).toHaveValue("Deneme 2");
  await page.keyboard.press("Control+z");
  await expect(subject).toHaveValue("Deneme");
  await page.keyboard.press("Control+Shift+z");
  await expect(subject).toHaveValue("Deneme 2");
});

test("gönderimi geri alma: varsayılan 10 sn; ileti tam o anda gönderilmek üzere Giden Kutusu'na konur", async ({ page }) => {
  await compose(page);
  const before = Date.now() / 1000;
  await page.keyboard.press("Control+Enter");
  await expect.poll(async () => (await calls(page, "send_mail")).length).toBe(1);
  const at = ((await calls(page, "send_mail"))[0].mail as { send_at: number }).send_at;
  expect(at - before).toBeGreaterThan(9.5);
  expect(at - before).toBeLessThan(11.5);
});

test("gönderimi geri alma süresi kapatılınca ileti hemen gönderilir", async ({ page }) => {
  await compose(page, "<div>Merhaba</div>", { undo_send: "0" });
  await page.keyboard.press("Control+Enter");
  await expect.poll(async () => (await calls(page, "send_mail")).length).toBe(1);
  expect(((await calls(page, "send_mail"))[0].mail as { send_at?: number | null }).send_at ?? null).toBeNull();
});

async function mainWindow(page: Page) {
  await installMock(page, fixtures());
  await page.goto("/");
  await expect(page.getByText("Bütçe toplantısı").first()).toBeVisible();
  await page.waitForFunction(() => (window as unknown as { __mock: { listening: (e: string) => boolean } }).__mock.listening("compose://sent"));
}
const sent = (page: Page, undo: number) =>
  page.evaluate((undo) => {
    (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("compose://sent", { id: 77, undo, due: Date.now() / 1000 + undo, sendAt: null, subject: "Deneme" });
  }, undo);

test("ana pencere: geri sayım gösterilir; 'Şimdi gönder' beklemeyi keser", async ({ page }) => {
  await mainWindow(page);
  await sent(page, 20);
  await expect(page.getByText(/İleti (20|19) sn sonra gönderilecek/)).toBeVisible();
  await expect(page.getByText(/İleti 1[0-9] sn sonra gönderilecek/)).toBeVisible({ timeout: 4000 });
  await page.getByRole("button", { name: "Şimdi gönder" }).click();
  await expect.poll(async () => (await calls(page, "retry_outbox")).map((a) => a.id)).toEqual([77]);
  await expect(page.getByText(/sn sonra gönderilecek/)).toHaveCount(0);
  expect(await calls(page, "cancel_outbox")).toEqual([]);
});

test("ana pencere: 'Geri al' iletiyi taslağa döndürür; süre dolunca bildirim kendiliğinden kapanır", async ({ page }) => {
  await mainWindow(page);
  await sent(page, 10);
  await page.getByRole("button", { name: "Geri al" }).click();
  await expect.poll(async () => (await calls(page, "cancel_outbox")).map((a) => a.id)).toEqual([77]);
  expect(await calls(page, "retry_outbox")).toEqual([]);

  // Süre dolunca: gönderimi arka uç yapar, bildirim kapanır ve ikinci bir gönderim isteği atılmaz
  await sent(page, 1);
  await expect(page.getByText(/sn sonra gönderilecek/)).toBeVisible();
  await expect(page.getByText(/sn sonra gönderilecek/)).toHaveCount(0, { timeout: 4000 });
  expect(await calls(page, "retry_outbox")).toEqual([]);
});
