// İleti listesi: adım Kime/Bilgi satırındaysa renkli satır; ileti sürüklenirken bırakılacak klasörün gösterilmesi
import { expect, test, type Page } from "@playwright/test";
import { calls, fixtures, installMock, type Fixtures, type MockMessage } from "./mock-backend";

const NOW = Math.floor(Date.UTC(2026, 10, 5, 9, 0) / 1000);
const msg = (id: number, subject: string, to: string, cc = ""): MockMessage => ({
  id, folder_id: 1, subject, sender: `Gönderen ${id} <g${id}@kurum.com.tr>`, date: NOW - id * 60, snippet: `Özet ${id}`, is_read: id % 2 === 0, to, cc, html: `<p>${subject}</p>`,
});
const MESSAGES = [
  msg(1, "Doğrudan bana", "Ben <ben@kurum.com.tr>, Ali Veli <ali@kurum.com.tr>"),
  msg(2, "Herkese duyuru", "Tüm Personel <tum@kurum.com.tr>"),
  msg(3, "Bilgide adım var", "Ekip <ekip@kurum.com.tr>", "BEN@Kurum.com.tr"),
  // Exchange listesi yalnızca görünen adları verir (adres ileti açılınca gelir)
  msg(4, "Yalnız ad var", "Ali Veli, Deneme KULLANICI"),
  // Benzer adres ve adlar sayılmaz
  msg(5, "Benzer adres", "Ruben <ruben@kurum.com.tr>, kaben@kurum.com.tr, ben@kurum.com.tr.ornek.org", "Deneme Kullanıcılar"),
];

async function open(page: Page, settings: Record<string, string> = {}, over: Partial<Fixtures> = {}) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await installMock(page, fixtures({ messages: MESSAGES, ...over, settings: { language: "tr", start_module: "mail", reading_pane: "right", sync_on_select: "0", mark_read_delay: "-1", ...settings } }));
  await page.goto("/");
  await expect(page.getByText("Herkese duyuru").first()).toBeVisible();
  return errors;
}

const row = (page: Page, subject: string) => page.getByRole("option").filter({ hasText: subject });
const marked = (page: Page) => page.locator("[role=option][data-to-me]").evaluateAll((els) => els.map((e) => e.querySelectorAll("span.truncate")[1]?.textContent ?? "").sort());

test("vurgu kapalıyken (varsayılan) hiçbir satır işaretlenmez", async ({ page }) => {
  const errors = await open(page);
  expect(await marked(page)).toEqual([]);
  await expect(row(page, "Doğrudan bana").getByText("Doğrudan bana")).toHaveClass(/text-primary/);
  expect(errors).toEqual([]);
});

test("adım Kime ya da Bilgi satırındaysa gönderen ve konu seçilen renkle yazılır", async ({ page }) => {
  const errors = await open(page, { me_highlight: "tocc", me_names: "Deneme Kullanıcı" });
  await expect.poll(() => marked(page)).toEqual(["Bilgide adım var", "Doğrudan bana", "Yalnız ad var"]);
  // Okunmamış: kırmızı ve kalın; okunmuş: kırmızı, kalın değil; ilgisiz ileti eskisi gibi
  const unread = row(page, "Doğrudan bana");
  await expect(unread.getByText("Doğrudan bana")).toHaveClass(/text-\[#c50f1f\]/);
  await expect(unread.getByText("Doğrudan bana")).toHaveClass(/font-semibold/);
  await expect(unread.getByText("Doğrudan bana")).not.toHaveClass(/text-primary/);
  await expect(unread.getByText("Gönderen 1")).toHaveClass(/text-\[#c50f1f\]/);
  await expect(unread.getByText("Gönderen 1")).toHaveClass(/font-bold/);
  const read = row(page, "Yalnız ad var");
  await expect(read.getByText("Yalnız ad var")).toHaveClass(/text-\[#c50f1f\]/);
  await expect(read.getByText("Yalnız ad var")).not.toHaveClass(/font-semibold/);
  await expect(row(page, "Herkese duyuru").getByText("Herkese duyuru")).not.toHaveClass(/text-\[#c50f1f\]/);
  expect(errors).toEqual([]);
});

test("yalnızca Kime ya da yalnızca Bilgi seçilebilir; renk değişir", async ({ page }) => {
  await open(page, { me_highlight: "to", me_color: "green" });
  await expect.poll(() => marked(page)).toEqual(["Doğrudan bana"]);
  await expect(row(page, "Doğrudan bana").getByText("Doğrudan bana")).toHaveClass(/text-\[#107c41\]/);
  await open(page, { me_highlight: "cc" });
  await expect.poll(() => marked(page)).toEqual(["Bilgide adım var"]);
});

test("eşleşme kuralları: tam adres ve tam ad; paylaşılan kutu ve tek sözcüklü ad sayılmaz", async ({ page }) => {
  await open(page);
  const r = await page.evaluate(async (path) => {
    const m: typeof import("../../src/lib/me") = await import(/* @vite-ignore */ path);
    const acc = (email: string, display_name: string, shared_of: number | null = null) => ({ email, display_name, shared_of }) as unknown as Parameters<typeof m.meKeys>[0][number];
    const keys = m.meKeys([acc("Yusuf.Ekici@Kurum.com.tr", "Yusuf EKİCİ"), acc("destek@kurum.com.tr", "Destek Ekibi", 1), acc("tek@kurum.com.tr", "Yusuf")], " Ekici, Yusuf ;ab");
    return {
      keys,
      hit: [
        m.mentionsMe("Ali Veli <ali@kurum.com.tr>, Yusuf Ekici <yusuf.ekici@kurum.com.tr>", keys),
        m.mentionsMe("Ali Veli, YUSUF EKİCİ (BT)", keys),
        m.mentionsMe("Ekici, Yusuf, Veli, Ali", keys),
        m.mentionsMe("TEK@KURUM.COM.TR", keys),
      ],
      miss: [
        m.mentionsMe("Destek Ekibi <destek@kurum.com.tr>", keys),
        m.mentionsMe("Yusuf Kaya <yusuf.kaya@kurum.com.tr>", keys),
        m.mentionsMe("x.yusuf.ekici@kurum.com.tr, yusuf.ekici@kurum.com.tr.tr", keys),
        m.mentionsMe("Yusuf Ekiciler Ltd", keys),
        m.mentionsMe("", keys),
        m.mentionsMe("Yusuf Ekici", []),
      ],
      mode: [m.addressedToMe({ to_addr: "a@b.c", cc: "Yusuf Ekici" }, "to", keys), m.addressedToMe({ to_addr: "a@b.c", cc: "Yusuf Ekici" }, "tocc", keys), m.addressedToMe({ to_addr: "Yusuf Ekici" }, "off", keys)],
    };
  }, "/src/lib/me.ts");
  expect(r.keys).toEqual(["yusuf.ekici@kurum.com.tr", "yusuf ekici", "tek@kurum.com.tr", "ekici, yusuf"]);
  expect(r.hit).toEqual([true, true, true, true]);
  expect(r.miss).toEqual([false, false, false, false, false, false]);
  expect(r.mode).toEqual([false, true, false]);
});

test("Seçenekler: vurgu seçilince renk ve ek ad alanı açılır, Tamam ile kaydedilir", async ({ page }) => {
  const errors = await (async () => {
    const e: string[] = [];
    page.on("pageerror", (x) => e.push(x.message));
    await installMock(page, fixtures({ window: "w-settings" }));
    await page.goto("/");
    return e;
  })();
  await page.waitForFunction(() => (window as unknown as { __mock: { listening: (e: string) => boolean } }).__mock.listening("window://assign"));
  await page.evaluate(() => (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("window://assign", { kind: "settings", payload: { tab: "mail" } }));
  const select = page.locator("select").filter({ has: page.locator('option[value="tocc"]') });
  await expect(select).toHaveCount(1);
  await expect(page.getByTestId("me-names")).toHaveCount(0);
  await select.selectOption("tocc");
  await page.getByTestId("me-names").fill("Deneme Kullanıcı");
  await page.locator("select").filter({ has: page.locator('option[value="orange"]') }).filter({ hasNot: page.locator('option[value="teal"]') }).selectOption("orange");
  await page.getByRole("button", { name: "Tamam" }).click();
  await expect.poll(async () => (await calls(page, "set_setting")).filter((a) => String(a.key).startsWith("me_")).map((a) => `${a.key}=${a.value}`).sort()).toEqual(["me_color=orange", "me_highlight=tocc", "me_names=Deneme Kullanıcı"]);
  expect(errors).toEqual([]);
});

test("ileti sürüklenirken imlecin altındaki klasör vurgulanır, adı etikette görünür ve bırakınca taşınır", async ({ page }) => {
  const errors = await open(page);
  const src = await row(page, "Herkese duyuru").boundingBox();
  const target = page.locator('[data-drop-key="f2"]');
  const other = page.locator('[data-drop-key="f3"]');
  const box = (await target.boundingBox())!;
  await page.mouse.move(src!.x + 120, src!.y + 20);
  await page.mouse.down();
  await page.mouse.move(src!.x + 100, src!.y + 30, { steps: 3 });
  await page.mouse.move(box.x + 40, box.y + box.height / 2, { steps: 5 });
  await expect(target).toHaveClass(/drop-target/);
  await expect(other).not.toHaveClass(/drop-target/);
  await expect(page.getByTestId("drag-target")).toHaveText("→ Gönderilmiş Öğeler");
  // Başka klasöre geçince vurgu da geçer; klasör dışına çıkınca kalkar
  const box3 = (await other.boundingBox())!;
  await page.mouse.move(box3.x + 40, box3.y + box3.height / 2, { steps: 3 });
  await expect(other).toHaveClass(/drop-target/);
  await expect(target).not.toHaveClass(/drop-target/);
  await page.mouse.move(src!.x + 100, src!.y + 30, { steps: 3 });
  await expect(page.locator(".drop-target")).toHaveCount(0);
  await page.mouse.move(box.x + 40, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
  await expect(page.locator(".drop-target")).toHaveCount(0);
  await expect.poll(async () => await calls(page, "move_messages")).toEqual([{ ids: [2], folderId: 2 }]);
  expect(errors).toEqual([]);
});
