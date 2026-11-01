# MailNix

Pardus için e-posta, takvim, kişiler, görevler ve notlar uygulaması.
Kurumsal **Microsoft Exchange** desteği vardır ve **.msg** ileti dosyalarını açar.

Altyapı: **Rust + Tauri 2** (arka uç), **Svelte 5 + Tailwind CSS + shadcn-svelte** (arayüz).

## Özellikler

**Hesaplar**
- Microsoft Exchange (şirket içi Exchange 2013+ ve Microsoft 365) EWS ile; IMAP/SMTP (Gmail, Hotmail, Yandex vb.).
- **Otomatik kurulum:** İlk açılışta oturum açmış kullanıcının adresi bulunur (Kerberos/SSSD/krb5.conf).
  Ayarlar Autodiscover ile alınır. Kurum ağında Kerberos ile **parola sorulmadan** bağlanılır.
- Kimlik doğrulama: Kerberos (GSSAPI/SPNEGO, tek oturum açma), NTLMv2, parola, OAuth2 (Microsoft 365 / Google).
- **Hesap kilitlenme koruması:** Sunucu parolayı reddederse (ör. AD'de parola değişti) hesap duraklatılır.
  Eski parola bir daha gönderilmez, böylece AD hesabı kilitlenmez. Yeni parola girildiğinde eşitleme yeniden başlar.

**Posta**
- Sık Kullanılanlar, klasör ağacı, arama klasörleri; konuşma görünümü; kurallar; kategoriler; bayrak ve takip.
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
- Takvim (Exchange takvimi ile eşitleme, yinelenen etkinlikler, hatırlatıcılar), kişiler (Exchange kişileri), görevler, notlar.
- Türkçe ve İngilizce arayüz. Yeni dil eklemek kolaydır (aşağıya bakın).

## Kurulum

| Dağıtım | Paket |
|---|---|
| Pardus 23 / 25, Debian 12 / 13, Ubuntu 22.04 / 24.04 ve türevleri | `MailNix_*_amd64.deb` |

```sh
sudo apt install ./MailNix_1.0.54_amd64.deb      # bağımlılıklar otomatik kurulur
```

Paket Ubuntu 22.04 tabanında (glibc 2.35) derlenir. Bu yüzden daha yeni tüm Pardus/Debian/Ubuntu sürümlerinde sorunsuz çalışır.
`.deb` paketi sistemdeki WebKitGTK 4.1 ve GTK 3 kitaplıklarını kullanır.

## Kurumsal kullanım (Exchange + Active Directory)

1. Bilgisayar etki alanına katılmış olmalı (Pardus'ta `pardus-domain-joiner`, `realmd`/`sssd` veya `krb5-user` + `kinit`).
   Oturum açınca Kerberos bileti alınır (`klist` ile görülebilir).
2. MailNix ilk açıldığında kullanıcının adresini bulur ve Exchange'e parolasız bağlanır.
   Kerberos kullanılamıyorsa yalnızca bir kez parola sorulur (NTLM).
3. Microsoft 365 için kurumun Entra ID yöneticisi bir "genel istemci" uygulaması kaydetmelidir (yönlendirme adresi `http://localhost`).
   Uygulama kimliği **Ayarlar → Hesaplar → OAuth** bölümüne girilir. Derleme sırasında `MAILNIX_MS_CLIENT_ID` ile pakete de gömülebilir.
4. İç ağda Autodiscover farklı bir adresteyse `MAILNIX_AUTODISCOVER_URL` ortam değişkeni ile verilebilir.
   EWS adresi hesap ayarlarından elle de girilebilir.

**Parola değişince:** Kerberos kullanan hesaplar parola saklamaz, bu yüzden AD parolası değişince bir şey yapmak gerekmez.
Parolalı (NTLM/IMAP) hesaplarda sunucu parolayı reddettiği anda eşitleme durur ve klasör listesinde uyarı çıkar.
"Parolayı güncelle" ile yeni parola girilir. MailNix reddedilen parolayı tekrar tekrar denemez.

## Yeni dil eklemek

`locales/tr.json` dosyasını örneğin `locales/de.json` adıyla kopyalayıp çevirin. `@name` alanına dilin adını yazın.
Uygulamayı yeniden derlemeden denemek için dosyayı `~/.local/share/mailnix/locales/` klasörüne koyun.
Sistem geneli için `/usr/share/mailnix/locales/` kullanılır. Dil, **Ayarlar → Genel** bölümünden seçilir.

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
