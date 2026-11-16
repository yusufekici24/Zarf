// Yeni ileti penceresi: biçim araçları (vurgu ve yazı rengi, stiller, biçim boyacısı), seçim araç
// çubuğu, bağlantı kartı, tablo sekmesi ve durum çubuğu
import { expect, test, type Locator, type Page } from "@playwright/test";
import { calls, fixtures, installMock } from "./mock-backend";

const LINES = "<div>Birinci satır</div><div>İkinci satır</div><div>Üçüncü satır</div>";
const YELLOW = "rgb(255, 255, 0)";
const CLEAR = "rgba(0, 0, 0, 0)";

async function compose(page: Page, html = LINES) {
  await installMock(page, fixtures({ window: "w-compose" }));
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
  await expect(editor).toContainText(html.replace(/<[^>]+>/g, " ").trim().split(/\s+/)[0]);
  return editor;
}
/** Satırı fareyle seçer (üç tıklama): gerçek kullanıcı gibi, seçim araç çubuğu da çıkar */
const pick = (editor: Locator, text: string) => editor.getByText(text, { exact: true }).click({ clickCount: 3 });
/** Metnin görünen arka planı (vurgu, metni saran öğededir) */
const background = (editor: Locator, text: string) =>
  editor.getByText(text, { exact: true }).last().evaluate((el) => {
    for (let n: Element | null = el; n && !n.classList.contains("rich-editor"); n = n.parentElement) {
      const bg = getComputedStyle(n).backgroundColor;
      if (bg !== "rgba(0, 0, 0, 0)") return bg;
    }
    return "rgba(0, 0, 0, 0)";
  });
/** Yazı boyutu, punto olarak */
const points = (el: Element) => Math.round(parseFloat(getComputedStyle(el).fontSize) * 0.75 * 2) / 2;
/** Satırı baştan sona sürükleyerek seçer (tek fare hareketi) */
async function drag(page: Page, editor: Locator, text: string) {
  const box = (await editor.getByText(text, { exact: true }).evaluate((el) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    const b = r.getBoundingClientRect();
    return { x: b.left, y: b.top, width: b.width, height: b.height };
  }))!;
  await page.mouse.move(box.x + 1, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width + 2, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
}
const ribbon = (page: Page) => page.locator(".ribbon-panel");
const tab = (page: Page, key: string) => page.locator(`[data-ribbon-tab="${key}"]`).click();

test("vurgu rengi art arda seçilen satırlara her seferinde uygulanır", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const editor = await compose(page);
  const highlight = ribbon(page).getByRole("button", { name: "Vurgu rengi", exact: true });

  await pick(editor, "Birinci satır");
  await highlight.click();
  expect(await background(editor, "Birinci satır")).toBe(YELLOW);

  // Aynı renk ikinci kez: eskiden renk kutusu "değişmedi" diye olay göndermiyor, satır boyanmıyordu
  await pick(editor, "İkinci satır");
  await highlight.click();
  expect(await background(editor, "İkinci satır")).toBe(YELLOW);
  expect(await background(editor, "Üçüncü satır")).toBe(CLEAR);

  await pick(editor, "Üçüncü satır");
  await highlight.click();
  expect(await background(editor, "Üçüncü satır")).toBe(YELLOW);
  // Yerel renk kutusu yalnızca palette, "özel renk" için durur
  await expect(ribbon(page).locator('input[type="color"]')).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("palet: seçilen renk düğmenin rengi olur, Renk Yok vurguyu kaldırır", async ({ page }) => {
  const editor = await compose(page);
  const group = ribbon(page).locator('[data-color="back"]');

  await pick(editor, "Birinci satır");
  await group.getByRole("button", { name: "Vurgu rengi: renk seç" }).click();
  await page.getByRole("button", { name: "#00ff00", exact: true }).click();
  expect(await background(editor, "Birinci satır")).toBe("rgb(0, 255, 0)");
  // Seçim palet açıkken de yerinde kaldı; düğme artık yeşil uygular
  await pick(editor, "İkinci satır");
  await group.getByRole("button", { name: "Vurgu rengi", exact: true }).click();
  expect(await background(editor, "İkinci satır")).toBe("rgb(0, 255, 0)");

  await group.getByRole("button", { name: "Vurgu rengi: renk seç" }).click();
  await page.getByRole("button", { name: "Renk Yok" }).click();
  expect(await background(editor, "İkinci satır")).toBe(CLEAR);

  // Özel renk: kod yazılıp uygulanır; aynı kod yeniden uygulanabilir
  for (const line of ["İkinci satır", "Üçüncü satır"]) {
    await pick(editor, line);
    await group.getByRole("button", { name: "Vurgu rengi: renk seç" }).click();
    await page.getByLabel("Renk kodu").fill("#ffcc99");
    await page.getByRole("button", { name: "Uygula" }).click();
    expect(await background(editor, line)).toBe("rgb(255, 204, 153)");
  }
});

test("yazı rengi ve biçim düğmeleri imleçteki biçimi gösterir", async ({ page }) => {
  const editor = await compose(page, "<div><b>Kalın yazı</b></div><div>Düz yazı</div>");
  const bold = ribbon(page).getByRole("button", { name: "Kalın (Ctrl+B)" });
  await pick(editor, "Kalın yazı");
  await expect(bold).toHaveAttribute("aria-pressed", "true");
  await pick(editor, "Düz yazı");
  await expect(bold).toHaveAttribute("aria-pressed", "false");

  await ribbon(page).getByRole("button", { name: "Yazı rengi", exact: true }).click();
  expect(await editor.getByText("Düz yazı").last().evaluate((el) => getComputedStyle(el).color)).toBe("rgb(192, 0, 0)");
  await ribbon(page).getByRole("button", { name: "İki yana yasla" }).click();
  await expect(ribbon(page).getByRole("button", { name: "İki yana yasla" })).toHaveAttribute("aria-pressed", "true");
});

test("seçim araç çubuğu: fareyle seçince çıkar, yazınca kaybolur", async ({ page }) => {
  const editor = await compose(page);
  const bubble = page.getByRole("toolbar", { name: "Seçim araç çubuğu" });
  await expect(bubble).toHaveCount(0);
  await pick(editor, "İkinci satır");
  await expect(bubble).toBeVisible();
  await bubble.getByRole("button", { name: "Kalın (Ctrl+B)" }).click();
  expect(await editor.getByText("İkinci satır").last().evaluate((el) => getComputedStyle(el).fontWeight)).toBe("700");
  await bubble.getByRole("button", { name: "Vurgu rengi", exact: true }).click();
  expect(await background(editor, "İkinci satır")).toBe(YELLOW);
  // Yazı tipi iki kademe büyür (11 → 12 → 14); seçim yerinde kaldığı için düğmeye art arda basılabilir
  await bubble.getByRole("button", { name: /Yazı tipini büyüt/ }).click();
  await bubble.getByRole("button", { name: /Yazı tipini büyüt/ }).click();
  expect(await editor.getByText("İkinci satır").last().evaluate(points)).toBe(14);
  await expect(bubble).toBeVisible();
  await page.keyboard.press("End");
  await expect(bubble).toHaveCount(0);
});

test("punto imleç boşken seçilirse yazılan metin o puntoda olur", async ({ page }) => {
  const editor = await compose(page, "<div>Başlangıç</div>");
  await editor.getByText("Başlangıç").click();
  await page.keyboard.press("End");
  await ribbon(page).getByLabel("Yazı boyutu").selectOption("20");
  await page.keyboard.type(" büyük");
  const size = await editor.evaluate((el) => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) if (n.nodeValue?.includes("büyük")) return Math.round(parseFloat(getComputedStyle(n.parentElement!).fontSize) * 0.75);
    return 0;
  });
  expect(size).toBe(20);
});

test("biçim boyacısı imleçteki biçimi seçilen metne taşır", async ({ page }) => {
  const editor = await compose(page, '<div><b><span style="color:rgb(192, 0, 0)">Kaynak</span></b></div><div>Hedef satır</div>');
  await editor.getByText("Kaynak").click();
  const painter = ribbon(page).getByRole("button", { name: /^Biçim Boyacısı/ });
  await painter.click();
  await expect(editor).toHaveClass(/painting/);
  await drag(page, editor, "Hedef satır");
  const style = await editor.getByText("Hedef satır").last().evaluate((el) => [getComputedStyle(el).fontWeight, getComputedStyle(el).color]);
  expect(style).toEqual(["700", "rgb(192, 0, 0)"]);
  // Tek tıklama bir kez uygular
  await expect(editor).not.toHaveClass(/painting/);
});

test("bağlantı kartı: Ctrl+K ile eklenir, düzenlenir, kaldırılır", async ({ page }) => {
  const editor = await compose(page);
  await pick(editor, "İkinci satır");
  await page.keyboard.press("Control+k");
  const card = page.locator('[data-editor-float="link"]');
  await expect(card.getByRole("textbox").first()).toHaveValue("İkinci satır");
  await expect(card.getByRole("textbox").nth(1)).toBeFocused();
  await page.keyboard.type("zarf.example/yardim");
  await page.keyboard.press("Enter");
  const link = editor.locator("a");
  await expect(link).toHaveAttribute("href", "https://zarf.example/yardim");
  await expect(link).toHaveText("İkinci satır");

  // Bağlantının içindeyken kart var olanı düzenler
  await link.click();
  await page.keyboard.press("Control+k");
  await expect(card.getByRole("textbox").nth(1)).toHaveValue("https://zarf.example/yardim");
  await card.getByRole("textbox").nth(1).fill("destek@kurum.com.tr");
  await card.getByRole("button", { name: "Tamam" }).click();
  await expect(link).toHaveAttribute("href", "mailto:destek@kurum.com.tr");

  await link.click();
  await page.keyboard.press("Control+k");
  await card.getByRole("button", { name: "Bağlantıyı kaldır" }).click();
  await expect(editor.locator("a")).toHaveCount(0);
  await expect(editor).toContainText("İkinci satır");
});

test("stiller ve harf dönüşümü; başlık stili gönderilen iletiye satır içi yazılır", async ({ page }) => {
  const editor = await compose(page, "<div>istanbul ofisi</div><div>Gövde metni</div>");
  await tab(page, "format");
  await pick(editor, "istanbul ofisi");
  await ribbon(page).getByRole("button", { name: "Büyük/küçük harf değiştir" }).click();
  await page.getByRole("menuitem", { name: "Her Sözcüğün İlk Harfi Büyük" }).click();
  await expect(editor).toContainText("İstanbul Ofisi");

  const h1 = ribbon(page).locator('[data-style="h1"]');
  await h1.click();
  await expect(editor.locator("h1")).toHaveText("İstanbul Ofisi");
  await expect(h1).toHaveAttribute("aria-pressed", "true");
  expect(await editor.locator("h1").evaluate(points)).toBe(20);

  await pick(editor, "Gövde metni");
  await ribbon(page).getByRole("button", { name: "Satır aralığı" }).click();
  await page.getByRole("menuitem", { name: "2,0" }).click();
  expect(await editor.getByText("Gövde metni").evaluate((el) => (el.closest("div") as HTMLElement).style.lineHeight)).toBe("2");

  await page.keyboard.press("Control+Enter");
  await expect.poll(async () => (await calls(page, "send_mail")).length).toBe(1);
  const sent = (await calls(page, "send_mail"))[0].mail as { html: string };
  expect(sent.html).toMatch(/<h1 style="font-size:20pt;[^"]*color:#2f5496[^"]*">İstanbul Ofisi<\/h1>/);
  expect(sent.html).toContain("line-height: 2");
});

test("tablo: bağlamsal sekme, satır ekleme, stil, silme; Tab sonraki hücreye geçer", async ({ page }) => {
  const editor = await compose(page, "<div>Önce</div>");
  await editor.getByText("Önce").click();
  await page.keyboard.press("End");
  await expect(page.locator('[data-ribbon-tab="table"]')).toHaveCount(0);
  await tab(page, "insert");
  await ribbon(page).getByRole("button", { name: "Tablo" }).click();
  await page.getByRole("button", { name: "3×2", exact: true }).click();
  const table = editor.locator("table");
  await expect(table.locator("tr")).toHaveCount(2);

  // İmleç yeni tablonun ilk hücresindedir; kenarlık türü satır içinde durur (alıcıda görünsün)
  expect(await table.locator("td").first().evaluate((el: HTMLElement) => el.style.borderStyle)).toBe("solid");
  await page.keyboard.type("Ad");
  await page.keyboard.press("Tab");
  await page.keyboard.type("Tutar");
  await expect(table.locator("td").nth(1)).toHaveText("Tutar");

  await tab(page, "table");
  await ribbon(page).getByRole("button", { name: "Alta Ekle" }).click();
  await expect(table.locator("tr")).toHaveCount(3);
  await ribbon(page).getByRole("button", { name: "Sağa Ekle" }).click();
  await expect(table.locator("tr").first().locator("td")).toHaveCount(4);

  await ribbon(page).locator('[data-table-style="blue"]').click();
  const cell = (r: number) => table.locator("tr").nth(r).locator("td").first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await cell(0)).toBe("rgb(68, 114, 196)");
  expect(await cell(1)).toBe("rgb(217, 226, 243)");
  expect(await cell(2)).toBe(CLEAR);

  // Son hücrede Tab yeni satır açar
  await table.locator("td").last().click();
  await page.keyboard.press("Tab");
  await expect(table.locator("tr")).toHaveCount(4);

  await ribbon(page).getByRole("button", { name: "Tabloyu Sil" }).click();
  await expect(table).toHaveCount(0);
  await editor.getByText("Önce").click();
  await expect(page.locator('[data-ribbon-tab="table"]')).toHaveCount(0);
});

test("durum çubuğu: sözcük sayısı ve yakınlaştırma", async ({ page }) => {
  const editor = await compose(page);
  const status = page.getByTestId("compose-status");
  await expect(status).toContainText("6 sözcük");
  await expect(status).toContainText("34 karakter");
  await pick(editor, "İkinci satır");
  await expect(status).toContainText("2 / 6 sözcük");
  await status.getByRole("button", { name: "Yakınlaştır" }).click();
  await status.getByRole("button", { name: "Yakınlaştır" }).click();
  await expect(status).toContainText("%120");
  expect(await editor.evaluate((el) => el.style.zoom)).toBe("1.2");
  await expect.poll(async () => (await calls(page, "set_setting")).some((c) => c.key === "compose_zoom" && c.value === "120")).toBe(true);
});
