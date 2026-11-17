# Sürüm geçmişi

Zarf'ın her sürümünde neyin değiştiği, kullanıcının gördüğü haliyle. En yeni sürüm en üsttedir. Paketler GitHub
**Releases** sayfasındadır; 2.0'dan önceki sürümlerin paketleri artık yayımlanmıyor, onların notları da bu dosyanın
sonundaki "2.0'a gelene kadar" bölümünde toplandı.

---

## Zarf 2.0

### 2.0.5 — 17 Kasım 2026

Bu sürüm iki önemli hatayı düzeltiyor: toplantı saatlerinin yanlış görünmesi ve yapıştırılan ekran görüntülerinin
boş gitmesi.

- **Toplantı saatleri artık doğru gidiyor.** Zarf'tan gönderilen toplantılar Exchange'e saat dilimi bildirilmeden
  gidiyordu. Exchange de toplantıyı Greenwich saatine göre kaydediyordu. Bu yüzden davet metninde saat 3 saat geri
  yazılıyor, tüm gün etkinlikler alıcıların takviminde bir gün önce görünüyor, sabah erken saatteki haftalık
  toplantılar yanlış güne düşebiliyordu. Toplantılar artık bilgisayarınızın saat dilimiyle (Türkiye) gönderiliyor.
  Bu sürümden önce gönderdiğiniz bir toplantı yanlış görünüyorsa saatini bir kez düzenleyip kaydetmeniz yeterli.
- **Toplantıyı düzenlemek yalnızca değiştirdiğiniz şeyi gönderiyor.** Önceden toplantıyı açıp hiçbir şeyi
  değiştirmeden kaydetmek bile bütün katılımcılara güncelleme gönderiyordu. Outlook'ta yazılmış toplantı metni
  (örneğin Teams bağlantısı) de düz metne dönüşüyordu. Artık yalnızca değişen bilgi gidiyor; hiçbir şey
  değişmediyse kimseye bir şey gitmiyor. "Özel" işareti ve kategori değişiklikleri de artık sunucuya yansıyor.
- **Yapıştırılan ekran görüntüsü alıcıya ulaşıyor.** Ekran görüntüsü yazma penceresinde görünüyordu ama iletiye
  gömülmediği için karşı tarafa boş gidiyordu. Bu hem yeni iletide hem yanıtta oluyordu. Resim artık yapıştırıldığı
  anda iletiye gömülüyor. Bir resim okunamazsa Zarf göndermeden önce sizi uyarıyor; resim sessizce kaybolmuyor.
- **Resimler Outlook'ta da gövdede görünüyor.** İleti içindeki resimler Outlook'un beklediği biçimde, adlarıyla
  (image001.png gibi) gönderiliyor. Önceden bazı alıcılarda resim gövdede değil, adsız bir ek olarak görünebiliyordu.
- Şeritteki **Yapıştır** düğmesi artık panodaki ekran görüntüsünü de yapıştırıyor.

### 2.0.4 — 17 Kasım 2026

- **Açılıştan hemen sonra ve pencere açtıktan sonra tıklamalar gecikmiyor.** Zarf, yeni ileti pencerelerini hızlı
  açabilmek için arka planda bir pencereyi önceden hazırlar. Bu hazırlık birkaç yüz milisaniye boyunca bütün
  pencereleri dondurduğu için tam o anda yapılan tıklamalar geç işleniyordu. Hazırlık artık siz bir şey yapmıyorken
  yapılıyor.
- Performans günlüğü (`~/.local/share/lynora/perf.log`) daha ayrıntılı ve daha doğru; gizli pencerede oluşan yanıltıcı
  "takılma" kayıtları artık yazılmıyor.

### 2.0.3 — 17 Kasım 2026

- **Açılışta yazıların değişmesi düzeldi.** Pencere, yazı tipi tamamen yüklendikten sonra gösteriliyor. Önceden
  açıldıktan kısa süre sonra yazılar yedek yazı tipinden Noto Sans'a geçiyor, ekran yakınlaşmış gibi görünüyordu.
- **Yeni gelen ileti hızlı açılıyor.** Yeni ileti gelince en yeni beş iletinin içeriği hemen indiriliyor. Önceden
  arka plandaki uzun işlerin (klasör taramaları, takvim, kişiler) bitmesini bekliyordu. İçerik yine de hemen
  gelmezse başlık ve önizleme anında görünüyor, altında "İleti indiriliyor…" yazıyor.
- **Performans günlüğü.** `~/.local/share/lynora/perf.log` dosyasına yalnızca yavaş olaylar yazılıyor: açılış süresi,
  takılmalar, geç işlenen tıklamalar, yavaş ileti açılışları ve yavaş Exchange istekleri. Adres, konu ya da ileti
  içeriği yazılmaz. Yavaşlık bildirirken bu dosyayı göndermeniz yeterli.

### 2.0.2 — 17 Kasım 2026

- **Gönderimi geri alma süresi.** Gönder'e bastıktan sonra ileti varsayılan olarak 10 saniye bekletiliyor. Süre
  Seçenekler > Posta > Gönderme bölümünden kapatılabilir ya da 5, 10, 20 veya 30 saniye yapılabilir. Ana pencerede
  çıkan bildirim kalan süreyi sayıyor: "Geri al" iletiyi taslağa döndürür, "Şimdi gönder" beklemeden gönderir.
  Önceden ileti seçilen süreden 30–60 saniye geç gidebiliyordu; artık tam zamanında gidiyor.
- **Ctrl+Z ve Ctrl+Y çalışıyor.** İleti yazarken ve Kime, Konu, arama gibi kutularda Ctrl+Z geri alıyor, Ctrl+Y ve
  Ctrl+Shift+Z yineliyor. Satır aralığı, büyük/küçük harf dönüşümü ve tablo işlemleri de geri alınabiliyor.
- **Daha hızlı arama.** "Ad soyad" gibi birden çok sözcüklü aramalar yaklaşık üç kat hızlandı; sonuçlar aynı.

### 2.0.1 — 17 Kasım 2026

- **Adınızın geçtiği iletiler renkli görünebiliyor.** Kime ya da Bilgi satırında adınız varsa ileti listede seçtiğiniz
  renkte (kırmızı, turuncu, yeşil, mor ya da mavi) görünür. Seçenekler > Posta > Okuma Bölmesi'nden açılır,
  varsayılan olarak kapalıdır. Exchange'de adres defterindeki adınız hesabınızdaki addan farklıysa "Aranacak ek adlar"
  alanına yazabilirsiniz.
- **Sürüklerken hedef klasör görünüyor.** İleti bir klasörün üzerine getirilince klasör vurgulanıyor, adı sürükleme
  etiketinde yazıyor. Kapalı bir klasörün üzerinde beklenince alt klasörleri açılıyor.
- **Okunmamış iletilerin üzerine gelince kalınlığın kaybolması (Ubuntu 24.04)** için yazı tipi değiştirildi.
- Uygulama menüsünde **"lynora" yazınca da Zarf bulunuyor.**

### 2.0.0 — 17 Kasım 2026

Uygulamanın yeni adı **Zarf**. Lynora'dan yükseltince veriler, hesaplar ve parolalar olduğu gibi kalır.

- Yeni kırmızı-gri logo ve açılış ekranı.
- **Toplantı zamanlayıcısı grupları açıyor.** Davete bir grup ya da dağıtım listesi eklenince zamanlayıcı grubun
  üyelerini tek tek gösteriyor; iç içe gruplar da açılıyor. Müsaitliği bilinmeyen kişi boş sayılmıyor.
- Boş/meşgul bilgisi, toplantının oluşturulduğu takvim hesabıyla soruluyor.
- **Kişi düzenleme.** Kendi kişileriniz ve CardDAV kişileri düzenlenebiliyor. Exchange adres defteri kayıtları
  değiştirilemez ama kişisel bir kopyası oluşturulabilir. Aynı adresle ikinci bir kişi eklenmeden önce uyarı çıkıyor.
- **Yeni ileti düzenleyicisi.** Renk paletleri, biçim boyacısı, hazır stiller (Başlık, Alıntı, Kod …) ve tablo araçları.
- Klasör sayaçları ileti listesini artık yavaşlatmıyor.

---

## 2.0'a gelene kadar

Uygulama ilk olarak **MailNix** adıyla Pardus için yazıldı. 1.1.0 ile **Lynora** adını aldı ve bütün Debian tabanlı
dağıtımlar için hazırlanmaya başladı. 2.0.0 ile adı **Zarf** oldu. Bu sürümlerin paketleri artık yayımlanmıyor;
aşağıda her birinde nelerin yapıldığı duruyor.

### Lynora 1.7.6 — 11 Kasım 2026

- **Exchange'in "sunucu şu anda bu isteği karşılayamıyor" hatası düzeldi.** Sunucu yoğun olduğunu ve ne kadar
  beklenmesi gerektiğini bildirdiğinde Lynora artık bu süreye uyuyor. Önceden beklemeden istek göndermeye devam
  ediyor, bu da sunucunun sınırını daha da uzatıyordu.
- Sınıra takılan klasör taraması süre dolunca kaldığı yerden kendiliğinden sürüyor.
- Hata her işlemde tekrar tekrar gösterilmiyor; en çok 15 dakikada bir, eşitlemenin ne zaman süreceğini söyleyen tek
  bir bildirim çıkıyor. Hata veri kaybına yol açmıyordu.

### Lynora 1.7.5 — 10 Kasım 2026

- **Gelişmiş arama.** Arama kutusu bileşik sorguları anlıyor: `kimden:ali (fatura VEYA makbuz) -taslak`. Kişi, konu,
  gövde, ek adı, kategori, klasör, durum ve önem alanlarında aranabiliyor; `tarih:bu hafta`, `tarih:geçen ay`,
  `tarih:7g` gibi tarihler ve İngilizce karşılıkları (`from:`, `subject:`, `has:attachment`) da çalışıyor.
- **Arama önerileri.** Yazarken alan adları, alanların değerleri ve kişiler öneriliyor.
- Adresle yapılan arama, listede yalnızca adıyla görünen kurum içi göndericileri de buluyor.
- **Tablolar düzgün görünüyor.** Tablolu iletilerde toplam satırı ve kenarlıklar korunuyor, dar bölmede sütunlar
  harf harf alt alta dizilmiyor.
- **Konuşmalar doğru kuruluyor.** İletiler artık yalnızca konuya göre değil, birbirine yanıt olup olmadıklarına göre
  bağlanıyor. Her gün aynı konuyla gelen raporlar tek konuşmada birleşmiyor.
- Konuşmanın en üstteki iletisi açılınca konuşmadaki bütün okunmamışlar okundu oluyor.

### Lynora 1.5.0 — 6 Kasım 2026

- **Renk teması.** Üst çubuktaki palet düğmesinden tek tıkla renk seçiliyor: mavi, mor, kırmızı, pembe, turuncu,
  yeşil, turkuaz, füme. Aynı yerden açık, koyu ya da sistem görünümü de seçiliyor. Seçim bütün pencerelere hemen
  yansıyor ve kalıcı oluyor.

### Lynora 1.4.0 — 6 Kasım 2026

- **Okunan iletinin kalın kalması düzeldi.** Windows yazı tipleri kurulu bilgisayarlarda bütün yazılar kalın
  görünüyor, okunmuş ve okunmamış iletiler ayırt edilemiyordu. Varsayılan yazı tipi artık pakete gömülü Noto Sans.
  Ayrıca bağlantı koptuğunda (VPN, uyku) okundu bilgisi kayboluyordu; artık bağlantı gelince sunucuya gönderiliyor.
- **Yoğun takvimlerde toplantılar görünüyor.** Tekrarlayan toplantısı çok olan takvimlerde Exchange'in 1000 öğelik
  sınırı aşıldığı için takvim hiç eşitlenmiyordu. Takvim artık parça parça isteniyor; eşitlenemezse nedeni takvimin
  üstünde yazıyor ve "Yeniden dene" ile tekrarlanabiliyor.
- **Outlook Web gibi konuşma görünümü.** Konuşmanın iletileri (gönderdiğiniz yanıtlar dahil) listede ve okuma
  bölmesinde alt alta açılıyor.
- **Yazı tipi ayarları.** Arayüzün ve iletilerin yazı tipi ile boyutu Seçenekler'den değiştirilebiliyor.

### Lynora 1.3.1 — 5 Kasım 2026

- **Kurumunuza bir kez güvenin.** Uzak resim uyarısında "@sirket.com alan adına güven" düğmesi var: bir kez basınca o
  kurumdan gelen bütün iletilerdeki resimler sorulmadan iniyor. Gmail, Outlook.com gibi herkese açık servislerde bu
  düğme çıkmıyor.
- Güven Merkezi'ne alan adı (`sirket.com`, `*.sirket.com`) yazılabiliyor; alt alan adları da kapsanıyor.

### Lynora 1.3.0 — 5 Kasım 2026

Büyük posta kutularındaki yavaşlığı gideren sürüm (1.2.0'daki düzeltmeyi de içerir; 1.2.0 ayrıca yayımlanmadı).

- **Hızlı liste, arama ve konuşmalar.** Klasör açmak, kaydırmak ve sıralamak posta kutusunun boyutundan bağımsız
  oldu. Arama dizinden yapılıyor; Türkçe harf ve aksan farkı gözetilmiyor (`AYDIN` ~ `aydın` ~ `aydin`).

  | İşlem | 1.1.0 | 1.3.0 |
  |---|---|---|
  | Klasörü açma (6 bin ileti, 1,5 GB) | 186 ms | 1–2 ms |
  | Tüm kutularda arama (30 bin ileti) | 1–5 sn | 3–50 ms |
  | Konuşma görünümü (30 bin ileti) | 130 ms | 9–12 ms |
  | Bir konuşmayı açma | 127 ms | < 0,1 ms |

- **Veritabanını küçültme.** Seçenekler > Gelişmiş > Veritabanı bölümünde silinen postaların bıraktığı boş alan
  görünüyor ve geri alınabiliyor.
- **Güvenli HTML.** HTML iletiler gösterilmeden önce temizleniyor: betikler ve tehlikeli bağlantılar atılıyor, biçim
  korunuyor.
- 200 ms'yi aşan veritabanı işlemleri `slow-queries.log` dosyasına yazılıyor (ileti içeriği yazılmaz).
- 1.1.0'dan yükseltince veritabanı ilk açılışta bir kez, birkaç saniyede yeni düzene geçiyor. Yeni düzendeki
  veritabanını 1.1.0 açamaz; geri dönmek için yükseltmeden önce yedek alınması gerekiyordu.

### Lynora 1.1.0 — 1 Kasım 2026

Lynora adıyla ilk sürüm. Uygulamanın önceki adı MailNix'ti; artık yalnızca Pardus için değil, bütün Debian tabanlı
dağıtımlar için hazırlanıyor.

- **İmzalı ve şifreli posta (S/MIME, OpenPGP).** İmzalar doğrulanıyor, şifreli iletiler açılıyor; yazarken ileti
  imzalanıp şifrelenebiliyor. Sertifikalar ve anahtarlar Güven Merkezi'nden yönetiliyor. Çözülen içerik diske
  yazılmıyor.
- **CalDAV ve CardDAV.** IMAP hesaplarında takvim ve kişiler Nextcloud, Radicale gibi sunucularla eşitleniyor.
- **Kuralları Şimdi Uygula.** Kurallar mevcut iletilere önizlemeyle, ilerleme çubuğuyla ve iptal edilebilir şekilde
  uygulanıyor.
- MailNix'ten geçişte veriler, parolalar ve ayarlar kendiliğinden taşınıyor; eski yedekler geri yüklenebiliyor.
