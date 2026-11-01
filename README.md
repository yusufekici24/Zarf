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

**Dosyalar**
- `.msg`, `.eml`, `.mbox`, `.ics`, `.vcf` açma ve içe aktarma; iletiyi `.msg`/`.eml` olarak kaydetme.
- `.pst` içe aktarma (`pst-utils` paketi gerekir).

**Diğer**
- Takvim (Exchange takvimi ile eşitleme, yinelenen etkinlikler, hatırlatıcılar), kişiler ve gruplar, görevler, notlar. Görev, not ve kişi grupları Exchange ile çift yönlü eşitlenir.
- **Yedekle / geri yükle:** Tüm veriler tek dosyada, isteğe bağlı parolayla AES-256-GCM şifreli (Dosya > Aç ve Dışarı Aktar).
- Türkçe ve İngilizce arayüz. Yeni dil eklemek kolaydır (aşağıya bakın).

## Kurulum

| Dağıtım | Paket |
|---|---|
| Debian tabanlı dağıtımlar: Debian 12 / 13, Ubuntu 22.04 / 24.04, Pardus 23 / 25 ve türevleri | `Lynora_*_amd64.deb` |

```sh
sudo apt install ./Lynora_1.0.54_amd64.deb      # bağımlılıklar otomatik kurulur
```

Paket Ubuntu 22.04 tabanında (glibc 2.35) derlenir. Bu yüzden daha yeni tüm Debian tabanlı dağıtımlarda sorunsuz çalışır.
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
# Gerçek sunuculara karşı uçtan uca testler (isteğe bağlı): tests/e2e_mail.rs içindeki açıklamalara bakın
```

## Lisans

GPL-3.0-or-later
