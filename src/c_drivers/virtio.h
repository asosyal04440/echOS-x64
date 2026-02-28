/*
 * echOS VirtIO Blok Aygıt Sürücüsü — Başlık Dosyası
 *
 * Bu dosya VirtIO Legacy (v0) PCI blok aygıtı protokolü için
 * register ofsetlerini, durum bayraklarını, tanımlayıcı (descriptor)
 * bayraklarını ve veri yapılarını tanımlar.
 *
 * VirtIO Standart: https://docs.oasis-open.org/virtio/virtio/v1.1/
 * (Bu uygulama Legacy/v0 arayüzünü kullanır.)
 *
 * PCI I/O Adres Uzayı Haritası (taban adresine göre ofsetler):
 *
 *  taban + 0x00  --> HOST_FEATURES  (cihazın desteklediği özellikler, RO)
 *  taban + 0x04  --> GUEST_FEATURES (sürücünün talep ettiği özellikler, WR)
 *  taban + 0x08  --> QUEUE_PFN      (virtqueue fiziksel sayfa numarası, WR)
 *  taban + 0x0C  --> QUEUE_NUM      (virtqueue boyutu, RO)
 *  taban + 0x0E  --> QUEUE_SEL      (aktif virtqueue seçici, WR)
 *  taban + 0x10  --> QUEUE_NOTIFY   (virtqueue bildiri kapısı, WR)
 *  taban + 0x12  --> STATUS         (cihaz durum register'ı, RW)
 *  taban + 0x13  --> ISR            (kesinti durum register'ı, RO)
 *  taban + 0x14  --> DEVICE_CFG     (aygıta özgü yapılandırma, RO)
 *  taban + 0x28  --> GUEST_PAGE_SIZE (misafir sayfa boyutu, WR)
 */

#pragma once
#include <stdint.h>

/* ============================================================================
 * VirtIO Halka Boyutu
 * ============================================================================
 *
 * Bir virtqueue'daki maksimum tanımlayıcı (descriptor) sayısı.
 * 8 seçildi: basit bir sürücü için yeterli, bellek kullanımı asgari.
 * Üretim sürücülerinde tipik değer 64, 128 veya 256'dır.
 */
#define VIRTIO_RING_SIZE 8

/* ============================================================================
 * VirtIO PCI Register Ofsetleri (taban adresine göre)
 * ============================================================================ */

/* Cihazın desteklediği özelliklerin bit maskesi (read-only) */
#define VIRTIO_PCI_HOST_FEATURES  0x00
/* Sürücünün talep ettiği özelliklerin bit maskesi (write) */
#define VIRTIO_PCI_GUEST_FEATURES 0x04
/* Aktif virtqueue için fiziksel sayfa numarası (write) */
#define VIRTIO_PCI_QUEUE_PFN      0x08
/* Seçili virtqueue'nun maksimum tanımlayıcı kapasitesi (read) */
#define VIRTIO_PCI_QUEUE_NUM      0x0C
/* Hangi virtqueue'nun seçili olduğu (write) */
#define VIRTIO_PCI_QUEUE_SEL      0x0E
/* Seçili virtqueue'ya yeni tanımlayıcı eklendiğini cihaza bildir (write) */
#define VIRTIO_PCI_QUEUE_NOTIFY   0x10
/* Cihaz durum register'ı — başlatma ve hata adımlarını izler (read/write) */
#define VIRTIO_PCI_STATUS         0x12
/* Kesinti Servis Yordamı durumu — kesintinin sebebini belirtir (read-clears) */
#define VIRTIO_PCI_ISR            0x13
/* Aygıta özgü yapılandırma alanı — blok için disk boyutu vb. (read-only) */
#define VIRTIO_PCI_DEVICE_CFG     0x14
/* Misafir işletim sisteminin sayfa boyutu — PFN hesaplamaları için (write) */
#define VIRTIO_PCI_GUEST_PAGE_SIZE 0x28

/* ============================================================================
 * VirtIO Durum Bayrağı Değerleri (STATUS Register)
 *
 * Başlatma sırasında bu bayraklar sırayla ayarlanır.
 * Hata durumunda FAILED bayrağı ayarlanır ve cihaz durur.
 *
 * Başlatma Sırası:
 *  0 (sıfırla) -> ACKNOWLEDGE -> ACKNOWLEDGE|DRIVER -> ... -> DRIVER_OK
 * ============================================================================ */

/* Cihaz algılandı (OS gördü) */
#define VIRTIO_STATUS_ACKNOWLEDGE  0x01
/* Sürücü yüklendi ve cihazı nasıl kullanacağını biliyor */
#define VIRTIO_STATUS_DRIVER       0x02
/* Sürücü hazır ve G/Ç isteklerini kabul edebilir */
#define VIRTIO_STATUS_DRIVER_OK    0x04
/* Özellik müzakeresi tamamlandı ve cihaz kabul etti */
#define VIRTIO_STATUS_FEATURES_OK  0x08
/* Başlatma başarısız — cihaz ile sürücü uyumsuz ya da hata oluştu */
#define VIRTIO_STATUS_FAILED       0x80

/* ============================================================================
 * Virtqueue Tanımlayıcı Bayrağı Değerleri (virtq_desc.flags)
 *
 * Bu bayraklar descriptor zincirinin nasıl yorumlanacağını belirtir.
 * ============================================================================ */

/* Bu tanımlayıcıdan sonra başka bir tanımlayıcı var (zincir devam ediyor) */
#define VIRTQ_DESC_F_NEXT     1
/* Bu tampon cihaz tarafından YAZILACAK (sürücü için write = cihaz mesajı alır) */
#define VIRTQ_DESC_F_WRITE    2
/* Bu tanımlayıcı dolaylı (indirect) bir descriptor tablosuna işaret ediyor */
#define VIRTQ_DESC_F_INDIRECT 4

/* ============================================================================
 * VirtIO Blok İstek Türleri (virtio_blk_req.type)
 * ============================================================================ */

/* Diskten veri OKU (cihaz → bellek) */
#define VIRTIO_BLK_T_IN  0
/* Diske veri YAZ (bellek → cihaz) */
#define VIRTIO_BLK_T_OUT 1

/* ============================================================================
 * VirtIO Veri Yapıları
 *
 * Tüm yapılar __attribute__((packed)) ile tanımlanmıştır:
 * derleyici dolgular (padding) ekleyemez; yapı üyeleri bellekte
 * tam olarak ardışık konumlanır. Bu, donanım/protokol uyumluluğu için zorunludur.
 * ============================================================================ */

/*
 * Virtque Tanımlayıcı (Descriptor):
 *
 * Her giriş, cihaza gönderilecek veya cihazdan alınacak bir bellek tamponunu tanımlar.
 * Tanımlayıcılar `next` alanıyla zincir oluşturabilir.
 *
 *  +--------+--------+--------+--------+
 *  |  addr  |  len   | flags  | next   |
 *  | 8 bayt | 4 bayt | 2 bayt | 2 bayt |
 *  +--------+--------+--------+--------+
 */
struct virtq_desc {
    uint64_t addr;   /* Fiziksel tampon adresi */
    uint32_t len;    /* Tampon boyutu (bayt) */
    uint16_t flags;  /* VIRTQ_DESC_F_* bayrağı kombinasyonu */
    uint16_t next;   /* F_NEXT ayarlıysa: bir sonraki tanımlayıcının indeksi */
} __attribute__((packed));

/*
 * Mevcut Halka (Available Ring):
 *
 * Sürücünün cihaza gönderdiği tanımlayıcı zinciri başlarının listesi.
 * Sürücü `ring[]`'e yazar ve `idx`'i artırır; cihaz `idx`'i izler.
 *
 *  +-------+-----+------------------+--------+
 *  | flags | idx | ring[RING_SIZE]  | unused |
 *  | 2 B   | 2 B | 2*RING_SIZE B   | 2 B    |
 *  +-------+-----+------------------+--------+
 */
struct virtq_avail {
    uint16_t flags;                   /* Kesinti devre dışı bayrağı (şu an kullanılmıyor) */
    uint16_t idx;                     /* Sürücünün eklediği son descriptor'ın indeksi + 1 */
    uint16_t ring[VIRTIO_RING_SIZE];  /* Her slot: bir descriptor zincirinin baş indeksi */
    uint16_t unused;                  /* Hizalama dolgusu */
} __attribute__((packed));

/*
 * Kullanılmış Halka Elemanı (Used Ring Element):
 *
 * Cihaz, işlediği her tanımlayıcı zinciri için bir giriş ekler.
 */
struct virtq_used_elem {
    uint32_t id;   /* İşlenen descriptor zincirinin baş indeksi */
    uint32_t len;  /* Cihazın yazdığı toplam bayt sayısı */
} __attribute__((packed));

/*
 * Kullanılmış Halka (Used Ring):
 *
 * Cihazın işlediği isteklerin listesi. Sürücü `idx`'i izlerek
 * yeni tamamlanan istekleri saptar.
 *
 *  +-------+-----+----------------------------------+
 *  | flags | idx | ring[RING_SIZE * used_elem boyut] |
 *  | 2 B   | 2 B | ...                              |
 *  +-------+-----+----------------------------------+
 */
struct virtq_used {
    uint16_t flags;                          /* Bildirim devre dışı bayrağı */
    uint16_t idx;                            /* Cihazın eklediği son elemanın indeksi + 1 */
    struct virtq_used_elem ring[VIRTIO_RING_SIZE]; /* İşlenen istekler */
} __attribute__((packed));

/*
 * VirtIO Blok İsteği Başlığı:
 *
 * Her disk G/Ç işlemi için descriptor[0]'a bu yapı yerleştirilir.
 * Cihaz bu yapıyı okur ve işlemi gerçekleştirir.
 *
 *  +------+----------+--------+
 *  | type | reserved | sector |
 *  | 4 B  | 4 B      | 8 B   |
 *  +------+----------+--------+
 */
struct virtio_blk_req {
    uint32_t type;     /* VIRTIO_BLK_T_IN (okuma) veya VIRTIO_BLK_T_OUT (yazma) */
    uint32_t reserved; /* Gelecek kullanım için sıfır — farklı VirtIO versiyonlarında kullanılabilir */
    uint64_t sector;   /* LBA (Logical Block Address) sektör numarası, 512 bayt/sektör */
} __attribute__((packed));

/* ============================================================================
 * Dışa Açık Fonksiyonlar
 * ============================================================================ */

/*
 * VirtIO blok aygıtını başlatır.
 *
 * @param base_port: PCI BAR0'dan okunan I/O adres taban portu.
 *                   QEMU'da genellikle 0x6000 veya 0x8000'dir.
 */
void virtio_disk_init(uint16_t base_port);

/*
 * VirtIO blok aygıtından tek sektör okur veya yazar.
 *
 * @param sector: LBA sektör numarası (her sektör 512 bayttır)
 * @param buf:    Veri tamponu — en az 512 bayt büyüklüğünde olmalıdır
 * @param write:  0 = okuma (disk → buf), sıfır dışı = yazma (buf → disk)
 *
 * Bu fonksiyon senkron çalışır: işlem tamamlanana kadar (veya zaman aşımına
 * kadar) geri dönmez.
 */
void virtio_disk_rw(uint64_t sector, void* buf, int write);
