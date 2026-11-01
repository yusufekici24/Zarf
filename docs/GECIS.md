# Linux Posta → Lynora geçiş notu

Taban ürün Lynora'dır (Tauri + Svelte, bir dönem adı MailNix'ti). Linux Posta'daki (`yusufekici24/mailapp-linux`)
iyi özellikler buraya taşınıyor. Paket yalnızca `.deb`; RPM ve AppImage kaldırıldı.

## Taşınanlar

Takma adlar, şablonlar, güvenilir göndericiler, bul/değiştir, HTML kaynağı, emoji, yazım denetimi, çökme
kurtarma, IMAP IDLE, gelişmiş yerel kurallar, şifreli yedekleme, Exchange görev/not/kişi grubu eşitlemesi,
paylaşılan posta kutusu, S/MIME ve OpenPGP, CalDAV ve CardDAV, Kuralları Şimdi Uygula önizleme ve ilerlemesi.

Durum: derleme, `svelte-check` ve `cargo test --lib` geçiyor. CalDAV ve CardDAV gerçek bir Radicale sunucusuna
karşı (`tests/e2e_dav.rs`), S/MIME ve OpenPGP gerçek `openssl` ve `gpg` ile sınandı. Arayüz başsız Chrome'da taklit
arka uçla gözden geçirildi. Exchange, Nextcloud ve çalışan Tauri uygulamasında henüz denenmedi.

## Ubuntu'da önce denenecekler

1. Uygulama açılıyor mu, eski veritabanı yükseltiliyor mu (yeni sütunlar).
2. IMAP IDLE: yeni posta anında geliyor mu, ağ kopunca yeniden bağlanıyor mu.
3. Exchange: görev, not ve grup oluşturma, düzenleme, silme; Outlook'ta görünüyor mu.
4. Paylaşılan posta kutusu: klasörler, gönderim, takvim.
5. Yedek al, parolalı geri yükle, yeniden başlatma.
6. Yazım denetimi: `hunspell-tr` kuruluyken kırmızı alt çizgi çıkıyor mu.
7. Yazıp uygulamayı öldür: açılışta geri yükleme önerisi geliyor mu.
8. S/MIME: Exchange üzerinden imzalı ve şifreli gönderim, Outlook'ta açılıyor mu; Outlook'tan gelen iletiler.
9. OpenPGP: pinentry parola penceresi Tauri uygulamasından açılıyor mu; Thunderbird ile karşılıklı deneme.
10. CalDAV ve CardDAV: Nextcloud ve iCloud (uygulamaya özel parola) ile eşitleme.

## Kalan işler

Lynora'nın kendi eksikleri:

- Kişiler, Görevler ve Notlar şeritleri eski stilde, Giriş ve Yardım sekmeleri var
- HTML temizleme üç düzenli ifadeden ibaret, asıl koruma CSP ve sandbox (`bleach` benzeri izin listesi gerekli)
- Yinelenen etkinlikler beş kalıpla sınırlı
- Arayüz testi yok

## Bilinen sınırlar

- Mevcut yerel görev ve notlar yerel kalır; yalnızca yeni oluşturulanlar ilk Exchange hesabına yazılır.
- Paylaşılan kutuda anlık posta yok (zamanlayıcıyla denetlenir), görev ve notlar eşitlenmez.
- Not renkleri yalnızca bu bilgisayarda tutulur.
- CalDAV'da serinin taşınmış tek oluşumu görünür ama salt okunurdur, düzenleme seri üzerinden yapılır. CardDAV kişi
  grupları (KIND:group) eşitlenmez. Çakışmada son yazan kazanır.
- Kuralları Şimdi Uygula yalnızca yerel kuralları çalıştırır (Exchange kuralları sunucudadır), iletme ve bildirim
  eylemleri yalnızca yeni gelen iletilerde uygulanır. IMAP'te yalnızca bu bilgisayara inmiş iletiler taranır.
- "Şifrele" açıkken otomatik taslak ve çökme kurtarma durur: yanıttaki çözülmüş alıntı düz metin yazılmasın. Elle
  kaydedilen taslak şifrelenmez. İmzalı ya da şifreli iletide gönderimi geri alma bekletmesi uygulanmaz.
- S/MIME sertifika iptali (CRL/OCSP) denetlenmez. Şifreli iletiler yerel aramada bulunmaz.
- Yedek parolaları içermez: anahtarlık yoksa hesap parolaları geri yüklemede yeniden istenebilir.

## Geliştirme

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl file libssl-dev librsvg2-dev libayatana-appindicator3-dev
# Rust (rustup) ve Node 22 gerekir
npm install
npm run tauri dev
npx svelte-check --threshold error
(cd src-tauri && cargo test --lib)
packaging/build-packages.sh   # .deb: Docker ile Ubuntu 22.04 tabanında
```

`.deb` CI'da yalnızca `v*` etiketinde ya da Actions > build > Run workflow ile üretilir.
