# Linux Posta → MailNix geçiş notu

Taban ürün MailNix'tir (eski adı Lynora, Tauri + Svelte). Linux Posta'daki (`yusufekici24/mailapp-linux`)
iyi özellikler buraya taşınıyor. Paket yalnızca `.deb`; RPM ve AppImage kaldırıldı.

## Taşınanlar

Takma adlar, şablonlar, güvenilir göndericiler, bul/değiştir, HTML kaynağı, emoji, yazım denetimi, çökme
kurtarma, IMAP IDLE, gelişmiş yerel kurallar, şifreli yedekleme, Exchange görev/not/kişi grubu eşitlemesi,
paylaşılan posta kutusu.

Durum: derleme, `svelte-check` ve `cargo test --lib` CI'da geçiyor. Hiçbiri gerçek sunucuya karşı ya da
çalışan uygulamada denenmedi.

## Ubuntu'da önce denenecekler

1. Uygulama açılıyor mu, eski veritabanı yükseltiliyor mu (yeni sütunlar).
2. IMAP IDLE: yeni posta anında geliyor mu, ağ kopunca yeniden bağlanıyor mu.
3. Exchange: görev, not ve grup oluşturma, düzenleme, silme; Outlook'ta görünüyor mu.
4. Paylaşılan posta kutusu: klasörler, gönderim, takvim.
5. Yedek al, parolalı geri yükle, yeniden başlatma.
6. Yazım denetimi: `hunspell-tr` kuruluyken kırmızı alt çizgi çıkıyor mu.
7. Yazıp uygulamayı öldür: açılışta geri yükleme önerisi geliyor mu.

## Kalan işler

Linux Posta'dan:

- S/MIME ve OpenPGP (`linux_posta/services/crypto.py`: `gpg` ve `openssl` komutları)
- CalDAV ve CardDAV
- Kuralları Şimdi Çalıştır için önizleme ve ilerleme

MailNix'in kendi eksikleri:

- Kişiler, Görevler ve Notlar şeritleri eski stilde, Giriş ve Yardım sekmeleri var
- HTML temizleme üç düzenli ifadeden ibaret, asıl koruma CSP ve sandbox (`bleach` benzeri izin listesi gerekli)
- Yinelenen etkinlikler beş kalıpla sınırlı
- Arayüz testi yok

## Bilinen sınırlar

- Mevcut yerel görev ve notlar yerel kalır; yalnızca yeni oluşturulanlar ilk Exchange hesabına yazılır.
- Paylaşılan kutuda anlık posta yok (zamanlayıcıyla denetlenir), görev ve notlar eşitlenmez.
- Not renkleri yalnızca bu bilgisayarda tutulur.
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
