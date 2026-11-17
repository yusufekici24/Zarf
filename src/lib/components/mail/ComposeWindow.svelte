<script lang="ts">
  import { defaultSignature, loadSignatures, signatureBlock } from "$lib/signatures";
  import { UNDO_SEND_DEFAULT } from "$lib/undo-send";
  import { identityByEmail, identityForReply, loadIdentities, type Identity } from "$lib/identities";
  import { loadTemplates, saveTemplates, type Template } from "$lib/templates";
  import { EMOJI } from "$lib/emoji";
  import FindReplaceBar from "./FindReplaceBar.svelte";
  // Yeni ileti / yanıtla / ilet / taslak düzenleme penceresi.
  import * as Dialog from "$lib/components/ui/dialog";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import RichEditor from "$lib/components/common/RichEditor.svelte";
  import AddressInput from "$lib/components/common/AddressInput.svelte";
  import AddressBookPicker from "$lib/components/common/AddressBookPicker.svelte";
  import TableGrid from "$lib/components/common/TableGrid.svelte";
  import DateField from "$lib/components/common/DateField.svelte";
  import TimeField from "$lib/components/common/TimeField.svelte";
  import EditorTools from "$lib/components/editor/EditorTools.svelte";
  import ColorButton from "$lib/components/editor/ColorButton.svelte";
  import { BLOCK_STYLES, TABLE_STYLES, caretState, inlineBlockStyles } from "$lib/editor";
  import { fontCss, fontStack, parseFont } from "$lib/fonts";
  import { app, type ComposeRequest } from "$lib/stores/app.svelte";
  import { api, errorText } from "$lib/api";
  import { i18n, t } from "$lib/i18n.svelte";
  import { escapeHtml, fmtDateTime, fmtSize, splitAddresses, textToHtml, toInputTime } from "$lib/format";
  import Ribbon from "$lib/components/ribbon/Ribbon.svelte";
  import WindowTitleBar from "$lib/components/common/WindowTitleBar.svelte";
  import { windowFocus } from "$lib/stores/focus.svelte";
  import RibbonGroup from "$lib/components/ribbon/RibbonGroup.svelte";
  import RibbonStack from "$lib/components/ribbon/RibbonStack.svelte";
  import RibbonButton from "$lib/components/common/RibbonButton.svelte";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Copy from "@lucide/svelte/icons/copy";
  import Code from "@lucide/svelte/icons/code";
  import Type from "@lucide/svelte/icons/type";
  import UserCheck from "@lucide/svelte/icons/user-check";
  import TableIcon from "@lucide/svelte/icons/table";
  import ImageIcon from "@lucide/svelte/icons/image";
  import LinkIcon from "@lucide/svelte/icons/link";
  import Minus from "@lucide/svelte/icons/minus";
  import Users from "@lucide/svelte/icons/users";
  import MailCheck from "@lucide/svelte/icons/mail-check";
  import Lock from "@lucide/svelte/icons/lock";
  import Shield from "@lucide/svelte/icons/shield";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import SpellCheck from "@lucide/svelte/icons/spell-check";
  import TextSelect from "@lucide/svelte/icons/text-select";
  import Search from "@lucide/svelte/icons/search";
  import FileCode from "@lucide/svelte/icons/file-code";
  import FileText from "@lucide/svelte/icons/file-text";
  import Smile from "@lucide/svelte/icons/smile";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { mail } from "$lib/stores/mail.svelte";
  import type { Contact, ContactGroup, MessageRow, OutAttachment } from "$lib/types";
  import Send from "@lucide/svelte/icons/send";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import PenLine from "@lucide/svelte/icons/pen-line";
  import Save from "@lucide/svelte/icons/save";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import AlertCircle from "@lucide/svelte/icons/circle-alert";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import Clock from "@lucide/svelte/icons/clock";
  import BookUser from "@lucide/svelte/icons/book-user";
  import X from "@lucide/svelte/icons/x";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import Mail from "@lucide/svelte/icons/mail";
  import Paintbrush from "@lucide/svelte/icons/paintbrush";
  import PaintBucket from "@lucide/svelte/icons/paint-bucket";
  import CalendarClock from "@lucide/svelte/icons/calendar-clock";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Rows3 from "@lucide/svelte/icons/rows-3";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import MoveHorizontal from "@lucide/svelte/icons/move-horizontal";
  import Shrink from "@lucide/svelte/icons/shrink";
  import Plus from "@lucide/svelte/icons/plus";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileImage from "@lucide/svelte/icons/file-image";
  import FileArchive from "@lucide/svelte/icons/file-archive";
  import FileSpreadsheet from "@lucide/svelte/icons/file-spreadsheet";
  import Presentation from "@lucide/svelte/icons/presentation";
  import { open as openDialog } from "$lib/dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { emitTo } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { onDestroy, onMount, untrack } from "svelte";
  import { toast } from "svelte-sonner";

  let {
    req,
    inline = false,
    onclosed,
  }: {
    req: ComposeRequest;
    /** Okuma bölmesinin içinde yanıt (Outlook satır içi yanıt) */
    inline?: boolean;
    onclosed?: () => void;
  } = $props();

  let accountId = $state<number | null>(app.defaultAccountId());
  let to = $state("");
  let cc = $state("");
  let bcc = $state("");
  let showCc = $state(false);
  let showBcc = $state(false);
  let subject = $state("");
  let html = $state("");
  let attachments = $state<OutAttachment[]>([]);
  let importance = $state(1);
  let readReceipt = $state(false);
  // İmzala / Şifrele: varsayılanlar hesabın güvenlik ayarlarından gelir
  let sign = $state(false);
  let encrypt = $state(false);
  let secMethod = $state<"smime" | "pgp">("smime");
  /** Şifreli iletiye yanıt: kaynağın yöntemi korunur */
  let secForced: "smime" | "pgp" | null = null;
  $effect(() => {
    const aid = accountId;
    untrack(() => {
      if (!aid) return;
      secMethod = secForced ?? (app.setting(`security.method.${aid}`, "smime") === "pgp" ? "pgp" : "smime");
      sign = app.setting(`security.sign.${aid}`, "0") === "1";
    });
  });
  let plain = $state(app.setting("compose_format", "html") === "plain");
  let inReplyTo = "";
  let references = "";
  let draftId = $state<number | null>(null);
  let sendAt = $state<number | null>(null);
  let dirty = $state(false);
  let ready = $state(false);
  let closing = false;
  let delayOpen = $state(false);
  let delayDate = $state<number | null>(null);
  let delayTime = $state("08:00");
  let bookOpen = $state(false);
  let lastSaved = $state<number | null>(null);
  let editor: RichEditor | undefined = $state();
  /** Seçili takma ad (null: hesabın kendi adresi) */
  let identity = $state<Identity | null>(null);
  let findOpen = $state(false);
  let sourceOpen = $state(false);
  let sourceHtml = $state("");
  /** Çökme kurtarma kaydının kimliği (geri yüklenen iletide aynısı sürer) */
  const recoveryId = req.recoveryId ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

  /** Outlook: yeni iletide "Yeni iletiler", yanıt/iletmede "Yanıtlar/İletmeler" imzası */
  const sigKind = req.mode === "new" ? "new" : "reply";

  /** Takma adın kendi imzası varsa o, yoksa hesabın varsayılan imzası */
  function applySignature(aid: number | null) {
    putSignature(signatureBlock(identity?.signature || defaultSignature(aid, sigKind)));
  }

  /** Kimden menüsü: "<hesap>" ya da "<hesap>:<takma ad>" */
  function pickFrom(value: string) {
    const [acc, ...rest] = value.split(":");
    accountId = Number(acc);
    identity = identityByEmail(accountId, rest.join(":"));
    applySignature(accountId);
  }

  /** İmza bloğunu değiştirir/ekler (iç içe öğeli zengin imzalar için DOM üzerinden) */
  function putSignature(sig: string) {
    if (editor) html = editor.getHtml();
    const box = document.createElement("div");
    box.innerHTML = html;
    // (eski adla kaydedilmiş taslaklardaki "mailnix-" işaretleri de tanınır)
    const old = box.querySelector(".lynora-signature, .mailnix-signature");
    const tpl = document.createElement("template");
    tpl.innerHTML = sig;
    const fresh = tpl.content.firstElementChild;
    if (old) {
      if (fresh) old.replaceWith(fresh);
      else old.remove();
    } else if (fresh) {
      const quote = box.querySelector(".lynora-quote, .mailnix-quote");
      const gap = document.createElement("div");
      gap.innerHTML = "<br>";
      if (quote) quote.before(fresh);
      else box.append(gap, fresh);
    }
    html = box.innerHTML;
  }

  onMount(async () => {
    try {
      if (req.sourceId != null && req.mode !== "new") {
        const d = await api.composePrefill(req.mode, req.sourceId);
        to = d.to;
        cc = d.cc;
        bcc = d.bcc;
        subject = d.subject;
        html = d.html;
        inReplyTo = d.in_reply_to;
        references = d.references;
        attachments = d.attachments;
        draftId = d.draft_id;
        importance = d.importance || 1;
        if (d.account_id && app.accounts.some((a) => a.id === d.account_id)) accountId = d.account_id;
        identity = identityByEmail(accountId, d.from_addr ?? "") ?? identityForReply(accountId, d.recipients ?? "");
        if (req.mode !== "draft" && req.mode !== "edit") {
          applySignature(accountId);
          if (d.security?.includes("encrypted")) {
            encrypt = true;
            secForced = d.security.startsWith("pgp") ? "pgp" : "smime";
            secMethod = secForced;
          }
        }
      } else {
        const d = req.data ?? {};
        to = d.to ?? "";
        cc = d.cc ?? "";
        bcc = d.bcc ?? "";
        subject = d.subject ?? "";
        html = (d.html ?? "") || "<div><br></div>";
        inReplyTo = d.in_reply_to ?? "";
        references = d.references ?? "";
        if (d.draft_id) draftId = d.draft_id;
        if (d.importance) importance = d.importance;
        if (d.account_id && app.accounts.some((a) => a.id === d.account_id)) accountId = d.account_id;
        identity = identityByEmail(accountId, d.from_addr ?? "");
        // Satır içi yanıttan yeni pencereye alınan ya da geri yüklenen ileti zaten imzalıdır
        if (!req.recoveryId && !/(?:lynora|mailnix)-signature/.test(d.html ?? "")) applySignature(accountId);
      }
      attachments = [...attachments, ...(req.attachments ?? [])];
      showCc = !!cc;
      showBcc = !!bcc;
    } catch (e) {
      app.error(e);
    }
    ready = true;
    // İlk doldurma "değişiklik" sayılmaz (boş pencereyi kapatırken soru sorulmaz)
    setTimeout(() => (dirty = false), 0);
    activity();
    setTimeout(() => (to ? editor?.focus() : document.getElementById(`to-${req.key}`)?.focus()), 50);
  });

  if (!inline) windowFocus.start();

  // Pencere başlığı = konu (Outlook gibi: "Adsız - İleti (HTML)")
  const winTitle = $derived(`${subject || t("compose.untitled_title")} - ${plain ? t("compose.window_plain") : t("compose.window_html")}`);
  let titleTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const s = winTitle;
    clearTimeout(titleTimer);
    if (!inline) titleTimer = setTimeout(() => void getCurrentWindow().setTitle(s).catch(() => {}), 200);
  });

  // Pencerenin kendi kapatma düğmesi: kaydedilmemiş değişiklik varsa sorulur
  onMount(() => {
    if (inline) return;
    let off: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested(async (e) => {
        if (closing) return;
        e.preventDefault();
        await close();
      })
      .then((f) => (off = f))
      .catch(() => {});
    return () => off?.();
  });

  // Dosya sürükle-bırak ile ek
  let unlisten: (() => void) | null = null;
  // Okuma bölmesindeki satır içi yanıtta (ana pencere) yalnızca yanıt alanının üzerine
  // bırakılan dosyalar eklenir; ayrı pencerede pencerenin her yerine bırakılabilir
  let rootEl: HTMLElement | undefined = $state();
  let fileHover = $state(false);
  function overCompose(pos: { x: number; y: number }): boolean {
    if (!inline) return true;
    if (!rootEl) return false;
    const r = rootEl.getBoundingClientRect();
    const k = window.devicePixelRatio || 1;
    const x = pos.x / k;
    const y = pos.y / k;
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }
  onMount(async () => {
    try {
      unlisten = await getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "over") fileHover = inline && overCompose(p.position);
        else if (p.type === "drop") {
          fileHover = false;
          if (p.paths.length && overCompose(p.position)) void addPaths(p.paths);
        } else fileHover = false;
      });
    } catch {
      /* tarayıcıda çalışırken */
    }
  });

  // Ana penceredeki listeden sürüklenen iletiler .msg eki olarak eklenir (Outlook gibi)
  let dropHover = $state(false);
  async function addMessages(ids: number[]) {
    for (const id of ids) {
      try {
        const f = await api.messageTempFile(id, "msg");
        attachments = [...attachments, { name: f.name, mime: f.mime, size: f.size, path: f.path }];
      } catch (e) {
        app.error(e);
      }
    }
  }
  onMount(() => {
    const offs: (() => void)[] = [];
    void getCurrentWebviewWindow().listen<boolean>("compose://drag-hover", (e) => (dropHover = e.payload)).then((f) => offs.push(f));
    void getCurrentWebviewWindow().listen<number[]>("compose://drop-messages", (e) => {
      dropHover = false;
      void addMessages(e.payload);
    }).then((f) => offs.push(f));
    return () => offs.forEach((f) => f());
  });

  // Otomatik taslak kaydı
  const hasContent = () => !!(subject.trim() || to.trim() || currentHtml().replace(/<[^>]+>/g, "").trim());
  // (Şifrele açıkken yapılmaz: yanıttaki çözülmüş alıntı sunucuya düz metin gitmesin)
  const autosave = setInterval(() => {
    if (dirty && ready && hasContent() && !encrypt) void saveDraft(true);
  }, 60000);
  // Çökme kurtarma: kaydedilmemiş içerik diske yazılır; gönderilince, taslak kaydedilince ya da
  // pencere kapanınca silinir. Uygulama çökerse açılışta geri yükleme önerilir.
  const recovery = setInterval(() => {
    if (!dirty || !ready || !hasContent() || encrypt) return;
    const data = { id: recoveryId, account_id: accountId, from_addr: identity?.email ?? "", to, cc, bcc, subject, html: currentHtml(), in_reply_to: inReplyTo, references, draft_id: draftId, importance, attachments: $state.snapshot(attachments), saved_at: Date.now() / 1000 };
    void api.saveRecovery(recoveryId, JSON.stringify(data)).catch(() => {});
  }, 20000);
  onDestroy(() => {
    clearInterval(autosave);
    clearInterval(recovery);
    unlisten?.();
  });

  $effect(() => {
    void [to, cc, bcc, subject, attachments.length, importance];
    if (ready) dirty = true;
  });

  async function addPaths(paths: string[]) {
    for (const p of paths) {
      try {
        const info = await api.fileInfo(p);
        attachments = [...attachments, { name: info.name, mime: info.mime, size: info.size, path: p }];
      } catch (e) {
        app.error(e);
      }
    }
  }

  // ---------------------------------------------------------------- Öğe Ekle → Outlook öğesi
  let itemOpen = $state(false);
  let itemQuery = $state("");
  let itemRows = $state<MessageRow[]>([]);
  let itemPicked = $state<Set<number>>(new Set());
  let itemTimer: ReturnType<typeof setTimeout> | undefined;
  async function searchItems() {
    try {
      const r = await api.listMessages(null, itemQuery, "all", { by: "date", desc: true, conversations: false }, 0, 100);
      itemRows = r.rows;
    } catch (e) {
      app.error(e);
    }
  }
  function openItemPicker() {
    itemOpen = true;
    itemPicked = new Set();
    void searchItems();
  }
  async function attachItems() {
    for (const id of itemPicked) {
      try {
        const f = await api.messageTempFile(id, "msg");
        attachments = [...attachments, { name: f.name, mime: f.mime, size: f.size, path: f.path }];
      } catch (e) {
        app.error(e);
      }
    }
    itemOpen = false;
  }

  async function attach() {
    const r = await openDialog({ multiple: true });
    if (r) await addPaths(Array.isArray(r) ? r : [r]);
  }

  /** Düzenleyicideki güncel HTML (yazarken her tuşta seri hale getirilmez) */
  function currentHtml(): string {
    return editor?.getHtml() ?? html;
  }

  /**
   * Gönderilecek HTML: Outlook/Word gibi paragraflar arasında boşluk olmaz
   * (p için margin:0); tek Enter tek satır, boş satır yalnızca gerçekten boş satırdır.
   */
  function outgoingHtml(src: string): string {
    const tmp = document.createElement("div");
    tmp.innerHTML = src;
    tmp.querySelectorAll("p").forEach((p) => {
      if (!p.style.margin) p.style.margin = "0";
    });
    // Seçilen yazı tipine yedekler eklenir (alıcıda yoksa bozulmaz)
    tmp.querySelectorAll("font[face]").forEach((f) => {
      const face = (f.getAttribute("face") ?? "").split(",")[0].replace(/['"]/g, "").trim();
      if (!face) return;
      const span = document.createElement("span");
      span.style.fontFamily = fontStack(face);
      const size = f.getAttribute("size");
      const color = f.getAttribute("color");
      if (!size && !color) {
        // (punto verilmişse satır içi stili de taşınır)
        const own = f.getAttribute("style");
        if (own) span.setAttribute("style", `${own};font-family:${fontStack(face)}`);
        span.append(...f.childNodes);
        f.replaceWith(span);
      } else {
        f.removeAttribute("face");
        f.setAttribute("style", `${f.getAttribute("style") ?? ""};font-family:${fontStack(face)}`);
      }
    });
    // Punto verilen metin düzenleyicide <font style> olarak durur; alıcıya span gider
    tmp.querySelectorAll("font[style]:not([face]):not([size]):not([color])").forEach((f) => {
      const span = document.createElement("span");
      span.setAttribute("style", f.getAttribute("style") ?? "");
      span.append(...f.childNodes);
      f.replaceWith(span);
    });
    // Paragraf stilleri (Başlık 1, Alıntı, Kod …) alıcıda da aynı görünsün
    inlineBlockStyles(tmp);
    return tmp.innerHTML;
  }

  /** HTML → düz metin: blok öğeler satır sonu, <br> satır sonu (çift sayılmaz). */
  function htmlToText(src: string): string {
    const tmp = document.createElement("div");
    tmp.innerHTML = src;
    let out = "";
    const BLOCK = /^(P|DIV|LI|TR|H[1-6]|BLOCKQUOTE|PRE|TABLE|UL|OL|HR)$/;
    const nl = () => {
      if (out && !out.endsWith("\n")) out += "\n";
    };
    const walk = (n: Node) => {
      if (n.nodeType === Node.TEXT_NODE) {
        out += (n.textContent ?? "").replace(/\s+/g, " ");
        return;
      }
      if (n.nodeType !== Node.ELEMENT_NODE) return;
      const el = n as HTMLElement;
      if (el.tagName === "BR") {
        out += "\n";
        return;
      }
      if (el.tagName === "STYLE" || el.tagName === "SCRIPT") return;
      const block = BLOCK.test(el.tagName);
      if (block) nl();
      if (el.tagName === "LI") out += "• ";
      el.childNodes.forEach(walk);
      if (el.tagName === "TD") out += "\t";
      if (block) nl();
    };
    tmp.childNodes.forEach(walk);
    return out.replace(/[ \t]+\n/g, "\n").replace(/\n{3,}/g, "\n\n").trim();
  }

  function payload() {
    const h = currentHtml();
    html = h;
    const bodyHtml = plain ? "" : `<div style="${fontCss(fontSpec).replace(/"/g, "'")}">${outgoingHtml(h)}</div>`;
    const text = htmlToText(h);
    return {
      account_id: accountId,
      to, cc, bcc, subject,
      text,
      html: bodyHtml,
      attachments: attachments.map((a) => ({ name: a.name, mime: a.mime, path: a.path ?? null, source_msg: a.source_msg ?? null, source_index: a.source_index ?? null })),
      from_email: identity?.email ?? "",
      from_name: identity?.name ?? "",
      reply_to: identity?.reply_to ?? "",
      in_reply_to: inReplyTo,
      references,
      importance,
      read_receipt: readReceipt,
      sign,
      encrypt,
      security: secMethod,
      send_at: sendAt,
      draft_id: draftId,
    };
  }

  /** "Ad <adres>" ya da yalnızca adres; grup adları (adres içermeyen) olduğu gibi */
  function addrOfEntry(x: string): string {
    return (x.match(/<([^>]+)>/)?.[1] ?? x).trim();
  }

  async function send() {
    if (!accountId) {
      toast.error(t("compose.no_account"));
      app.openAccountDialog();
      return;
    }
    if (![to, cc, bcc].some((x) => x.includes("@"))) {
      toast.error(t("error.no_recipients"));
      return;
    }
    // Geçersiz adres (ör. "ali@" ya da çözümlenemeyen bir ad): gönderilmez, hangisi olduğu söylenir
    const entries = [to, cc, bcc].flatMap((x) => splitAddresses(x)).map((x) => x.trim()).filter(Boolean);
    // Adres içermeyenler yerel kişi grubu adı olabilir (gönderirken üyelerine açılır)
    const groups = entries.some((x) => !x.includes("@")) ? new Set((await api.listGroups().catch(() => [])).map((g) => g.name.trim().toLocaleLowerCase())) : new Set<string>();
    const bad = entries.filter((x) => (x.includes("@") ? !/^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$/.test(addrOfEntry(x)) : !groups.has(x.toLocaleLowerCase())));
    if (bad.length) {
      await app.confirm(t("compose.bad_address"), t("compose.bad_address_text", bad.slice(0, 5).join("; ")), { ok: t("common.ok"), cancel: "" });
      return;
    }
    const sendOpts = { ok: t("compose.send_anyway"), cancel: t("compose.dont_send"), safe: true };
    if (!subject.trim() && !(await app.confirm(t("compose.no_subject"), t("compose.no_subject_text"), sendOpts))) return;
    // Yalnızca kullanıcının yazdığı kısım denetlenir (alıntı ve imza hariç)
    const own = currentHtml().split(/<div class="(?:lynora|mailnix)-(?:signature|quote)">/)[0];
    const plainText = own.replace(/<[^>]+>/g, " ").toLocaleLowerCase();
    // Boş ileti (metin de ek de yok)
    const hasText = plainText.replace(/&nbsp;|\s/g, "").length > 0 || /<img\b/i.test(own);
    if (!hasText && !attachments.length && !(await app.confirm(t("compose.empty_body"), t("compose.empty_body_text"), sendOpts))) return;
    // Sunucuların çoğu 25 MB'tan büyük iletiyi reddeder
    const total = attachments.reduce((n, a) => n + (a.size ?? 0), 0);
    if (total > 25 * 1024 * 1024 && !(await app.confirm(t("compose.too_large"), t("compose.too_large_text", Math.round(total / 1048576)), sendOpts))) return;
    try {
      // Outlook'taki "Gönderimi geri al": ileti kısa bir süre giden kutusunda bekletilir
      // (imzalı ya da şifreli ileti düzenlenemez: taslağa dönemeyeceği için bekletilmez)
      // Süre dolunca arka uç iletiyi tam zamanında gönderir (sync.rs send_later); ana penceredeki
      // bildirim yalnızca geri sayımı ve "Geri al" / "Şimdi gönder" düğmelerini gösterir
      const undo = sendAt || sign || encrypt ? 0 : Number(app.setting("undo_send", UNDO_SEND_DEFAULT)) || 0;
      const p = payload();
      const due = undo > 0 ? Date.now() / 1000 + undo : null;
      if (due) p.send_at = due;
      const id = await api.sendMail(p);
      dirty = false;
      // Gönderim bildirimi ve "geri al" ana pencerede gösterilir; bu pencere hemen kapanır
      await emitTo("main", "compose://sent", { id, undo, due, sendAt, subject }).catch(() => {});
      close(true);
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  async function saveDraft(silent = false) {
    try {
      draftId = await api.saveDraft(payload());
      dirty = false;
      lastSaved = Date.now() / 1000;
      void api.removeRecovery(recoveryId).catch(() => {});
      if (!silent) toast.success(t("compose.draft_saved"));
    } catch (e) {
      if (!silent) app.error(e);
    }
  }

  async function discard() {
    if (!(await app.confirm(t("compose.discard_confirm"), undefined, { ok: t("compose.discard"), destructive: true }))) return;
    if (draftId) await api.discardDraft(draftId);
    dirty = false;
    close(true);
  }

  async function close(force = false) {
    if (!force && dirty) {
      const r = await app.choose(t("compose.save_changes"), t("compose.save_changes_text"), t("compose.save_draft"), t("compose.dont_save"));
      if (r === "cancel") return;
      if (r === "ok") await saveDraft(true);
    }
    closing = true;
    await api.removeRecovery(recoveryId).catch(() => {});
    if (inline) {
      onclosed?.();
      return;
    }
    await api.recycleWindow().catch(() => getCurrentWindow().destroy().catch(() => window.close()));
  }

  /** Satır içi yanıtı ayrı pencereye al (Outlook "Yeni Pencerede Aç") */
  function popOut() {
    const h = currentHtml();
    app.compose({
      mode: "new",
      data: { account_id: accountId, from_addr: identity?.email ?? "", to, cc, bcc, subject, html: h, in_reply_to: inReplyTo, references, draft_id: draftId, importance },
      attachments: [...attachments],
    });
    dirty = false;
    closing = true;
    void api.removeRecovery(recoveryId).catch(() => {});
    onclosed?.();
  }

  // ---------------------------------------------------------------- şablonlar
  function applyTemplate(tp: Template) {
    if (!subject.trim()) subject = tp.subject;
    if (plain) editor?.exec("insertText", htmlToText(tp.html));
    else editor?.insertHtml(tp.html);
    dirty = true;
  }

  /** Yalnızca kullanıcının yazdığı kısım şablon olur (imza ve alıntı hariç) */
  async function saveAsTemplate() {
    const name = (await app.prompt(t("tpl.save_title"), t("sig.name_label"), subject))?.trim();
    if (!name) return;
    const box = document.createElement("div");
    box.innerHTML = currentHtml();
    box.querySelectorAll(".lynora-signature, .mailnix-signature, .lynora-quote, .mailnix-quote").forEach((n) => n.remove());
    const list = loadTemplates();
    const same = list.some((x) => x.name.toLocaleLowerCase() === name.toLocaleLowerCase());
    if (same && !(await app.confirm(t("tpl.exists", name), t("tpl.overwrite"), { ok: t("common.save") }))) return;
    await saveTemplates([...list.filter((x) => x.name.toLocaleLowerCase() !== name.toLocaleLowerCase()), { name, subject, html: box.innerHTML }]);
    toast.success(t("tpl.saved", name));
  }

  // ---------------------------------------------------------------- HTML kaynağı
  function openSource() {
    sourceHtml = currentHtml();
    sourceOpen = true;
  }

  function applySource() {
    html = sourceHtml;
    dirty = true;
    sourceOpen = false;
  }

  // Satır içi yanıt açıkken başka ileti seçilirse taslak sessizce kaydedilip kapanır
  if (inline) {
    mail.inlineGuard = async () => {
      if (dirty) await saveDraft(true);
      closing = true;
      onclosed?.();
    };
  }
  onDestroy(() => {
    if (inline && mail.inlineGuard) mail.inlineGuard = null;
  });

  function openDelay() {
    delayDate = sendAt ?? Math.floor(Date.now() / 1000) + 3600;
    delayTime = toInputTime(sendAt ?? Math.floor(Date.now() / 1000) + 3600);
    delayOpen = true;
  }

  // Ayarlar > Posta > Kırtasiye ve Yazı Tipleri: yeni ileti / yanıt-iletme / düz metin
  const isReply = ["reply", "reply_all", "forward"].includes(req.mode);
  const fontSpec = $derived(plain ? parseFont(app.setting("font_plain", ""), "font_plain") : parseFont(app.setting(isReply ? "font_reply" : "font_new", ""), isReply ? "font_reply" : "font_new"));
  /** İmleçteki biçim (şeritteki kutular ve düğmeler gösterir) */
  let caret = $state(caretState());
  /** Şeridin açık sekmesi; imleç tablodayken bağlamsal "Tablo" sekmesi de görünür */
  let ribbonTab = $state("message");
  $effect(() => {
    if (ribbonTab === "table" && (!caret.table || plain)) ribbonTab = "message";
  });
  const hold = (e: Event) => e.preventDefault();
  let tableOpen = $state(false);
  /** Stil galerisindeki düğmelerin önizlemesi */
  const STYLE_PREVIEW: Record<string, string> = {
    div: "font-size:12px",
    h1: "font-size:15px;color:#2f5496",
    h2: "font-size:13px;color:#2f5496",
    h3: "font-size:12px;font-weight:bold;color:#1f3763",
    blockquote: "font-size:12px;color:#666;border-left:3px solid #c7c7c7;padding-left:6px",
    pre: "font-family:monospace;font-size:11px;background:#f3f3f3;color:#333;padding:0 4px;border-radius:2px",
  };

  // ---------------------------------------------------------------- durum çubuğu
  let counts = $state({ words: 0, chars: 0, selected: 0 });
  let statTimer: ReturnType<typeof setTimeout> | undefined;
  /** Sözcük sayısı yazarken her tuşta değil, kısa bir duraklamadan sonra yenilenir */
  function activity() {
    clearTimeout(statTimer);
    statTimer = setTimeout(() => (counts = editor?.stats() ?? counts), 250);
  }
  let zoom = $state(Math.min(200, Math.max(50, Number(app.setting("compose_zoom", "100")) || 100)));
  let zoomTimer: ReturnType<typeof setTimeout> | undefined;
  function setZoom(v: number) {
    zoom = Math.min(200, Math.max(50, Math.round(v / 10) * 10));
    clearTimeout(zoomTimer);
    zoomTimer = setTimeout(() => void app.setSetting("compose_zoom", String(zoom)), 600);
  }

  // ---------------------------------------------------------------- ek kartları
  const FILE_KINDS: [RegExp, typeof FileIcon, string][] = [
    [/\.(png|jpe?g|gif|bmp|webp|svg|tiff?|heic)$/i, FileImage, "#8764b8"],
    [/\.pdf$/i, FileText, "#d13438"],
    [/\.(docx?|odt|rtf)$/i, FileText, "#2b579a"],
    [/\.(xlsx?|ods|csv)$/i, FileSpreadsheet, "#217346"],
    [/\.(pptx?|odp)$/i, Presentation, "#c43e1c"],
    [/\.(zip|rar|7z|tar|gz|bz2|xz)$/i, FileArchive, "#986f0b"],
    [/\.(msg|eml)$/i, Mail, "#0f6cbd"],
  ];
  const fileKind = (name: string) => FILE_KINDS.find(([re]) => re.test(name)) ?? ([/./, FileIcon, "#69797e"] as (typeof FILE_KINDS)[number]);
  const totalSize = $derived(attachments.reduce((n, a) => n + (a.size ?? 0), 0));

  const fromLabel = $derived.by(() => {
    if (identity) return identity.email;
    const a = app.accounts.find((x) => x.id === accountId);
    return a ? a.email : t("compose.no_account_option");
  });

  /** Düz metne geçiş: biçimlendirme atılır */
  function toPlain() {
    if (plain) return;
    html = escapeHtml(htmlToText(currentHtml())).replace(/\n/g, "<br>");
    plain = true;
  }

  async function paste() {
    editor?.focus();
    try {
      const text = await navigator.clipboard.readText();
      if (text) editor?.exec("insertText", text);
    } catch {
      editor?.exec("paste");
    }
  }

  /** Outlook "Adları Denetle": adres olmayan girdileri kişi/dizin aramasıyla çözer */
  async function checkNames() {
    const bad: string[] = [];
    const fix = async (v: string) => {
      const out: string[] = [];
      for (const e of splitAddresses(v)) {
        if (/@[^\s@]+\.[^\s@]+/.test(e)) {
          out.push(e);
          continue;
        }
        const r = await api.suggestRecipients(e, true).catch(() => []);
        if (r[0]) out.push(r[0].name && r[0].name !== r[0].email ? `${r[0].name} <${r[0].email}>` : r[0].email);
        else {
          out.push(e);
          bad.push(e);
        }
      }
      return out.join(", ");
    };
    to = await fix(to);
    cc = await fix(cc);
    bcc = await fix(bcc);
    if (bad.length) toast.error(t("compose.names_bad", bad.join(", ")));
    else toast.success(t("compose.names_ok"));
  }

  function setDelay() {
    sendAt = null;
    if (delayDate) {
      const d = new Date(delayDate * 1000);
      const [h, m] = delayTime.split(":").map(Number);
      d.setHours(h, m, 0, 0);
      sendAt = Math.floor(d.getTime() / 1000);
    }
    delayOpen = false;
  }

  function onkeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      void send();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void saveDraft();
    } else if ((e.ctrlKey || e.metaKey) && ["f", "h"].includes(e.key.toLowerCase())) {
      e.preventDefault();
      findOpen = true;
    } else if (e.key === "Escape" && !e.defaultPrevented && !delayOpen && !bookOpen && !sourceOpen && !app.confirmReq && !app.promptReq && !document.querySelector("[data-state=open][role=menu], [data-slot=popover-content], [data-editor-float]")) {
      e.preventDefault();
      void close();
    }
  }

  // ---------------------------------------------------------------- adres defteri
  let bookSearch = $state("");
  let contacts = $state<Contact[]>([]);
  let groups = $state<ContactGroup[]>([]);
  let picked = $state<Set<string>>(new Set());

  // Outlook "Ad Seç": Kime / Bilgi / Gizli alanlarıyla genel adres listesi
  let pickerOpen = $state(false);
  function openBook() {
    pickerOpen = true;
  }
  function applyPicked(r: Record<string, string[]>) {
    const join = (cur: string, add: string[]) => {
      const have = new Set(splitAddresses(cur).map((x) => (x.match(/<([^>]+)>/)?.[1] ?? x).trim().toLowerCase()));
      const extra = add.filter((x) => !have.has((x.match(/<([^>]+)>/)?.[1] ?? x).trim().toLowerCase()));
      return [cur.trim().replace(/[,;]$/, ""), ...extra].filter(Boolean).join(", ");
    };
    if (r.to?.length) to = join(to, r.to);
    if (r.cc?.length) { cc = join(cc, r.cc); showCc = true; }
    if (r.bcc?.length) { bcc = join(bcc, r.bcc); showBcc = true; }
  }

  const bookItems = $derived.by(() => {
    const q = bookSearch.toLocaleLowerCase();
    const items: { label: string; value: string; sub: string }[] = [];
    for (const g of groups) items.push({ label: g.name, value: g.members, sub: t("contacts.group") });
    for (const c of contacts) {
      for (const e of [c.email, c.email2].filter(Boolean)) items.push({ label: c.display_name, value: `${c.display_name} <${e}>`, sub: e });
    }
    return items.filter((i) => !q || i.label.toLocaleLowerCase().includes(q) || i.sub.toLocaleLowerCase().includes(q));
  });

  function addPicked(field: "to" | "cc" | "bcc") {
    const add = [...picked].join(", ");
    if (!add) return;
    const join = (cur: string) => (cur.trim() ? cur.trim().replace(/[,;]$/, "") + ", " + add : add);
    if (field === "to") to = join(to);
    if (field === "cc") { cc = join(cc); showCc = true; }
    if (field === "bcc") { bcc = join(bcc); showBcc = true; }
    picked = new Set();
  }
</script>

<svelte:window onkeydown={(e) => !inline && onkeydown(e)} />
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={rootEl}
  class="bg-background relative flex flex-col {inline ? 'h-full' : `h-screen border ${windowFocus.focused ? 'border-primary' : 'border-black/25 dark:border-white/20'}`}"
  onkeydown={(e) => inline && onkeydown(e)}
>
  {#if dropHover || fileHover}
    <div class="bg-primary/10 text-primary ring-primary pointer-events-none absolute inset-0 z-50 flex items-center justify-center text-base font-medium ring-4 ring-inset">
      <Paperclip class="mr-2 size-5" />{t("compose.drop_attach")}
    </div>
  {/if}
    {#if !inline}
    <WindowTitleBar title={winTitle} onsave={() => saveDraft()} onundo={() => editor?.exec("undo")} onredo={() => editor?.exec("redo")} onclose={() => close()} />
    {:else}
      <!-- Satır içi yanıt (Outlook): sağ üstte Sil ve Yeni Pencerede Aç -->
      <div class="flex shrink-0 items-center justify-end gap-1 px-3 pt-2 text-[13px]">
        {#if lastSaved}<span class="text-muted-foreground mr-auto text-xs">{t("compose.saved_at", new Date(lastSaved * 1000).toLocaleTimeString().slice(0, 5))}</span>{/if}
        <button class="hover:bg-accent flex items-center gap-1.5 rounded px-2 py-1" onclick={discard}><Trash2 class="size-4" />{t("common.delete")}</button>
        <button class="hover:bg-accent flex items-center gap-1.5 rounded px-2 py-1" onclick={popOut}><ExternalLink class="size-4" />{t("compose.pop_out")}</button>
      </div>
    {/if}
    <div class="contents">
    <!-- Şerit (Outlook ileti penceresi: Dosya | İleti | Ekle | Seçenekler | Metni Biçimlendir | Gözden Geçir) -->
    {#if !inline}
    <div class="bg-chrome">
    <Ribbon bind:active={ribbonTab} tabs={[{ key: "message", label: t("ribbon.message") }, { key: "insert", label: t("ribbon.insert") }, { key: "options", label: t("ribbon.options") }, { key: "format", label: t("ribbon.format_text") }, { key: "review", label: t("ribbon.review") }, ...(caret.table && !plain ? [{ key: "table", label: t("ribbon.table") }] : [])]}>
      {#snippet file()}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>{#snippet child({ props })}<button {...props} class="rounded-t px-2.5 pt-1 pb-1.5 hover:bg-black/5 dark:hover:bg-white/10">{t("menu.file")}</button>{/snippet}</DropdownMenu.Trigger>
          <DropdownMenu.Content class="w-56">
            <DropdownMenu.Item onclick={() => saveDraft()}><Save class="size-4" />{t("compose.save_draft")}<DropdownMenu.Shortcut>Ctrl+S</DropdownMenu.Shortcut></DropdownMenu.Item>
            <DropdownMenu.Item onclick={send}><Send class="size-4" />{t("compose.send")}<DropdownMenu.Shortcut>Ctrl+Enter</DropdownMenu.Shortcut></DropdownMenu.Item>
            <DropdownMenu.Separator />
            <DropdownMenu.Item onclick={discard}><Trash2 class="size-4" />{t("compose.discard")}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => close()}><X class="size-4" />{t("common.close")}<DropdownMenu.Shortcut>Esc</DropdownMenu.Shortcut></DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      {/snippet}
      {#snippet content(tab)}
        {#if tab === "message"}
          <RibbonGroup label={t("ribbon.g_clipboard")}>
            <RibbonButton icon={ClipboardPaste} label={t("ribbon.paste")} onclick={paste} />
            <RibbonStack>
              <RibbonButton small icon={Scissors} label={t("ribbon.cut")} onmousedown={(e: MouseEvent) => { e.preventDefault(); editor?.exec("cut"); }} />
              <RibbonButton small icon={Copy} label={t("ribbon.copy")} onmousedown={(e: MouseEvent) => { e.preventDefault(); editor?.exec("copy"); }} />
              <!-- Tek tıklama bir kez, çift tıklama Esc'e kadar uygular (Outlook Biçim Boyacısı) -->
              <RibbonButton small icon={Paintbrush} label={t("editor.painter")} title={t("editor.painter_hint")} disabled={plain} active={caret.painter > 0} onmousedown={hold} onclick={() => editor?.painter()} ondblclick={() => editor?.painter(true)} />
            </RibbonStack>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_basic_text")}>
            <div class:opacity-40={plain} class:pointer-events-none={plain}><EditorTools layout="basic" {editor} {caret} /></div>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_names")}>
            <RibbonButton icon={BookUser} label={t("compose.address_book")} onclick={openBook} />
            <RibbonButton icon={UserCheck} label={t("ribbon.check_names")} onclick={checkNames} />
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_include")}>
            <DropdownMenu.Root>
              <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={Paperclip} label={t("ribbon.attach_file")} menu />{/snippet}</DropdownMenu.Trigger>
              <DropdownMenu.Content>
                <DropdownMenu.Item onclick={attach}>{t("compose.attach")}…</DropdownMenu.Item>
                <DropdownMenu.Item onclick={openItemPicker}>{t("compose.attach_item")}…</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
            {@render signatureMenu()}
            {@render templateMenu()}
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_tags")} last>
            <RibbonStack>
              <RibbonButton small icon={Clock} label={t("compose.delay")} active={!!sendAt} onclick={openDelay} />
              <RibbonButton small icon={AlertCircle} iconClass="text-red-600" label={t("ribbon.high_importance")} active={importance === 2} onclick={() => (importance = importance === 2 ? 1 : 2)} />
              <RibbonButton small icon={ArrowDown} iconClass="text-blue-600" label={t("ribbon.low_importance")} active={importance === 0} onclick={() => (importance = importance === 0 ? 1 : 0)} />
            </RibbonStack>
          </RibbonGroup>
        {:else if tab === "format"}
          <RibbonGroup label={t("ribbon.g_format")}>
            <RibbonButton icon={Code} label={t("ribbon.html")} active={!plain} onclick={() => (plain = false)} />
            <RibbonButton icon={Type} label={t("ribbon.plain")} active={plain} onclick={toPlain} />
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_font")}>
            <div class:opacity-40={plain} class:pointer-events-none={plain}><EditorTools layout="font" {editor} {caret} /></div>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_paragraph")}>
            <div class:opacity-40={plain} class:pointer-events-none={plain}><EditorTools layout="paragraph" {editor} {caret} /></div>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_styles")}>
            <div class="grid grid-cols-3 gap-1 py-0.5" class:opacity-40={plain} class:pointer-events-none={plain}>
              {#each BLOCK_STYLES as s (s.tag)}
                <button type="button" class="hover:border-primary flex h-[27px] w-[86px] items-center overflow-hidden rounded-[3px] border px-1.5 text-left whitespace-nowrap {caret.block === s.tag ? 'border-primary bg-(--primary-faint)' : ''}" data-style={s.tag} aria-pressed={caret.block === s.tag} title={t(s.label)} onmousedown={hold} onclick={() => editor?.setBlock(s.tag)}>
                  <span style={STYLE_PREVIEW[s.tag]}>{t(s.label)}</span>
                </button>
              {/each}
            </div>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_editing")} last>
            <RibbonStack>
              <RibbonButton small icon={Search} label={t("editor.find_replace")} onclick={() => (findOpen = true)} />
              <RibbonButton small icon={TextSelect} label={t("ribbon.select_all")} onclick={() => { editor?.focus(); editor?.exec("selectAll"); }} />
            </RibbonStack>
          </RibbonGroup>
        {:else if tab === "table"}
          <!-- Bağlamsal sekme: yalnızca imleç bir tablonun içindeyken görünür -->
          <RibbonGroup label={t("ribbon.g_table_styles")}>
            <div class="grid grid-cols-3 gap-1 py-0.5">
              {#each TABLE_STYLES as s (s.key)}
                <button type="button" class="hover:border-primary rounded-[3px] border bg-white p-[3px]" data-table-style={s.key} title={t(s.label)} aria-label={t(s.label)} onmousedown={hold} onclick={() => editor?.tableOp("style", s.key)}>
                  <span class="grid h-[19px] w-[44px] grid-cols-3 grid-rows-3" style={s.lines === "all" ? `gap:1px;background:${s.line};border:1px solid ${s.line}` : ""}>
                    {#each [0, 0, 0, 1, 1, 1, 2, 2, 2] as r, i (i)}
                      <span style="background:{r === 0 && s.head?.bg ? s.head.bg : s.band && r === 1 ? s.band : '#fff'}{s.lines === 'rows' ? `;border-bottom:${r === 0 ? 2 : 1}px solid ${s.line}` : s.lines === 'none' ? ';outline:1px dashed #cfcfcf;outline-offset:-1px' : ''}"></span>
                    {/each}
                  </span>
                </button>
              {/each}
            </div>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_rows_cols")}>
            <RibbonStack>
              <RibbonButton small icon={ArrowUp} label={t("table.row_above")} onmousedown={hold} onclick={() => editor?.tableOp("rowAbove")} />
              <RibbonButton small icon={ArrowDown} label={t("table.row_below")} onmousedown={hold} onclick={() => editor?.tableOp("rowBelow")} />
            </RibbonStack>
            <RibbonStack>
              <RibbonButton small icon={ArrowLeft} label={t("table.col_left")} onmousedown={hold} onclick={() => editor?.tableOp("colLeft")} />
              <RibbonButton small icon={ArrowRight} label={t("table.col_right")} onmousedown={hold} onclick={() => editor?.tableOp("colRight")} />
            </RibbonStack>
            <RibbonStack>
              <RibbonButton small icon={Rows3} label={t("table.del_row")} onmousedown={hold} onclick={() => editor?.tableOp("delRow")} />
              <RibbonButton small icon={Columns3} label={t("table.del_col")} onmousedown={hold} onclick={() => editor?.tableOp("delCol")} />
              <RibbonButton small icon={Trash2} iconClass="text-red-600" label={t("table.del_table")} onmousedown={hold} onclick={() => editor?.tableOp("delTable")} />
            </RibbonStack>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_cell")}>
            <div class="flex flex-col gap-1 py-0.5">
              <ColorButton icon={PaintBucket} label={t("table.shading")} kind="shade" onpick={(c) => editor?.setColor("shade", c)} />
            </div>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_table_size")} last>
            <RibbonStack>
              <RibbonButton small icon={MoveHorizontal} label={t("table.fit_window")} onmousedown={hold} onclick={() => editor?.tableOp("fit")} />
              <RibbonButton small icon={Shrink} label={t("table.fit_content")} onmousedown={hold} onclick={() => editor?.tableOp("auto")} />
            </RibbonStack>
          </RibbonGroup>
        {:else if tab === "insert"}
          <RibbonGroup label={t("ribbon.g_include")}>
            <RibbonButton icon={Paperclip} label={t("ribbon.attach_file")} onclick={attach} />
            <RibbonButton icon={Mail} label={t("compose.attach_item")} onclick={openItemPicker} />
            {@render signatureMenu()}
            {@render templateMenu()}
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_tables")}>
            <DropdownMenu.Root bind:open={tableOpen}>
              <DropdownMenu.Trigger disabled={plain}>{#snippet child({ props })}<RibbonButton {...props} icon={TableIcon} label={t("editor.table")} menu disabled={plain} />{/snippet}</DropdownMenu.Trigger>
              <DropdownMenu.Content class="w-auto p-0" onCloseAutoFocus={hold}>
                <!-- Izgara menü öğesi olmadığı için seçince menü kendiliğinden kapanmaz -->
                <TableGrid onpick={(r, c) => { tableOpen = false; editor?.insertTable(r, c); }} oncustom={() => { tableOpen = false; void editor?.table(); }} />
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_illustrations")}>
            <RibbonButton icon={ImageIcon} label={t("editor.insert_image")} disabled={plain} onclick={() => editor?.pickImage()} />
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_links")}>
            <RibbonButton icon={LinkIcon} label={t("editor.insert_link")} disabled={plain} onclick={() => editor?.link()} />
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_symbols")} last>
            <RibbonButton icon={Minus} label={t("editor.hr")} disabled={plain} onclick={() => editor?.exec("insertHorizontalRule")} />
            <RibbonButton icon={CalendarClock} label={t("editor.date_time")} onclick={() => editor?.exec("insertText", new Date().toLocaleString(i18n.locale, { dateStyle: "long", timeStyle: "short" }))} />
            <DropdownMenu.Root>
              <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={Smile} label={t("editor.emoji")} menu />{/snippet}</DropdownMenu.Trigger>
              <DropdownMenu.Content class="w-auto p-1">
                <div class="grid grid-cols-8 gap-0.5">
                  {#each EMOJI as e (e)}<DropdownMenu.Item class="justify-center px-1 py-1 text-lg leading-none" onclick={() => editor?.exec("insertText", e)}>{e}</DropdownMenu.Item>{/each}
                </div>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </RibbonGroup>
        {:else if tab === "options"}
          <RibbonGroup label={t("ribbon.g_show_fields")}>
            <RibbonButton icon={Users} label={t("compose.bcc_btn")} active={showBcc} onclick={() => (showBcc = !showBcc)} />
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_format")}>
            <RibbonButton icon={Code} label={t("ribbon.html")} active={!plain} onclick={() => (plain = false)} />
            <RibbonButton icon={Type} label={t("ribbon.plain")} active={plain} onclick={toPlain} />
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_tracking")}>
            <RibbonButton small icon={MailCheck} label={t("compose.read_receipt")} active={readReceipt} onclick={() => (readReceipt = !readReceipt)} />
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_security")}>
            <RibbonStack>
              <RibbonButton small icon={ShieldCheck} label={t("compose.sign")} active={sign} onclick={() => (sign = !sign)} />
              <RibbonButton small icon={Lock} label={t("compose.encrypt")} active={encrypt} onclick={() => (encrypt = !encrypt)} />
              <DropdownMenu.Root>
                <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} small icon={Shield} label={secMethod === "pgp" ? "OpenPGP" : "S/MIME"} menu />{/snippet}</DropdownMenu.Trigger>
                <DropdownMenu.Content>
                  <DropdownMenu.Item onclick={() => (secMethod = "smime")}>S/MIME</DropdownMenu.Item>
                  <DropdownMenu.Item onclick={() => (secMethod = "pgp")}>OpenPGP</DropdownMenu.Item>
                </DropdownMenu.Content>
              </DropdownMenu.Root>
            </RibbonStack>
          </RibbonGroup>
          <RibbonGroup label={t("ribbon.g_more_options")} last>
            <RibbonButton icon={Clock} label={t("compose.delay")} active={!!sendAt} onclick={openDelay} />
          </RibbonGroup>
        {:else if tab === "review"}
          <RibbonGroup label={t("ribbon.g_editing")} last>
            <RibbonButton icon={SpellCheck} label={t("ribbon.spelling")} active={app.setting("spell_check", "1") === "1"} onclick={() => void app.setSetting("spell_check", app.setting("spell_check", "1") === "1" ? "0" : "1")} />
            <RibbonButton icon={Search} label={t("editor.find_replace")} onclick={() => (findOpen = true)} />
            <RibbonButton icon={FileCode} label={t("editor.html_source")} disabled={plain} onclick={openSource} />
            <RibbonButton icon={TextSelect} label={t("ribbon.select_all")} onclick={() => { editor?.focus(); editor?.exec("selectAll"); }} />
          </RibbonGroup>
        {/if}
      {/snippet}
    </Ribbon>
    </div>
    {/if}

    {#snippet signatureMenu()}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={PenLine} label={t("compose.signature")} menu />{/snippet}</DropdownMenu.Trigger>
        <DropdownMenu.Content>
          {#each loadSignatures() as sg (sg.name)}
            <DropdownMenu.Item onclick={() => putSignature(signatureBlock(sg.name))}>{sg.name}</DropdownMenu.Item>
          {/each}
          <DropdownMenu.Item onclick={() => putSignature("")}>{t("compose.no_signature")}</DropdownMenu.Item>
          <DropdownMenu.Separator />
          <DropdownMenu.Item onclick={() => app.openSettings("signatures")}>{t("compose.edit_signatures")}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/snippet}

    {#snippet templateMenu()}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={FileText} label={t("compose.template")} menu />{/snippet}</DropdownMenu.Trigger>
        <DropdownMenu.Content>
          {#each loadTemplates() as tp (tp.name)}
            <DropdownMenu.Item onclick={() => applyTemplate(tp)}>{tp.name}</DropdownMenu.Item>
          {/each}
          {#if loadTemplates().length}<DropdownMenu.Separator />{/if}
          <DropdownMenu.Item onclick={saveAsTemplate}>{t("compose.save_template")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => app.openSettings("templates")}>{t("compose.edit_templates")}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/snippet}

    {#if sendAt}
      <div class="flex items-center gap-2 bg-amber-50 px-4 py-1 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">
        <Clock class="size-3.5" />{t("compose.scheduled", fmtDateTime(sendAt))}
        <button class="underline" onclick={() => (sendAt = null)}>{t("common.cancel")}</button>
      </div>
    {/if}

    <!-- Başlık alanları (Outlook): solda Gönder; Kimden / Kime / Bilgi düğmeleri, Konu -->
    <div class="flex gap-3 border-b px-4 pt-3 pb-2">
      <button class="compose-btn flex h-[84px] w-[64px] shrink-0 flex-col items-center justify-center gap-2 text-[13px]" title="Ctrl+Enter" onclick={send}>
        <Send class="size-6" strokeWidth={1.5} />{t("compose.send")}
      </button>
      <div class="grid min-w-0 flex-1 grid-cols-[88px_1fr] items-center gap-x-2 gap-y-2 text-[13px]">
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>{#snippet child({ props })}<button {...props} class="compose-btn flex h-8 items-center justify-center gap-1 text-[13px]">{t("compose.from_btn")}<ChevronDown class="size-3.5" /></button>{/snippet}</DropdownMenu.Trigger>
          <DropdownMenu.Content class="w-80">
            <DropdownMenu.RadioGroup value={identity ? `${identity.account_id}:${identity.email}` : String(accountId ?? "")} onValueChange={pickFrom}>
              {#each app.accounts as a (a.id)}
                <DropdownMenu.RadioItem value={String(a.id)}>{a.display_name ? `${a.display_name} <${a.email}>` : a.email}</DropdownMenu.RadioItem>
                {#each loadIdentities(a.id) as x (x.email)}
                  <DropdownMenu.RadioItem value={`${a.id}:${x.email}`}>{x.name ? `${x.name} <${x.email}>` : x.email}</DropdownMenu.RadioItem>
                {/each}
              {/each}
            </DropdownMenu.RadioGroup>
            {#if !app.accounts.length}<DropdownMenu.Item onclick={() => app.openAccountDialog()}>{t("account.add")}</DropdownMenu.Item>{/if}
          </DropdownMenu.Content>
        </DropdownMenu.Root>
        <div class="truncate px-1">{fromLabel}</div>
        <button class="compose-btn mt-0.5 h-8 self-start text-[13px]" onclick={openBook}>{t("compose.to_btn")}</button>
        <div class="compose-field"><AddressInput id={`to-${req.key}`} bind:value={to} outlook pills collapse={8} /></div>
        <button class="compose-btn mt-0.5 h-8 self-start text-[13px]" onclick={openBook}>{t("compose.cc_btn")}</button>
        <div class="compose-field"><AddressInput bind:value={cc} outlook pills collapse={8} /></div>
        {#if showBcc}
          <button class="compose-btn mt-0.5 h-8 self-start text-[13px]" onclick={openBook}>{t("compose.bcc_btn")}</button>
          <div class="compose-field"><AddressInput bind:value={bcc} outlook pills collapse={8} /></div>
        {/if}
        <span class="text-muted-foreground pr-2 text-right">{t("mail.subject")}</span>
        <input class="compose-field h-8 bg-transparent px-1 outline-none" bind:value={subject} />
      </div>
    </div>

    {#if attachments.length}
      <!-- Ek kartları: türe göre simge, boyut; çift tıklayınca açılır. Sağda toplam (25 MB üstü uyarı rengi) -->
      <div class="flex flex-wrap items-center gap-1.5 border-b px-4 py-1.5" data-testid="attachments">
        {#each attachments as a, i (i + a.name)}
          {@const [, Kind, color] = fileKind(a.name)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span class="bg-(--primary-faint) flex h-9 max-w-64 items-center gap-2 rounded-[4px] border pr-1 pl-1.5 text-xs" title={a.name} ondblclick={() => a.path && void api.openAttachment(a.path).catch((e) => app.error(e))}>
            <span class="flex size-6 shrink-0 items-center justify-center rounded-[3px] text-white" style="background:{color}"><Kind class="size-3.5" /></span>
            <span class="flex min-w-0 flex-col leading-[14px]">
              <span class="truncate">{a.name}</span>
              {#if a.size}<span class="text-muted-foreground text-[10px]">{fmtSize(a.size)}</span>{/if}
            </span>
            <button class="hover:bg-accent hover:text-destructive rounded p-0.5" title={t("common.remove")} aria-label={t("common.remove")} onclick={() => (attachments = attachments.filter((_, j) => j !== i))}><X class="size-3.5" /></button>
          </span>
        {/each}
        <span class="ml-auto text-[11px] {totalSize > 25 * 1024 * 1024 ? 'text-destructive font-medium' : 'text-muted-foreground'}">{t("compose.attachments_total", attachments.length, fmtSize(totalSize))}</span>
      </div>
    {/if}

    {#if findOpen}<FindReplaceBar {editor} onclose={() => (findOpen = false)} />{/if}
    <RichEditor bind:this={editor} bind:html bind:caret {plain} toolbar={inline} zoom={inline ? 1 : zoom / 100} fontStyle={fontCss(fontSpec)} onchange={() => (dirty = true)} onactivity={activity} onfiles={(p) => void addPaths(p)} />
    {#if !inline}
      <!-- Durum çubuğu (Word/Outlook gibi): sözcük sayısı, taslak durumu, biçim, yakınlaştırma -->
      <div class="bg-chrome text-muted-foreground flex h-6 shrink-0 items-center gap-3 border-t px-3 text-[11px] select-none" data-testid="compose-status">
        <span>{counts.selected ? t("compose.words_selected", counts.selected, counts.words) : t("compose.words", counts.words)}</span>
        <span>{t("compose.chars", counts.chars)}</span>
        {#if lastSaved || dirty}
          <span class="bg-border h-3 w-px"></span>
          <span>{dirty || !lastSaved ? t("compose.unsaved") : t("compose.saved_at", new Date(lastSaved * 1000).toLocaleTimeString().slice(0, 5))}</span>
        {/if}
        <span class="flex-1"></span>
        <span>{plain ? t("ribbon.plain") : "HTML"}</span>
        <span class="bg-border h-3 w-px"></span>
        <div class="flex items-center gap-1">
          <button class="rounded p-0.5 hover:bg-black/10 dark:hover:bg-white/10" title={t("compose.zoom_out")} aria-label={t("compose.zoom_out")} onclick={() => setZoom(zoom - 10)}><Minus class="size-3" /></button>
          <input type="range" class="accent-primary h-1 w-24" min="50" max="200" step="10" value={zoom} aria-label={t("ribbon.zoom")} oninput={(e) => setZoom(Number((e.target as HTMLInputElement).value))} />
          <button class="rounded p-0.5 hover:bg-black/10 dark:hover:bg-white/10" title={t("compose.zoom_in")} aria-label={t("compose.zoom_in")} onclick={() => setZoom(zoom + 10)}><Plus class="size-3" /></button>
          <button class="w-10 rounded text-right tabular-nums hover:bg-black/10 dark:hover:bg-white/10" title={t("compose.zoom_reset")} onclick={() => setZoom(100)}>%{zoom}</button>
        </div>
      </div>
    {/if}
    </div>
</div>

<!-- Teslimi geciktir -->
<Dialog.Root bind:open={delayOpen}>
  <Dialog.Content class="z-[60] sm:max-w-sm" overlayClass="z-[60]">
    <Dialog.Header><Dialog.Title>{t("compose.delay")}</Dialog.Title><Dialog.Description>{t("compose.delay_text")}</Dialog.Description></Dialog.Header>
    <div class="flex items-center gap-2"><DateField bind:value={delayDate} /><TimeField bind:value={delayTime} /></div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => { sendAt = null; delayOpen = false; }}>{t("compose.no_delay")}</Button>
      <Button data-default-button onclick={setDelay}>{t("common.ok")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- HTML kaynağı -->
<Dialog.Root bind:open={sourceOpen}>
  <Dialog.Content class="z-[60] flex h-[70vh] flex-col sm:max-w-3xl" overlayClass="z-[60]">
    <Dialog.Header><Dialog.Title>{t("editor.html_source")}</Dialog.Title></Dialog.Header>
    <textarea class="selectable min-h-0 flex-1 resize-none rounded border bg-transparent p-2 font-mono text-xs outline-none" spellcheck="false" bind:value={sourceHtml}></textarea>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (sourceOpen = false)}>{t("common.cancel")}</Button>
      <Button onclick={applySource}>{t("common.ok")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Öğe Ekle: Outlook öğesi (.msg eki) -->
<Dialog.Root bind:open={itemOpen}>
  <Dialog.Content class="z-[60] sm:max-w-xl" overlayClass="z-[60]">
    <Dialog.Header><Dialog.Title>{t("compose.attach_item")}</Dialog.Title></Dialog.Header>
    <Input placeholder={t("compose.attach_item_search")} bind:value={itemQuery} oninput={() => { clearTimeout(itemTimer); itemTimer = setTimeout(searchItems, 250); }} />
    <div class="h-80 overflow-y-auto rounded-md border">
      {#each itemRows as r (r.id)}
        <label class="hover:bg-accent flex cursor-pointer items-center gap-2 border-b px-3 py-1.5 text-sm">
          <input type="checkbox" checked={itemPicked.has(r.id)} onchange={(e) => { const s2 = new Set(itemPicked); (e.target as HTMLInputElement).checked ? s2.add(r.id) : s2.delete(r.id); itemPicked = s2; }} />
          <div class="min-w-0 flex-1">
            <div class="truncate font-medium">{r.subject || t("mail.no_subject")}</div>
            <div class="text-muted-foreground truncate text-xs">{r.sender} · {fmtDateTime(r.date)}</div>
          </div>
        </label>
      {:else}
        <div class="text-muted-foreground p-6 text-center text-sm">{t("mail.empty_folder")}</div>
      {/each}
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (itemOpen = false)}>{t("common.cancel")}</Button>
      <Button disabled={!itemPicked.size} onclick={attachItems}>{t("compose.attach")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<AddressBookPicker
  bind:open={pickerOpen}
  fields={[{ key: "to", label: t("compose.to_btn") }, { key: "cc", label: t("compose.cc_btn") }, { key: "bcc", label: t("compose.bcc_btn") }]}
  onpickFields={applyPicked}
/>

<!-- Adres defteri (eski) -->
<Dialog.Root bind:open={bookOpen}>
  <Dialog.Content class="z-[60] sm:max-w-lg" overlayClass="z-[60]">
    <Dialog.Header><Dialog.Title>{t("compose.address_book")}</Dialog.Title></Dialog.Header>
    <Input placeholder={t("contacts.search")} bind:value={bookSearch} />
    <div class="h-72 overflow-y-auto rounded-md border">
      {#each bookItems as it, i (i + it.value)}
        <label class="hover:bg-accent flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm">
          <input type="checkbox" checked={picked.has(it.value)} onchange={(e) => { const s = new Set(picked); (e.target as HTMLInputElement).checked ? s.add(it.value) : s.delete(it.value); picked = s; }} />
          <span class="flex-1 truncate">{it.label}</span><span class="text-muted-foreground truncate text-xs">{it.sub}</span>
        </label>
      {:else}
        <div class="text-muted-foreground p-6 text-center text-sm">{t("contacts.empty")}</div>
      {/each}
    </div>
    <Dialog.Footer class="gap-1">
      <Button variant="outline" onclick={() => addPicked("to")}>{t("mail.to")} →</Button>
      <Button variant="outline" onclick={() => addPicked("cc")}>{t("mail.cc")} →</Button>
      <Button variant="outline" onclick={() => addPicked("bcc")}>{t("mail.bcc")} →</Button>
      <Button onclick={() => (bookOpen = false)}>{t("common.close")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
