# Lynora

Debian tabanlı Linux dağıtımları için e-posta, takvim, kişiler, görevler ve notlar uygulaması.
Kurumsal **Microsoft Exchange** desteği vardır ve **.msg** ileti dosyalarını açar.

Altyapı: **Rust + Tauri 2** (arka uç), **Svelte 5 + Tailwind CSS + shadcn-svelte** (arayüz).

## Özellikler

**Hesaplar**
- Microsoft Exchange (şirket içi Exchange 2013+ ve Microsoft 365) EWS ile; IMAP/SMTP (Gmail, Hotmail, Yandex vb.).
- **Otomatik kurulum:** İlk açılışta oturum açmış kullanıcının adresi bulunur (Kerberos/SSSD/krb5.conf).
  Ayarlar Autodiscover ile alınır. Kurum ağında Kerberos ile **parola sorulmadan** bağlanılır.
- Kimlik doğrulama: Kerberos (GSSAPI/SPNEGO, tek oturum açma), NTLMv2, parola, OAuth2 (Microsoft 365 / Google).
- **Paylaşılan posta kutusu:** Tam Erişim yetkiniz olan kutu kendi kimliğinizle ayrı hesap gibi eklenir.
- **Kimlikler (takma adlar):** Bir hesaba ek "Kimden" adresleri, yanıt adresi ve kimliğe özel imza.
- **Hesap kilitlenme koruması:** Sunucu parolayı reddederse (ör. AD'de parola değişti) hesap duraklatılır.
  Eski parola bir daha gönderilmez, böylece AD hesabı kilitlenmez. Yeni parola girildiğinde eşitleme yeniden başlar.

**Posta**
- Sık Kullanılanlar, klasör ağacı, arama klasörleri; konuşma görünümü; kurallar; kategoriler; bayrak ve takip.
- **Anlık posta:** Exchange akış aboneliği ve IMAP IDLE ile yeni ileti sunucuya düştüğü anda gelir.
- **Kurallar:** Exchange kuralları sunucuda çalışır. IMAP için yerel kurallar: çoklu koşul, ve/veya, düzenli ifade, özel durumlar.
- **Kuralları Şimdi Uygula:** Yerel kurallar seçilen klasördeki mevcut iletilere uygulanır. Alt klasörler ve okunmuş/okunmamış seçilebilir. **Önizle** hiçbir şeyi değiştirmeden hangi iletiye ne yapılacağını gösterir; çalıştırma ilerleme çubuğuyla sürer ve iptal edilebilir.
- **Şablonlar ve düzenleyici:** İleti şablonları, bul/değiştir, HTML kaynağı, emoji, yazım denetimi (`hunspell-tr`).
- **Çökme kurtarma:** Yazılan ileti yerelde saklanır, uygulama çökerse açılışta geri yükleme önerilir.
- **Güvenilir göndericiler:** Adres ya da alan adı; bu kaynaklardan gelen resimler sorulmadan iner.
- **Alıcı önerileri:** Yazarken kişiler, son kullanılan adresler ve kurum dizini (GAL) önerilir. Adresler "chip" olarak görünür.
- **İletiyi yeni iletiye sürükleyince .msg eki** olarak eklenir. "Ek olarak ilet" de .msg kullanır.
- Gönderimi geri alma, teslimi geciktirme, taslakların otomatik kaydı, imzalar, okundu bilgisi, önem düzeyi.
- Toplantı davetlerini Kabul Et / Belki / Reddet ile yanıtlama.
- Ofis dışında yanıtı (Exchange'de sunucuda çalışır).
- Arama işleçleri: `kimden:ali konu:"aylık rapor" has:attachment is:unread after:2026-01-01`.
  İngilizce karşılıkları da çalışır: `from:`, `to:`, `subject:`, `body:`, `category:`, `before:`, `on:`.

**Güvenlik**
- **S/MIME ve OpenPGP:** İmzalı ve şifreli iletiler okunur: imza doğrulanır, şifreli ileti çözülür. Okuma bölmesindeki çubuk imzanın geçerli, güvenilmeyen ya da geçersiz olduğunu ve imzalayanı gösterir.
- Yazarken **Seçenekler** sekmesinde *İmzala*, *Şifrele* ve yöntem seçimi. Şifreli iletiye yanıt otomatik şifrelenir, Gönderilmiş kopyasını siz de okuyabilirsiniz.
- **Ayarlar → Güven Merkezi:** Kişisel S/MIME sertifikası (.p12/.pfx) içe aktarma, OpenPGP anahtarı seçimi, varsayılan imzalama. Alıcı sertifikaları imzalı iletilerden otomatik toplanır, elle de eklenebilir.
- Çözülen içerik diske ve veritabanına yazılmaz. Özel anahtar şifreli saklanır, parolası anahtarlıktadır. `openssl` ve `gnupg` paketleri gerekir (paket önerir).

**Dosyalar**
- `.msg`, `.eml`, `.mbox`, `.ics`, `.vcf` açma ve içe aktarma; iletiyi `.msg`/`.eml` olarak kaydetme.
- `.pst` içe aktarma (`pst-utils` paketi gerekir).

**Diğer**
- Takvim (Exchange takvimi ile eşitleme, yinelenen etkinlikler, hatırlatıcılar), kişiler ve gruplar, görevler, notlar. Görev, not ve kişi grupları Exchange ile çift yönlü eşitlenir.
- **CalDAV ve CardDAV:** IMAP hesaplarında takvim ve kişiler Nextcloud, Radicale gibi sunucularla çift yönlü eşitlenir. Hesabın gelişmiş ayarlarına sunucu adresi yazılır, takvimler ve adres defterleri kendiliğinden bulunur. Yerelde değişen özellikler dışında sunucudaki nesne korunur.
- **Yedekle / geri yükle:** Tüm veriler tek dosyada, isteğe bağlı parolayla AES-256-GCM şifreli (Dosya > Aç ve Dışarı Aktar).
- Türkçe ve İngilizce arayüz. Yeni dil eklemek kolaydır (aşağıya bakın).

## Kurulum

| Dağıtım | Paket |
|---|---|
| Ubuntu 24.04 ve sonrası, Debian 13 ve türevleri (glibc 2.39+) | `Lynora_*_amd64.deb` |

```sh
sudo apt install ./Lynora_1.1.0_amd64.deb      # bağımlılıklar otomatik kurulur
```

Paketler GitHub **Releases** sayfasında yayımlanır. Debian 12, Ubuntu 22.04 ve Pardus 23 gibi eski dağıtımlar için
paket Ubuntu 22.04 tabanında üretilir: `packaging/build-packages.sh` (Docker) ya da Actions > build > Run workflow.
`.deb` paketi sistemdeki WebKitGTK 4.1 ve GTK 3 kitaplıklarını kullanır.

### MailNix'ten geçiş

Uygulamanın önceki adı MailNix'ti. Lynora paketi kurulurken eski `mail-nix` paketi kaldırılır. İlk açılışta veriler
`~/.local/share/mailnix` klasöründen `~/.local/share/lynora` klasörüne taşınır. Anahtarlıktaki parolalar, oturumda başlatma
ve varsayılan e-posta uygulaması seçimi de yeni ada aktarılır. Eski `.mnbak` yedekleri geri yüklenebilir.
`MAILNIX_*` ortam değişkenleri artık okunmaz, bunların yerine `LYNORA_*` kullanılır.

## Kurumsal kullanım (Exchange + Active Directory)

1. Bilgisayar etki alanına katılmış olmalı (`realmd`/`sssd` veya `krb5-user` + `kinit`; Pardus'ta `pardus-domain-joiner` de kullanılabilir).
   Oturum açınca Kerberos bileti alınır (`klist` ile görülebilir).
2. Lynora ilk açıldığında kullanıcının adresini bulur ve Exchange'e parolasız bağlanır.
   Kerberos kullanılamıyorsa yalnızca bir kez parola sorulur (NTLM).
3. Microsoft 365 için kurumun Entra ID yöneticisi bir "genel istemci" uygulaması kaydetmelidir (yönlendirme adresi `http://localhost`).
   Uygulama kimliği **Ayarlar → Hesaplar → OAuth** bölümüne girilir. Derleme sırasında `LYNORA_MS_CLIENT_ID` ile pakete de gömülebilir.
4. İç ağda Autodiscover farklı bir adresteyse `LYNORA_AUTODISCOVER_URL` ortam değişkeni ile verilebilir.
   EWS adresi hesap ayarlarından elle de girilebilir.

**Parola değişince:** Kerberos kullanan hesaplar parola saklamaz, bu yüzden AD parolası değişince bir şey yapmak gerekmez.
Parolalı (NTLM/IMAP) hesaplarda sunucu parolayı reddettiği anda eşitleme durur ve klasör listesinde uyarı çıkar.
"Parolayı güncelle" ile yeni parola girilir. Lynora reddedilen parolayı tekrar tekrar denemez.

## Yeni dil eklemek

`locales/tr.json` dosyasını örneğin `locales/de.json` adıyla kopyalayıp çevirin. `@name` alanına dilin adını yazın.
Uygulamayı yeniden derlemeden denemek için dosyayı `~/.local/share/lynora/locales/` klasörüne koyun.
Sistem geneli için `/usr/share/lynora/locales/` kullanılır. Dil, **Ayarlar → Genel** bölümünden seçilir.

## Geliştirme

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl file libssl-dev librsvg2-dev libayatana-appindicator3-dev
npm install
npm run tauri dev                      # geliştirme
packaging/build-packages.sh            # .deb (Docker ile Ubuntu 22.04 tabanında) → dist/
```

Testler:

```sh
cd src-tauri && cargo test             # birim testleri
# Gerçek sunuculara karşı uçtan uca testler (isteğe bağlı): tests/e2e_mail.rs ve tests/e2e_dav.rs içindeki açıklamalara bakın
```

## Lisans

GPL-3.0-or-later
