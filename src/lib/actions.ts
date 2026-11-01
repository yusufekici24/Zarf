// Dosya menüsü işlemleri (ana pencere): aç, içe aktar.
import { open as openDialog } from "$lib/dialog";
import { toast } from "svelte-sonner";
import { api } from "$lib/api";
import { app } from "$lib/stores/app.svelte";
import { mail } from "$lib/stores/mail.svelte";
import { t } from "$lib/i18n.svelte";

export async function openFile() {
  const p = await openDialog({
    multiple: true,
    filters: [
      { name: t("filter.all_supported"), extensions: ["msg", "eml", "ics", "vcf", "mbox", "ical", "vcs"] },
      { name: t("filter.outlook_msg"), extensions: ["msg"] },
      { name: t("filter.eml"), extensions: ["eml"] },
      { name: "iCalendar", extensions: ["ics"] },
      { name: "vCard", extensions: ["vcf"] },
    ],
  });
  for (const path of p ? (Array.isArray(p) ? p : [p]) : []) await app.openPath(path);
}

export async function importPst() {
  const p = await openDialog({ filters: [{ name: "PST/OST", extensions: ["pst", "ost"] }] });
  if (!p || typeof p !== "string") return;
  const id = toast.loading(t("import.pst_running"));
  try {
    const r = await api.importPst(p);
    toast.success(t("import.pst_done", r.messages, r.contacts, r.events), { id });
    await app.loadFolders();
    app.bump("mail");
    app.bump("contacts");
    app.bump("calendar");
  } catch (e) {
    toast.error(String(e), { id });
  }
}

export async function importMail() {
  const p = await openDialog({ multiple: true, filters: [{ name: t("filter.mail_files"), extensions: ["eml", "msg", "mbox"] }] });
  const target = mail.folderId ?? app.folderGroups.flatMap((g) => g.folders).find((f) => f.role === "inbox")?.id;
  if (p && target != null) {
    toast.success(t("import.messages_done", await api.importMailFiles(target, Array.isArray(p) ? p : [p])));
    await app.loadFolders();
  }
}
