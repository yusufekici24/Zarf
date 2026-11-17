# Zarf — kod haritası

Bu dosya, kodu her seferinde baştan taramadan doğru yere gitmek için yazıldı. Bir şeyi değiştirince
burayı da güncelleyin (yeni modül, tablo, ayar anahtarı, olay ya da tuzak).

Zarf, Debian tabanlı Linux için Outlook benzeri e-posta, takvim, kişiler, görevler ve notlar uygulamasıdır.
Kurumsal Exchange (EWS) birinci önceliktir, IMAP/SMTP de desteklenir. Arayüz metinleri ve kod açıklamaları Türkçedir.
Ürün adı Zarf (2.0.0'dan beri); eski adları Lynora, MailNix ve Linux Posta. lynora veri/çalıştırılabilir dosya kimlikleri ve Lynora anahtarlık hizmeti yükseltme uyumluluğu için korunur.

- Arka uç: Rust + Tauri 2 (`src-tauri/`). SQLite (rusqlite, gömülü), rustls, kendi HTTP/EWS/SMTP istemcileri.
- Ön yüz: Svelte 5 (runes) + SvelteKit statik + Tailwind 4 + shadcn-svelte (`src/`).
- Paket: `.deb` (glibc 2.39+, yerel derleme) ya da Docker'da Ubuntu 22.04 tabanı (`packaging/build-packages.sh`).
- Depo: github.com/yusufekici24/Zarf-LinuxMailApplication (özel). Sürümler Releases'te `Zarf_X.Y.Z_amd64.deb`.

## Konu belgeleri (önce buraya bakın)

Bu dosya genel haritadır. Aşağıdaki konularda ayrıntı (hangi fonksiyon nerede, kurallar, tuzaklar) ayrı
dosyadadır; ilgili işe başlamadan yalnızca o dosyayı okumak yeter.

| Konu | Dosya |
|---|---|
| Performans: ölçümler, salt okunur DB tanısı, sayaç yenileme ve çözüm kararları | `docs/PERFORMANS.md` |
| Zarf 2.0: grup zamanlayıcısı, kişi yönetimi değerlendirmesi, performans ve marka | `docs/zarf-2.0.md` |
| Şu anki durum: son sürüm, doğrulanmamış noktalar, sıradaki işler | `docs/DURUM.md` |
| Arama: sözdizimi, ifade ağacı, SQL/AQS, arama kutusu önerileri | `docs/arama.md` |
| Konuşmalar: `tid`, `thread_keys`, bağlama kuralları, konuşmada okundu | `docs/konusmalar.md` |
| İleti gövdesi: HTML temizleme, tablo ve resim düzeni | `docs/ileti-govdesi.md` |
| Yazma penceresi ve düzenleyici: biçim araçları, renk düğmesi, stiller, tablo sekmesi, durum çubuğu | `docs/yazma-penceresi.md` |
| Linux Posta → Lynora geçişi, bilinen sınırlar | `docs/GECIS.md` |

Yeni bir konu büyüdüğünde (takvim, kurallar, yazma penceresi …) buraya uzun bölüm eklemek yerine `docs/` altına
dosya açıp bu tabloya satır ekleyin. Bir oturumun sonunda `docs/DURUM.md` güncellenir.

## Komutlar

```sh
npm run tauri dev                       # geliştirme
npm run check                           # svelte-check (0 hata; 15 eski uyarı var)
cd src-tauri && cargo test --lib        # Rust birim testleri (109)
npm run test:ui                         # Playwright arayüz testleri (sahte arka uç, sistem Chrome'u)
npx tauri build --bundles deb           # → src-tauri/target/release/bundle/deb/Zarf_X.Y.Z_amd64.deb
```

Kabuk betiklerinde `LC_ALL=C` kullanın: tr_TR yerelinde `[a-z]` aralığı `i`'yi kapsamıyor.

## Dizinler

| Yol | İçerik |
|---|---|
| `src-tauri/src/` | Arka uç (aşağıdaki tablo) |
| `src-tauri/src/mail/` | Protokol istemcileri: EWS, IMAP, SMTP, HTTP, TLS, Kerberos/NTLM, MIME, DAV |
| `src-tauri/tests/` | Gerçek sunuculara karşı uçtan uca testler (`#[ignore]`, ortam değişkeniyle) |
| `src/routes/+page.svelte` | **Tüm pencerelerin tek sayfası**: pencere etiketine göre ana pencere, ikincil pencere, uyarılar ya da anımsatıcı çizilir |
| `src/routes/+layout.svelte` | Gömülü yazı tipi, `app.css`, arayüz yazı tipinin uygulanması, `SelectPopup`, hata günlüğü |
| `src/lib/stores/` | `app.svelte.ts` (ayarlar, hesaplar, klasörler, onay/istem pencereleri), `mail.svelte.ts` (liste, seçim, okuma) |
| `src/lib/components/` | Modüller: `mail/`, `calendar/`, `contacts/`, `tasks/`, `notes/`, `dialogs/`, `ribbon/`, `common/`, `ui/` (shadcn) |
| `src/lib/api.ts` | Rust komutlarına tipli sarmalayıcılar (komut adı = Rust fonksiyon adı) |
| `src/lib/types.ts` | Rust veri modellerinin TS karşılıkları |
| `src/lib/search-syntax.ts` | Arama anahtarları ve öneriler; `db.rs` `search_key()` ile aynı kalmalı (`docs/arama.md`) |
| `locales/tr.json`, `en.json` | Arayüz metinleri; **iki dosyada aynı anahtarlar olmalı**. Rust `t!()`, ön yüz `t()` |
| `tests/ui/` | Playwright testleri ve `mock-backend.ts` |
| `docs/` | Konu belgeleri (yukarıdaki tablo) |
| `packaging/` | Docker paket betiği, `.desktop`, MIME, `postinst.sh` |
| `.github/workflows/build.yml` | Her push: npm build + svelte-check + `cargo test --lib`. Paket işi yalnızca elle (workflow_dispatch) |

## Mimari ve veri akışı

```
Svelte bileşeni → store (mail/app) → api.ts invoke → commands.rs (#[tauri::command])
   → yerel değişiklik hemen db.rs'e yazılır, ardından MailService (sync.rs) kuyruğuna iş eklenir
   → Worker (iki iş parçacığı: "lynora-mail" ön kuyruk, "lynora-mail-bg" arka kuyruk)
   → mail/ews.rs | mail/imap_client.rs | mail/smtp.rs | mail/dav.rs
   → sonuç db.rs'e yazılır → emit("mail://changed" …) → +page.svelte dinler → store yenilenir
```

- Kuyruk (`sync.rs` `Queue`): anahtarlı iş zaten bekliyorsa yenisi eklenmez (`submit_keyed`). Ön kuyruk hataları
  `mail://error` ile kullanıcıya gösterilir; arka kuyruk hataları yalnızca `eprintln!` (görünmez!). Ağ hataları
  (`MailError::is_net`) "Bağlantı kesildi" durumuna geçirir; `spawn_net_monitor` bağlantı gelince `sync_all` çalıştırır.
- Veritabanı: tek yazıcı bağlantı `db.conn()` (Mutex) + 3 okuyucu `db.rconn()`. WAL, `busy_timeout=5000`.
- Pencereler (`windows.rs`): ana pencere `main`; ikincil pencereler `w-…` (yeni ileti, okuma; ayarlar `w-settings`). Hızlı açılış için
  gizli bir yedek pencere hazır tutulur; içerik `window://assign` olayıyla verilir. Ayrıca `alerts` ve `reminders`.
  Ayar başka pencerede değişince `app://state-changed` → her pencere `app.loadAll()`.

## Arka uç modülleri (`src-tauri/src`)

| Dosya | Sorumluluk / önemli öğeler |
|---|---|
| `lib.rs` | Açılış: tek örnek, açılış ekranı, veri klasörü taşıma, `Db` açma, iş parçacıkları (arama dizini kurma, boş alan geri verme), komut kaydı (`invoke_handler`) |
| `commands.rs` | Tüm Tauri komutları (~4000 satır). Yeni komut: burada yaz, `lib.rs`'te kaydet, `api.ts`'e ekle |
| `db.rs` | Şema (`SCHEMA`), sonradan eklenen sütunlar (`MIGRATIONS`), açılış geçişleri (`init_*`), tüm sorgular. Testler dosyanın sonunda (`search_tests` modülü) |
| `sync.rs` | `MailService` (arayüzden çağrılan, kuyruğa iş ekleyen) ve `Worker` (işi yapan). IMAP eşitleme, Giden Kutusu, bayraklar (`push_flags`), IDLE |
| `ews_sync.rs` | Exchange eşitlemesi: klasör listesi, klasör (`ews_sync_folder`, tam tarama `ews_enumerate`, değişiklikler `ews_apply_changes`), gövde ön yükleme, sabitlenenler, takvim (`ews_sync_calendar`), kişiler, kurallar |
| `pim_sync.rs` / `dav_sync.rs` | Exchange görev/not/kişi grubu; CalDAV/CardDAV çift yönlü eşitleme |
| `windows.rs` | İkincil pencereler, yedek pencere, uyarı kartları, anımsatıcı penceresi |
| `crypto.rs` | S/MIME (openssl komutu) ve OpenPGP (gpg) |
| `rules.rs` | Yerel kural değerlendirme (IMAP); Exchange kuralları sunucuda (`mail/rule_spec.rs`) |
| `msgfile.rs`, `mail/tnef.rs`, `ical.rs` | .msg, winmail.dat, .ics/.vcf/CSV |
| `backup.rs` | Yedek dosyası (AES-256-GCM) |
| `secret.rs` | Parolalar anahtarlıkta (keyring); `password_plain()` |
| `splash.rs`, `desktop.rs`, `visibility.rs`, `links.rs`, `filedialog.rs`, `i18n.rs` | Açılış ekranı, masaüstü bütünleşmesi, görünürlük, bağlantı açma, dosya diyaloğu, çeviri |
| `mail/ews.rs` | EWS istemcisi. `call()` SOAP gönderir, ileti bazlı hataları döndürür; sunucu meşgulse (`ErrorServerBusy`) istenen süre dolmadan istek atmaz (`busy_left`). `pooled()` bağlantı havuzu. Takvim: `calendar_summary_of` (parçalı, `fetch_split`), `calendar_details` |
| `mail/imap_client.rs` | `imap` crate sarmalayıcısı (select, search, fetch, store_flag, IDLE) |
| `mail/http.rs`, `tls.rs`, `net.rs` | HTTP/1.1 (rustls), TCP/TLS, bağlantı durumu ve "yakında ulaşılamadı" listesi |
| `mail/discover.rs`, `gssapi.rs`, `ntlm.rs`, `ldap.rs`, `dns.rs` | Autodiscover, Kerberos, NTLMv2, AD'den adres, SRV |
| `mail/parse.rs`, `build.rs`, `html.rs`, `address.rs` | MIME ayrıştırma/oluşturma, HTML temizleme (ammonia), adres düzeltme |

## Veritabanı (`~/.local/share/lynora/lynora.db`)

Tablolar: `accounts folders messages message_content message_fts fts_build pending_flags contacts contact_groups
directory recipients calendars events tasks notes rules categories settings dismissed_reminders thread_keys`.

- `messages`: liste sütunları + `ns` (önekleri atılmış küçük harfli konu: konu sıralaması ve konuşma yedek yolu),
  `conv` (Exchange konuşma kimliği), `tid` (konuşma numarası; `thread_keys` ile hesaplanır, bkz. `docs/konusmalar.md`),
  `remote_id` (Exchange öğe kimliği) ya da `uid` (IMAP), `is_read/is_flagged/is_answered/is_pinned`, `att_names`.
- `message_content`: gövde ve özgün ileti (`raw`), ayrı tabloda (liste hızlı kalsın). `write_content` UPSERT'tir.
- `message_fts`: içeriksiz FTS5 arama dizini; tetikleyicilerle güncellenir. `fts_build`: eski iletilerin arka plan kurulum aralığı.
- `pending_flags (message_id, field, value, attempts)`: kullanıcının sunucuya henüz yazılmamış bayrak değişiklikleri.
- `events`: `remote_id`, `master_id` (yinelenen seri), `change_key` (Exchange ChangeKey), `etag/dav_raw/dirty` (CalDAV).
- `settings`: anahtar/değer; ön yüzde `app.setting(key, varsayılan)`.

Şema değişikliği: yeni tablo → `SCHEMA` (`CREATE TABLE IF NOT EXISTS`); yeni sütun → `MIGRATIONS` listesine
`(tablo, sütun, tür)`; veri dönüşümü → `init()` içinde bir kez çalışan `init_*` (ayar anahtarıyla korunur).
Büyük düzen değişikliği gerekiyorsa `needs_upgrade()` true döner ve açılış ekranında geçiş yapılır.

## Okundu/bayrak akışı (okunan iletinin yeniden kalın görünmemesi)

1. Arayüz: `mail.setRead` satırı hemen günceller → `set_read` komutu.
2. `MailService::set_flag` → `db.set_flags_local` (alanı yazar + `pending_flags` kaydı) → `flags:{hesap}` anahtarlı iş.
3. `Worker::push_flags`: klasör/alan/değer başına tek istek (EWS `UpdateItem`, IMAP `UID STORE`). Başarılıysa
   kayıt düşer; ağ hatası ya da duraklatılmış hesapta kalır (sonraki eşitlemede yeniden); diğer hatalarda 3 denemeden
   sonra düşer. `Worker::sync_account` her eşitlemeden önce `push_flags` çağırır.
4. Sunucudan gelen bayraklar **yalnızca** `db.set_flag_from_server` ile yazılır: bekleyen kayıt varsa dokunmaz.
   Bu alanlara `set_message_field` ile sunucu değeri yazmayın.
5. Otomatik okundu: `mail.autoRead` (`mark_read_delay`; okuma bölmesi kapalıysa yok). Konuşmada açılan ileti
   (`showOnly`) de okundu olur. Konuşmanın baş satırı açılınca o klasördeki bütün okunmamışlar okundu olur; baş
   satır içinde okunmamış varken kalın kalır (`mail.unread`, `thread_unread`; bkz. `docs/konusmalar.md`).

## Exchange takvimi

`ews_sync_calendar` (arka kuyruk): bugünden −60/+400 gün. `calendar_summary` aralığı 31 günlük parçalarla ister
(`CAL_MAX`=500 öğe/istek; Exchange'in `EWSFindCountLimit` sınırı 1000). Dolan parça (`IncludesLastItemInRange=false`
ya da `ErrorExceededFindCountLimit`) ikiye bölünür, sınırdaki toplantı tekilleştirilir. Ayrıntılar (açıklama,
katılımcılar) yalnızca ChangeKey'i değişen/yeni toplantılar için `calendar_details` ile istenir. Hata
`set_calendar_error` → `calendar://sync-error` → takvimde uyarı çubuğu (`calendar_sync_errors` komutu).

## Ön yüz

- `+page.svelte`: kısayollar, olay dinleyicileri (`mail://changed` → `mail.refresh()`, `calendar://changed` →
  `app.bump("calendar")` …), ölçek (`zoom.ts`), tema, modül seçimi.
- `stores/mail.svelte.ts`: `messages` (`$state.raw`, satırlar yerinde değiştirilmez), `selected`, `current`
  (gösterilen ileti), `thread` (okuma bölmesindeki konuşma), `expanded` (listede açılmış konuşmalar),
  `load(keep)` / `refresh()` / `loadMore()`, `select` → `show` → `autoRead`, `setRead`, `toggleExpand`.
- `MessageList.svelte`: sanal kaydırma (satır türleri `head`/`row`/`child`, ofsetler önceden hesaplanır).
  Adı Kime/Bilgi satırında geçen iletiler renkli yazılır (`toMe`, kurallar `me.ts`; liste satırı bunun için `cc` de taşır).
- İleti sürükleme: `MessageList` işaretçi olaylarıyla yürütür (HTML5 sürükle-bırak değil), durum `mail.drag`.
  İmlecin altındaki klasör `mail.drag.over`'a yazılır; `FolderPane` onu `drop-target` sınıfıyla vurgular, kapalı
  klasörü 700 ms'de açar, bölme kenarında listeyi kaydırır. Klasör satırları `data-folder-id` ve `data-drop-key` taşır.
- `ReadingPane.svelte`: `subject`/`details` snippet'leri; `stacked` ise konuşma alt alta (açık ileti + tek satırlık
  diğerleri), değilse tek ileti + geçiş düğmeleri. `MessageBody.svelte`: korumalı iframe, `autoHeight` kipi.
- `SettingsDialog.svelte`: `get/set` bekleyen değişiklikleri tutar, **Tamam**'da `commit()` yazar. Yardımcı
  snippet'ler: `section`, `check(key, label, varsayılan)`, `pick(key, label, varsayılan, [[değer, ad]], geniş)`.
- Yardımcılar: `format.ts` (tarih, ad/adres), `trust.ts` (güvenilir göndericiler), `uifont.ts` (arayüz/okuma yazı
  tipi), `accent.ts` (renk teması), `fonts.ts` (Kırtasiye), `zoom.ts` (ölçek), `security.ts`, `quicksteps.svelte.ts`,
  `me.ts` ("adım Kime/Bilgi satırında" eşleşmesi ve vurgu renkleri).
- Renk teması: `<html data-accent="…">` + `app.css`'teki `html[data-accent]` / `html.dark[data-accent]` kuralları
  `--primary`'yi değiştirir. Vurgu tonları için **sabit mavi yazmayın**; `text-primary`, `bg-primary/NN`,
  `bg-(--primary-soft)` (seçili), `bg-(--primary-hover)` (üzerine gelme), `bg-(--primary-faint)` kullanın. Anlam taşıyan
  renkler (takvim paleti, toplantı "meşgul" mavisi, şerit simge renkleri) temaya bağlı değildir.

## Önemli ayar anahtarları

| Anahtar | Değerler (varsayılan önce) | Nerede |
|---|---|---|
| `ui_scale` | `auto`, 0.9…1.5 | zoom.ts |
| `accent` | `blue`, `purple`, `red`, `pink`, `orange`, `green`, `teal`, `graphite` | accent.ts, +layout, üst çubuk paleti |
| `ui_font` | `""` (gömülü Noto Sans), `system`, aile adı | uifont.ts, +layout |
| `read_font`, `read_font_size` | `""` (Calibri), aile; `11` pt | MessageBody |
| `list_font` | `normal`, `small`, `large`, `xlarge` | MessageList |
| `unread_bold` | `1`, `0` | MessageList |
| `me_highlight`, `me_color`, `me_names` | `off`/`to`/`cc`/`tocc`; `red`/`orange`/`green`/`purple`/`blue`; `;` ile ayrılmış ek adlar | me.ts, MessageList |
| `preview_lines`, `reading_zoom`, `mark_read_delay` | `1`; `100`; `0` (−1 = hiç) | liste, okuma |
| `reading_pane` | `right`, `bottom`, `off` | MailView |
| `conversations` | `0`, `1` | liste sorgusu |
| `conversation_layout`, `conversation_order`, `conversation_expand` | `stacked`/`single`; `newest`/`oldest`; `1`/`0` | ReadingPane, MessageList |
| `group_by_date`, `show_avatars` | `1` | MessageList |
| `load_remote`, `trusted_senders` (JSON dizi), `plain_text_only` | `0`; `[]`; `0` | ReadingPane, trust.ts |
| `font_new`, `font_reply`, `font_plain` (JSON) | Calibri 11 / Consolas 10 | fonts.ts |
| `compose_mini_toolbar`, `compose_zoom` | `1`; `100` | RichEditor, ComposeWindow (`docs/yazma-penceresi.md`) |
| `undo_send` | `10` (sn; `0` kapalı, 5/10/20/30) | `undo-send.ts`, ComposeWindow, +page `undoSendToast`, sync.rs `send_later` |
| `theme`, `language`, `start_module`, `sync_interval`, `sync_on_select`, `work_offline` | | app |

## Olaylar (Rust → ön yüz)

`mail://changed {folder}` liste yenile · `mail://new` yeni posta · `mail://status` durum çubuğu · `mail://error`
hata · `mail://bg` arka plan etkinliği · `mail://older` eski iletiler geldi · `mail://sent` · `mail://rule-notify` ·
`net://state` bağlantı · `account://auth_failed` · `account://progress` · `calendar://changed` ·
`calendar://sync-error` · `contacts://changed` · `pim://changed` · `rules://changed|progress` · `directory://updated` ·
`reminders://refresh` · `alerts://add` · `window://assign|reset` · `settings://show` · `app://open-args|mailto|hover-link` ·
`compose://drag-hover|drop-messages`.

## Testler

- Rust: modül içi `#[cfg(test)]`. Veritabanı testleri `Db::open_memory()`. `sync.rs` testlerinde sahte IMAP
  sunucusu (`fake_imap`) ve `test_worker`. **Paralel testlerde aynı ana makineyi kullanmayın**: ulaşılamayan sunucu
  `net::mark_down(host)` ile genel listeye girer (ör. biri `127.0.0.1`, öteki `127.0.0.3`).
- Playwright: `tests/ui/mock-backend.ts` `window.__TAURI_INTERNALS__.invoke`'u taklit eder. Tanımsız komut `null`
  döner (bileşen `.map` çağırırsa test sayfa hatasıyla düşer): yeni komut kullanan ekran için işleyici ekleyin.
  `__mock.calls` çağrıları, `__mock.emit(olay, yük)` olay göndermeyi sağlar; ayarlar penceresi için `window: "w-settings"`
  ve ardından `window://assign`. Saf TS fonksiyonları sayfa içinde `import("/src/lib/x.ts")` ile sınanabilir (bkz. trust.spec).
- Gerçek WebKitGTK'de deneme (derleme ve sudo gerekmez): `scripts/webkit-mock.mjs` sahte arka ucu başlangıç betiğine
  çevirir, `scripts/webkit-dene.py` ön yüzü görünmeyen bir pencerede açıp fare olayı gönderir, betik çalıştırır,
  görüntü alır, tuş gönderir (kullanım dosyanın başında). Yalnızca yazılımla çizimi sınar; GPU ile çizim uygulamanın
  kendisinde denenir. Görünmez pencerede `setTimeout` kısılır: ölçüm betiklerinde zamanlayıcı yerine mikro görev
  kullanın. `mock-backend.ts`'in `list_messages` taklidi satır başına bütün iletileri tarar (binlerce iletide yavaş):
  büyük liste ölçerken taklidi sayfada hızlı bir sürümle değiştirin.
- Veritabanı ölçümü (gerçek kod, sentetik büyük kutu): `ZARF_PERF_N=150000 cargo test --release --lib perf_tests -- --ignored --nocapture`
  (`db.rs` `perf_tests`; sonuçlar `docs/PERFORMANS.md`).

## Sürüm yayınlama

1. Sürüm numarası 6 yerde: `package.json`, `package-lock.json` (satır 3 ve 9), `src-tauri/Cargo.toml`,
   `src-tauri/Cargo.lock` (`name = "lynora"` girdisi), `src-tauri/tauri.conf.json`, README'deki kurulum satırı.
2. `npm run check`, `cargo test --lib`, `npm run test:ui`, ardından `npx tauri build --bundles deb`.
3. Commit (yazar `yusufekici24 <yusuf.142536@gmail.com>`; **commit ve PR'lara AI/Claude imzası eklenmez**), açıklamalı
   etiket `vX.Y.Z` ("Zarf X.Y.Z"), `git push origin main` ve `git push origin vX.Y.Z`; CI'yi bekleyin.
4. `gh release create vX.Y.Z <deb> --verify-tag --title "Zarf X.Y.Z" --notes-file notlar.md --latest`; paketi
   `gh release download` ile indirip sha256'yı yereldekiyle karşılaştırın.

## Tuzaklar

- **SQLite `PRAGMA optimize`** küçük veritabanında FTS iç tablolarına "2 satır" istatistiği yazar; büyüyünce FTS
  yazmaları 40 kat yavaşlar. Optimize ayrı, kısa ömürlü bağlantıda çalışır ve FTS istatistikleri orada silinir
  (aynı bağlantıda silmek tahmini sıfırlamaz).
- İçeriksiz FTS5 `'delete'` komutu eski değerleri ister: tetikleyiciler eski gövdeyi okur; bu yüzden
  `message_content` için `REPLACE` değil UPSERT kullanılır.
- rusqlite `execute_batch` her ifadeyi tek adım çalıştırır: `PRAGMA incremental_vacuum(N)` sorgu olarak sonuna kadar adımlanmalı.
- ammonia: `<title>` `clean_content_tags`'e eklenmeli (yoksa metni gövdede görünür).
- **WebKitGTK Ctrl+Z / Ctrl+Y'yi bağlamıyor** (2.52'de denendi; ne metin kutusunda ne düzenlenebilir alanda).
  Düzenleyici kendi geçmişini kullanır, metin kutuları `execCommand("undo"|"redo")`'ya yönlendirilir
  (`docs/yazma-penceresi.md` → Geri al / yinele). Playwright (Chrome) bunu göstermez: kısayollar Chrome'da zaten çalışır.
- Gönderimi geri alma: ileti Giden Kutusu'nda `send:<zaman>:<hesap>` ile bekler; gönderim `sending:` ile, geri alma
  `hold` ile **koşullu** sahiplenir (`Db::swap_pending`), ikisi birden kazanamaz. Kapanışta yarım kalan `sending:`
  açılışta `send:`'e döner (`reset_stuck_sending`). Kısa bekleme (≤10 dk) `send_later` ile tam zamanında gönderilir.
- **WebKitGTK, fare basılıyken `:hover`'ı güncellemez** (basılan öğede kalır). Sürükleme sırasındaki vurgular
  `hover:` sınıfıyla yapılmaz; imlecin altındaki öğe `document.elementFromPoint` ile bulunup duruma yazılır
  (`mail.drag.over`). Chrome'da `:hover` çalıştığı için Playwright bu hatayı göstermez.
- **WebKitGTK**: korumalı iframe'deki belgeye eklenen dinleyiciler ve gözlemciler çalışmaz (bağlantılar `links.rs`
  üzerinden yerel taraftan gelir; `autoHeight` dışarıdan zamanlayıcıyla ölçer). Gizli pencerede
  `requestAnimationFrame` hiç tetiklenmez.
- **Yazı tipi**: arayüzde `Segoe UI Variable` kendiliğinden seçilmemeli; Windows yazı tipleri kurulu makinede tüm metin
  kalın görünüyordu (okunmuş/okunmamış ayırt edilemiyordu). Varsayılan gömülü Noto Sans; **değişken yazı tipi gömmeyin**:
  WebKitGTK kalınlık eksenini her çizimde uygulayamıyor (Ubuntu 24.04'te okunmamış satır üzerine gelince kalınlığını
  yitiriyordu). `@fontsource/noto-sans` kalınlık başına ayrı dosya verir (400/500/600/700, `+layout.svelte`).
- EWS ileti listesi alıcıları yalnızca görünen adla verir (`DisplayTo`/`DisplayCc`, adres yok); `to_addr`/`cc` ileti
  açılınca MIME'dan adresli yazılır. Alıcıya bakan arayüz kuralları (`me.ts`) bu yüzden hem adı hem adresi arar.
- EWS: dahili göndericide `EmailAddress` X.500 olabilir (`@` yok; liste başlığında yalnızca ad saklanır), ileti açılınca
  MIME'dan SMTP adresi yazılır. `CalendarView` sayfalanamaz: 1000 öğe sınırı (yukarıda).
- EWS istek sınırı (`ErrorServerBusy`): yanıt SOAP Fault olarak gelir, bekleme süresi `BackOffMilliseconds` içindedir.
  Süre dolmadan atılan her istek geri çevrilir ve sınırı uzatır; bu yüzden kapı `Ews::call` içindedir (sunucu adresi
  başına). Yeni EWS çağrısı `call` dışından gönderilmez. Meşgul hatası (`MailError::is_busy`) bağlantıyı düşürmez,
  `ews_enumerate` kendini `defer_after` ile yeniden sıraya koyar.
- Konuşmalar konuya göre **birleştirilmez**: bağ Exchange konuşma kimliği ve `References`/`In-Reply-To` ile kurulur
  (`docs/konusmalar.md`). Arama anahtarı eklerken `search_key()` ve `search-syntax.ts` birlikte değişir (`docs/arama.md`).
- ammonia `tfoot`'u varsayılan olarak atar; gövdede `overflow-wrap:anywhere` tabloları bozar (`docs/ileti-govdesi.md`).
- `tests/ui` dosyalarında `process` ve yazımsız `import("/src/…")` kullanmayın: `npm run check` hata verir (bkz. trust.spec).
- Yerel `<input type="color">` (ve `<select>`) `change` olayını yalnızca değer **değişince** gönderir: "aynı rengi
  yeniden uygula" gibi işler ona bağlanmaz (`docs/yazma-penceresi.md`, `ColorButton.svelte`).
- Svelte, öğe kenarındaki boşluğu kırpar: `<span>{x} · </span>` sondaki boşluğu kaybeder; `{" · "}` kullanın.
- `system_fonts` komutu zaten var (fc-list); Kırtasiye, ileti yazma ve Seçenekler kullanır.
- `svelte-check` CI'de `--threshold error`; yeni uyarı eklememeye çalışın (sayı 15).
