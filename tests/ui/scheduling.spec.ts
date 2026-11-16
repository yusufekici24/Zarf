import { expect, test } from "@playwright/test";
import { calls, fixtures, installMock } from "./mock-backend";

test("gruplar iç içe açılır, kişiler tekilleştirilir; döngü ve erişim hatası boş sayılmaz", async ({ page }) => {
  await installMock(page, fixtures());
  await page.goto("/");
  const result = await page.evaluate(async path => {
    const m = await import(/* @vite-ignore */ path);
    const groups: Record<string, string[]> = {
      "ekip": ["Ali <ali@test.local>", "Alt <alt@test.local>"],
      "alt@test.local": ["ALI@test.local", "Veli <veli@test.local>"],
      "loop@test.local": ["loop@test.local"],
      "empty@test.local": [],
    };
    const directory = {
      groupAddresses: async (emails: string[]) => emails.filter(e => e in groups || e === "fail@test.local"),
      expandGroup: async (email: string) => { if (email === "fail@test.local") throw Error("denied"); return groups[email]; },
    };
    const r = await m.resolveSchedulingPeople([
      { name: "Ekip", addr: "ekip", role: "opt" },
      { name: "Ali", addr: "Ali@test.local", role: "req" },
      { name: "Döngü", addr: "loop@test.local", role: "req" },
      { name: "Erişim", addr: "fail@test.local", role: "req" },
      { name: "Boş grup", addr: "empty@test.local", role: "req" },
    ], directory);
    return { ...r, states: [[0, 0], [0, 2], [0, 4], [], [2, 4]].map(m.combinedAvailability) };
  }, "/src/lib/scheduling.ts");
  expect(result.people.filter((p: { addr: string }) => p.addr === "ali@test.local")).toEqual([{ name: "Ali", addr: "ali@test.local", role: "req" }]);
  expect(result.people.find((p: { addr: string }) => p.addr === "veli@test.local").group).toBe("Ekip");
  expect(result.people.filter((p: { unresolved: boolean }) => p.unresolved)).toHaveLength(3);
  expect(result.warnings).toHaveLength(3);
  expect(result.states).toEqual([0, 2, 4, 4, 4]);
});

test("zamanlayıcı grup üyelerini ve yalnızca merged meşguliyetini gösterir; davet grubunu korur", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", e => errors.push(e.message));
  await installMock(page, fixtures({ window: "w-event" }));
  await page.addInitScript(() => {
    const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> } };
    const original = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === "group_addresses") return (args?.emails as string[]).filter(e => e === "ekip@test.local");
      if (cmd === "expand_group") return ["Ali <ali@test.local>", "Veli <veli@test.local>"];
      if (cmd === "resolve_names") return {};
      if (cmd === "suggest_recipients") return [];
      if (cmd === "free_busy_details") return { "ali@test.local": { merged: "2".repeat(48), events: [] }, "veli@test.local": { merged: "0".repeat(48), events: [] } };
      return original(cmd, args);
    };
  });
  await page.goto("/");
  await page.waitForFunction(() => (window as unknown as { __mock: { listening: (e: string) => boolean } }).__mock.listening("window://assign"));
  await page.evaluate(() => (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("window://assign", {
    kind: "event", payload: { meeting: true, event: { id: 0, calendar_id: 1, title: "Grup testi", start: 1794898800, end: 1794900600, attendees: "Ekip <ekip@test.local>", recurrence: "none", reminder_minutes: 15, show_as: "busy", description: "", location: "", category: "", all_day: false } },
  }));
  await page.getByRole("button", { name: "Zamanlama Yardımcısı", exact: true }).first().click();
  await expect(page.locator('[title="ali@test.local · Ekip"]')).toBeVisible();
  await expect(page.locator('[title="veli@test.local · Ekip"]')).toBeVisible();
  await expect(page.locator('[title="ali@test.local · Ekip"] + div .bg-blue-600').first()).toBeVisible();
  await page.getByRole("button", { name: "Toplantı", exact: true }).click();
  await expect(page.getByText("Ekip", { exact: true }).first()).toBeVisible();
  expect(await calls(page, "save_event")).toEqual([]);
  expect(errors).toEqual([]);
});
