# Cilt 1 Sozluk

Bu sozluk, cilt boyunca gecen teknik terimleri hizli tekrar icin toplar.

## A

- **ABI**: Uygulama ve cekirdek arasinda binary seviyede anlasma kurallari.
- **Admission Control**: Deadline task kabulundan once toplam yuk siniri kontrolu.
- **AEAD**: Hem sifreleme hem butunluk dogrulama saglayan yapi.

## B

- **Bandwidth (scheduler)**: Gorevin period basina kullanabilecegi CPU oranı.
- **Buddy Allocator**: Bloklari bolup birlestiren bellek tahsis modeli.
- **Boot Contract**: Firmware/bootloader ile kernel arasindaki baslangic sozlesmesi.

## C

- **CAS (Compare-And-Swap)**: Atomik kosullu yazma islemi.
- **CFS**: Vruntime tabanli adil scheduler modeli.
- **COW (Copy-on-Write)**: Yazana kadar paylasim yapan sayfa modeli.
- **CQ / CQE**: io_uring tamamlanma kuyrugu ve girdisi.

## D

- **Deadline Scheduler**: En yakin son tarihi onceleyen zamanlama modeli.
- **DMA**: Cihazin CPU'yu baypas ederek bellek erisimi yapmasi.
- **Deque (Chase-Lev)**: Owner ve stealer icin farkli uc kullanan lock-free kuyruk.

## E

- **EDF**: Earliest Deadline First.
- **EEVDF**: Earliest Eligible Virtual Deadline First.
- **Entropy**: Rastgelelik kalitesi.

## F

- **Fallback Chain**: Bir kaynak bulunamayinca yedek yol sirasi.
- **Fence**: Bellek islemlerinin gorunurluk sirasini koruyan bariyer.

## G

- **Guard Page**: Tasma yakalamak icin erisim yasakli sayfa.

## H

- **HKDF**: Anahtar turetme fonksiyonu ailesi.
- **HPACK**: HTTP/2 baslik sikistirma formati.

## I

- **IOMMU**: Cihaz bellek erisimlerini sinirlayan ceviri birimi.
- **io_uring**: SQ/CQ tabanli asenkron I/O modeli.

## K

- **Kswapd**: Arka planda bellek geri kazanimi yapan gorev.

## L

- **Lag (EEVDF)**: Gorevin sanal zaman dengesindeki konumu.
- **Lazy Fault**: Sayfa ilk erisimde map edilen fault sinifi.
- **Lock-Free**: Kilit almadan ilerleyen ama atomik ile dogru kalan yapi.

## M

- **MGLRU**: Cok nesilli LRU reclaim modeli.
- **Mitigasyon**: Dezavantaj etkisini azaltan muhendislik onlemi.

## N

- **Nonce**: Tekrar edilmemesi gereken sayi/deger.

## O

- **OOM**: Bellek tukendiginde son care mekanizmasi.
- **Ownership Boundary**: Kaynagin kime ait oldugu sinir.

## P

- **Page Fault**: Sayfa erisiminde map/izin hatasi olayi.
- **PMM**: Physical Memory Manager.
- **Preemption**: Calisan gorevin kesilip digerine gecilmesi.
- **Publication Boundary**: Veri gorunurluk sirasinin dogrulandigi nokta.

## Q

- **QUIC**: UDP uzerinde modern tasima protokolu.

## R

- **Refault**: Evict edilmis sayfanin kisa surede tekrar fault vermesi.
- **Reclaim**: Bellek geri kazanma islemi.
- **RT Scheduler**: Gercek zamanli politika yoneticisi.

## S

- **Scheduler**: CPU zamanini gorevler arasinda dagitan katman.
- **SeqCst**: Atomik ordering seviyelerinin en kati olanı.
- **SQ / SQE**: io_uring gonderim kuyrugu ve girdisi.
- **State Machine**: Durumlar ve gecis kurallari ile tanimli model.

## T

- **THP**: Transparent Huge Pages.
- **TLSF**: Two-Level Segregated Fit allocator.
- **TOCTOU**: Kontrol ile kullanim arasindaki zaman penceresi hatasi.

## U

- **Utilization (U=C/T)**: Deadline gorevin period basina kullanim orani.

## V

- **Vruntime**: CFS'te adalet icin tutulan sanal calisma zamani.

## W

- **Work Stealing**: Bos CPU'nun baska CPU kuyruundan is calmaya calismasi.
- **Writeback**: Kirli verinin disk benzeri kalici ortama geri yazilmasi.

## Z

- **Zone (memory)**: Donanim limitlerine gore ayrilmis fiziksel bellek sinifi.
- **ZSwap**: Disk swap oncesi RAM icinde sikistirma katmani.
