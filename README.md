# Zarf

Sürüm **2.0.5** (önceki ürün adı Lynora). [Kişi yönetimi, grup zamanlayıcısı ve performans değerlendirmesi](docs/zarf-2.0.md). Mevcut hesaplar ve anahtarlık uyumluluğu için teknik `lynora` kimlikleri korunur.

Debian tabanlı Linux dağıtımları için e-posta, takvim, kişiler, görevler ve notlar uygulaması.
Kurumsal **Microsoft Exchange** desteği sunar ve **.msg** ileti dosyalarını açar.

Altyapı: **Rust + Tauri 2**, **Svelte 5 + Tailwind CSS + shadcn-svelte** (arayüz).

## Zarf 2.0

Kırmızı-gri marka ve açılış ekranı, grup üyelerini gösteren zamanlayıcı, kişi yönetimi
ve büyük listelerde sayfalama. [Ayrıntılı performans analizi ve tanı adımları](docs/PERFORMANS.md).

## Özellikler

**Hesaplar**
- Microsoft Exchange (on-prem Exchange 2013+) EWS ile; IMAP/SMTP (Gmail, Hotmail, Yandex vb.).
- **Otomatik kurulum:** İlk açılışta oturum açmış kullanıcının adresi bulunur (Kerberos/SSSD/krb5.conf).
  Ayarlar Autodiscover ile alınır. Kurum ağında Kerberos ile **parola sorulmadan** bağlanılır.
- Kimlik doğrulama: Kerberos (GSSAPI/SPNEGO, tek oturum açma), NTLMv2, parola, OAuth2 (Microsoft 365 / Google).
- **Paylaşılan posta kutusu:** Tam Erişim yetkiniz olan kutu kendi kimliğinizle ayrı hesap gibi eklenir.
- **Kimlikler (takma adlar):** Bir hesaba ek "Kimden" adresleri değiştirilebilir, yanıt adresi ve kimliğe göre imza seçilebilir.
- **Hesap kilitlenme koruması:** Sunucu parolayı reddederse (ör. AD'de parola değişti) hesap duraklatılır.
  Eski parola bir daha gönderilmez, böylece AD hesabı kilitlenmez. Yeni parola girildiğinde eşitleme yeniden başlar.

**Posta**
- Sık Kullanılanlar, klasör ağacı, arama klasörleri; konuşma görünümü; kurallar; kategoriler; bayrak ve takip gibi gelişmiş özellikler mevcuttur.
- **Konuşmalar (OWA gibi):** Konuşma satırının okuna basınca iletileri, Gönderilmiş Öğeler'deki yanıtlar dahil,
  listede alt alta açılır. Okuma bölmesinde konuşmanın iletileri alt alta durur: gösterilen ileti açıktır, diğerleri
  tek satırdır, tıklanınca açılır ve okundu olur. **Seçenekler → Gelişmiş → Konuşmalar**: alt alta ya da tek ileti
  görünümü, en yeni ileti üstte ya da altta gibi seçenekler mevcuttur.
- **Okundu bilgisi kaybolmaz:** Okundu/okunmadı ve bayrak değişiklikleri yerelde hemen yazılır, sunucuya yazılana
  kadar bekleyen olarak tutulur. Bağlantı yokken (VPN, uyku) yapılan değişiklik bağlantı gelince gönderilir. Eşitleme
  bu arada sunucudaki eski durumu geri yazmaz; okunan ileti yeniden kalın görünmez.
- **Anlık posta:** Exchange akış aboneliği ve IMAP IDLE ile yeni ileti sunucuya düştüğü anda gelir.
- **Kurallar:** Exchange kuralları sunucuda çalışır. IMAP için yerel kurallar: çoklu koşul, ve/veya, düzenli ifade, özel durumlar ayarlanabilir.
- **Kuralları Şimdi Uygula:** Yerel kurallar seçilen klasördeki mevcut iletilere uygulanır. Alt klasörler ve okunmuş/okunmamış seçilebilir. **Önizle** hiçbir şeyi değiştirmeden hangi iletiye ne yapılacağını gösterir; çalıştırma ilerleme çubuğuyla sürer ve iptal edilebilir.
- **Şablonlar ve düzenleyici:** İleti şablonları, bul/değiştir, HTML kaynağı, emoji, yazım denetimi yapabilir (`hunspell-tr`).
- **Çökme kurtarma:** Yazılan ileti yerelde saklanır, uygulama çökerse açılışta geri yükleme önerilir.
- **Güvenilir göndericiler:** Adres ya da alan adı (`sirket.com`, alt alan adları dahil); bu kaynaklardan gelen resimler
  sorulmadan iner. Resim uyarısındaki "@sirket.com alan adına güven" düğmesi kurumdaki herkesi bir kerede ekler
  (Gmail, Outlook.com gibi herkese açık servislerde bu düğme çıkmaz).
- **Alıcı önerileri:** Yazarken kişiler, son kullanılan adresler ve kurum dizini (GAL) önerilir. Adresler "chip" olarak görünür.
- **İletiyi yeni iletiye sürükleyince .msg eki** olarak eklenir. "Ek olarak ilet" de .msg kullanır.
- Gönderimi geri alma, teslimi geciktirme, taslakların otomatik kaydı, imzalar, okundu bilgisi, önem düzeyi.
- Toplantı davetlerini Kabul Et / Belki / Reddet ile yanıtlama.
- Ofis dışında yanıtı (Exchange'de sunucuda çalışır).
- Arama dili: `kimden:ali (fatura VEYA makbuz) -taslak tarih:bu hafta ek:var durum:okunmamış`. Alanlar
  (`kimden:`, `kime:`, `bilgi:`, `kişi:`, `konu:`, `gövde:`, `ekadı:`, `kategori:`, `klasör:`, `tarih:`, `sonra:`,
  `önce:`), VEYA / DEĞİL / parantez, tırnak içinde tam ifade ve göreli tarihler (`dün`, `geçen ay`, `7g`,
  `2026-01-01..2026-03-31`). İngilizce karşılıkları da çalışır (`from:`, `to:`, `subject:`, `has:`, `is:`, `OR`, `NOT`).
  Arama kutusu yazarken alanları, değerlerini ve alanın içinde kişileri önerir.

**Görünüm**
- **Renk teması:** Üst çubuktaki palet düğmesi rengi tek tıkla değiştirir: mavi (varsayılan), mor, kırmızı, pembe,
  turuncu, yeşil, turkuaz, füme. Aynı yerden açık, koyu ya da sistem görünümü seçilir; **Seçenekler → Genel → Renk**
  bölümünde de vardır. Seçili satır, bugün ve üzerine gelme tonları seçilen renkten türer, koyu görünümde rengin açık tonu kullanılır.
- **Yazı tipi:** **Seçenekler → Genel → Yazı tipi** arayüzün yazı tipini bu bilgisayarda kurulu yazı tiplerinden seçer.
  Varsayılan, pakete gömülü Noto Sans'tır; her bilgisayarda aynı görünür. Sistemde Windows yazı tipleri kurulu olsa bile
  "Segoe UI Variable" kendiliğinden seçilmez: WebKitGTK bu değişken yazı tipinin kalınlığını her zaman uygulayamıyor ve
  okunmuş iletiler de kalın görünebiliyordu.
- **Okuma Bölmesi (Seçenekler → Posta):** İleti listesi yazı boyutu (küçük, normal, büyük, çok büyük), okunmamış
  iletileri kalın ya da yalnızca renkli gösterme, kendi yazı tipi olmayan iletilerin okuma yazı tipi ve boyutu
  (varsayılan Calibri 11 pt). Arayüz ölçeği **Seçenekler → Genel** bölümündedir.

**Güvenlik**
- **S/MIME ve OpenPGP:** İmzalı ve şifreli iletiler okunur: imza doğrulanır, şifreli ileti çözülür. Okuma bölmesindeki çubuk imzanın geçerli, güvenilmeyen ya da geçersiz olduğunu ve imzalayanı gösterir.
- Yazarken **Seçenekler** sekmesinde *İmzala*, *Şifrele* ve yöntem seçimi. Şifreli iletiye yanıt otomatik şifrelenir, Gönderilmiş kopyasını siz de okuyabilirsiniz.
- **Ayarlar → Güven Merkezi:** Kişisel S/MIME sertifikası (.p12/.pfx) içe aktarma, OpenPGP anahtarı seçimi, varsayılan imzalama. Alıcı sertifikaları imzalı iletilerden otomatik toplanır, elle de eklenebilir.
- Çözülen içerik diske ve veritabanına yazılmaz. Özel anahtar şifreli saklanır, parolası anahtarlıktadır. `openssl` ve `gnupg` paketleri gerekir (paket önerir).
- **HTML temizleme:** Gelen HTML iletiler tarayıcıyla aynı HTML5 ayrıştırıcısı ve izin listesiyle temizlenir (ammonia).
  Betik, olay işleyicisi, form, iframe, gömülü nesne, `<base>`, `<meta>` ve `javascript:` bağlantıları her yazımıyla
  (kodlanmış, bölünmüş, büyük/küçük harf) atılır. Biçim korunur: stiller, Word/Outlook sınıfları, tablolar, gömülü görseller.
  İleti ayrıca betik çalıştıramayan, içerik güvenlik politikalı (CSP) bir çerçevede gösterilir.

**Dosyalar**
- `.msg`, `.eml`, `.mbox`, `.ics`, `.vcf` açma ve içe aktarma; iletiyi `.msg`/`.eml` olarak kaydetme.
- `.pst` içe aktarma (`pst-utils` paketi gerekir).

**Diğer**
- Takvim (Exchange takvimi ile eşitleme, yinelenen etkinlikler, hatırlatıcılar), kişiler ve gruplar, görevler, notlar. Görev, not ve kişi grupları Exchange ile çift yönlü eşitlenir.
- **Yoğun takvimler:** Exchange tek istekte en çok 1000 öğe döndürür (`EWSFindCountLimit`). Takvim bu yüzden 31 günlük
  parçalarla istenir, dolan parça ikiye bölünür; tekrarlayan toplantısı çok olan takvimler de eksiksiz gelir. Açıklama
  ve katılımcılar yalnızca yeni ya da değişen toplantılar için istenir (Exchange ChangeKey). Takvim eşitlenemezse
  nedeni takvimin üstünde yazar ve **Yeniden dene** ile tekrarlanır.
- **CalDAV ve CardDAV:** IMAP hesaplarında takvim ve kişiler Nextcloud, Radicale gibi sunucularla çift yönlü eşitlenir. Hesabın gelişmiş ayarlarına sunucu adresi yazılır, takvimler ve adres defterleri kendiliğinden bulunur. Yerelde değişen özellikler dışında sunucudaki nesne korunur.
- **Yedekle / geri yükle:** Tüm veriler tek dosyada, isteğe bağlı parolayla AES-256-GCM şifreli (Dosya > Aç ve Dışarı Aktar).
- Türkçe ve İngilizce arayüz. Yeni dil eklemek kolaydır (aşağıya bakın).

## Performans ve veritabanı

- **Hızlı liste:** İleti gövdeleri ve özgün iletiler ayrı tabloda tutulur. Klasör açmak, kaydırmak ve sıralamak posta
  kutusunun boyutundan bağımsızdır. Arka plan eşitlemesi listeyi yalnızca gerçekten bir şey değiştiğinde yeniler.
- **Tam metin arama dizini (SQLite FTS5):** Arama konu, kişiler, gövde, kategori ve ek adlarında dizinden yapılır.
  Türkçe harf ve aksan farkı gözetilmez (`AYDIN` ~ `aydın` ~ `aydin`, `Şükrü` ~ `sukru`), sözcük başları eşleşir.
  Dizin içeriği ikinci kez saklamaz, yalnızca sözcükleri tutar (veritabanının yaklaşık %10'u).
- **Konuşma görünümü:** "RE:/YNT:/Fw:" önekleri atılmış konu ayrı bir sütunda ve dizinde tutulur. Konuşmaları
  listelemek, konuya göre sıralamak ve bir konuşmayı açmak tüm iletileri taramaz.
- **Veritabanını küçültme:** **Seçenekler → Gelişmiş → Veritabanı** bölümü dosya boyutunu ve silinen postaların
  bıraktığı kullanılmayan alanı gösterir. *Veritabanını sıkıştır* bu alanı geri alır; diskte yeterli yer yoksa başlamaz.
  Sıkıştırmadan sonra (yeni kurulumlarda baştan) kullanılmayan alan arka planda kendiliğinden geri verilir.
- **Yavaş sorgu günlüğü:** 200 ms'yi aşan veritabanı işlemleri `~/.local/share/lynora/slow-queries.log` dosyasına
  yazılır. Yalnızca SQL metni yazılır, ileti içeriği ve adresler yazılmaz; dosya 1 MB'ı aşınca `.1` olarak kenara alınır.
  Yavaşlık bildirirken bu dosyayı ekleyin.

Sentetik posta kutularında ölçülen süreler:

| İşlem | Önce | 1.3.0 |
|---|---|---|
| Klasörü açma, ilk sayfa (6 bin ileti, 1,5 GB) | 186 ms | 1–2 ms |
| Tüm kutularda arama (30 bin ileti, 1 GB) | 1–5 sn | 3–50 ms |
| Konuşma görünümü (30 bin ileti) | 130 ms | 9–12 ms |
| Bir konuşmayı açma (30 bin ileti) | 127 ms | < 0,1 ms |

İki harflik önek araması (ör. `ka`) çok sayıda sözcüğü kapsadığı için yaklaşık 0,2 sn sürer.

### Sürüm yükseltme

1.1.0 ve öncesinden yükseltince veritabanı ilk açılışta **bir kez** yeni düzene geçer. Bu sırada açılış ekranında
"Veritabanı yeni sürüme hazırlanıyor (yalnızca bir kez)…" yazar (1,5 GB'lık posta kutusunda birkaç saniye). Geçiş
yarıda kesilirse sonraki açılışta kaldığı yerden sürer, dosya büyümez. Arama dizini eski iletiler için arka planda
kurulur (30 bin iletide birkaç saniye); bitene kadar arama eski yöntemle çalışır. Yeni düzene geçen veritabanını
eski sürümler açamaz: geri dönmek için yükseltmeden önce alınmış bir yedek gerekir.

1.3.x'ten 1.4.0'a yükseltmede geçiş yoktur: yalnızca bir tablo (bekleyen bayraklar) ve bir sütun (toplantı ChangeKey)
eklenir, 1.3.x bu veritabanını açmaya devam eder. İlk takvim eşitlemesinde toplantıların ayrıntıları bir kez yeniden
istenir.

## Kurulum

| Dağıtım | Paket |
|---|---|
| Ubuntu 24.04 ve sonrası, Debian 13 ve türevleri (glibc 2.39+) | `Zarf_*_amd64.deb` |

```sh
sudo apt install ./Zarf_2.0.5_amd64.deb      # bağımlılıklar otomatik kurulur
```

Paketler GitHub **Releases** sayfasında yayımlanır. Debian 12, Ubuntu 22.04 ve Pardus 23 gibi eski dağıtımlar için
paket Ubuntu 22.04 tabanında üretilir: `packaging/build-packages.sh` (Docker) ya da Actions > build > Run workflow.
`.deb` paketi sistemdeki WebKitGTK 4.1 ve GTK 3 kitaplıklarını kullanır.

### MailNix'ten geçiş

Uygulamanın önceki adı MailNix'ti. Zarf paketi kurulurken eski `mail-nix` veya `lynora` paketi kaldırılır; kişisel veri dizini korunur. İlk açılışta veriler
`~/.local/share/mailnix` klasöründen `~/.local/share/lynora` klasörüne taşınır. Anahtarlıktaki parolalar, oturumda başlatma
ve varsayılan e-posta uygulaması seçimi de yeni ada aktarılır. Eski `.mnbak` yedekleri geri yüklenebilir.
`MAILNIX_*` ortam değişkenleri artık okunmaz, bunların yerine `LYNORA_*` kullanılır.

## Kurumsal kullanım (Exchange + Active Directory)

1. Bilgisayar etki alanına katılmış olmalı (`realmd`/`sssd` veya `krb5-user` + `kinit`; Pardus'ta `pardus-domain-joiner` de kullanılabilir).
   Oturum açınca Kerberos bileti alınır (`klist` ile görülebilir).
2. Zarf ilk açıldığında kullanıcının adresini bulur ve Exchange'e parolasız bağlanır.
   Kerberos kullanılamıyorsa yalnızca bir kez parola sorulur (NTLM).
3. Microsoft 365 için kurumun Entra ID yöneticisi bir "genel istemci" uygulaması kaydetmelidir (yönlendirme adresi `http://localhost`).
   Uygulama kimliği **Ayarlar → Hesaplar → OAuth** bölümüne girilir. Derleme sırasında `LYNORA_MS_CLIENT_ID` ile pakete de gömülebilir.
4. İç ağda Autodiscover farklı bir adresteyse `LYNORA_AUTODISCOVER_URL` ortam değişkeni ile verilebilir.
   EWS adresi hesap ayarlarından elle de girilebilir.

**Parola değişince:** Kerberos kullanan hesaplar parola saklamaz, bu yüzden AD parolası değişince bir şey yapmak gerekmez.
Parolalı (NTLM/IMAP) hesaplarda sunucu parolayı reddettiği anda eşitleme durur ve klasör listesinde uyarı çıkar.
"Parolayı güncelle" ile yeni parola girilir. Zarf reddedilen parolayı tekrar tekrar denemez.

## Yeni dil eklemek

`locales/tr.json` dosyasını örneğin `locales/de.json` adıyla kopyalayıp çevirin. `@name` alanına dilin adını yazın.
Uygulamayı yeniden derlemeden denemek için dosyayı `~/.local/share/lynora/locales/` klasörüne koyun.
Sistem geneli için `/usr/share/lynora/locales/` kullanılır. Dil, **Ayarlar → Genel** bölümünden seçilir.

## Geliştirme

Kodun haritası (modüller, veri akışı, tablolar, ayar anahtarları, tuzaklar): [agent.md](agent.md).

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl file libssl-dev librsvg2-dev libayatana-appindicator3-dev
npm install
npm run tauri dev                      # geliştirme
packaging/build-packages.sh            # .deb (Docker ile Ubuntu 22.04 tabanında) → dist/
```

Testler:

```sh
cd src-tauri && cargo test             # birim testleri
npm run test:ui                        # arayüz testleri (Playwright, sahte arka uçla; sistemdeki Google Chrome kullanılır)
# Gerçek sunuculara karşı uçtan uca testler (isteğe bağlı): tests/e2e_mail.rs ve tests/e2e_dav.rs içindeki açıklamalara bakın
```

## Lisans

GPL-3.0-or-later
