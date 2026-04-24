/*
 * echOS VirtIO Blok Cihazı Sürücüsü
 *
 * VirtIO, sanal makineler (VM) ile hypervisor arasındaki G/Ç (I/O) iletişimi
 * için tasarlanmış standart bir arayüzdür. Gerçek donanım emülasyonundan çok
 * daha hızlıdır çünkü gereksiz donanım katmanlarını atlar.
 *
 * Bu sürücü PCI üzerinden Legacy VirtIO (v0) protokolünü kullanarak
 * sanal disklere blok (sektör) okuma/yazma işlemi yapar.
 *
 * VirtIO Halka (Virtqueue) Mimarisi:
 *
 *  +------------------+       +------------------+       +------------------+
 *  |  Descriptor Table|       |  Available Ring  |       |   Used Ring      |
 *  | desc[0]: istek   | ----> | idx: sürücü ekler| ----> | idx: cihaz işler |
 *  | desc[1]: veri    |       | ring[]: desc idx |       | ring[]: kullanıldı|
 *  | desc[2]: durum   |       +------------------+       +------------------+
 *  +------------------+
 *
 *  Sürücü --> Available Ring'e ekler --> Cihazı bildirir (QUEUE_NOTIFY)
 *  Cihaz  --> isteği işler --> Used Ring'e ekler --> (isteğe bağlı) kesinti
 *
 * Zaman Karmaşıklığı:
 *  - virtio_disk_init(): O(1) — tek seferlik başlatma
 *  - virtio_disk_rw():   O(1) — meşgul bekleme (polling) ile tek sektör I/O
 */

#include "virtio.h"
#include <stdint.h>

#define VIRTIO_INVALID_PHYS_ADDR UINT64_MAX
#define VIRTIO_BLK_SECTOR_SIZE 512u

/*
 * x86 port G/Ç yardımcı fonksiyonları
 *
 * x86 mimarisinde I/O cihazlarına erişmek için özel talimatlar (IN/OUT) kullanılır.
 * Bu talimatlar bellek eşlemeli G/Ç'dan (MMIO) farklıdır; 64 KiB'lik ayrı bir
 * I/O adres uzayına erişirler.
 *
 * Satır içi (inline) tanımlanmaları: compiler'ın bunları doğrudan çağrı yerine
 * koda gömmesini sağlar; fonksiyon çağrısı ek yükü (overhead) ortadan kalkar.
 */

/* 8-bit (1 bayt) I/O portuna yazar. */
static inline void outb(uint16_t port, uint8_t value) {
    __asm__ volatile("outb %0, %1" : : "a"(value), "Nd"(port));
}

/* 8-bit (1 bayt) I/O portundan okur. */
static inline uint8_t inb(uint16_t port) {
    uint8_t value;
    __asm__ volatile("inb %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

/* 16-bit (2 bayt) I/O portuna yazar. */
static inline void outw(uint16_t port, uint16_t value) {
    __asm__ volatile("outw %0, %1" : : "a"(value), "Nd"(port));
}

/* 16-bit (2 bayt) I/O portundan okur. */
static inline uint16_t inw(uint16_t port) {
    uint16_t value;
    __asm__ volatile("inw %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

/* 32-bit (4 bayt) I/O portuna yazar. */
static inline void outl(uint16_t port, uint32_t value) {
    __asm__ volatile("outl %0, %1" : : "a"(value), "Nd"(port));
}

/* 32-bit (4 bayt) I/O portundan okur. */
static inline uint32_t inl(uint16_t port) {
    uint32_t value;
    __asm__ volatile("inl %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

/*
 * Bellek bariyeri (memory barrier / fence).
 *
 * Derleyicinin bellek erişimlerini yeniden sıralamasını (reorder) engeller.
 * Donanım cihazlarıyla iletişimde kritiktir: descriptor yazmadan önce
 * available ring güncellemesi yapılamaz.
 *
 * Not: Bu sadece derleyici bariyeridir. Gerçek donanımda gerekirse
 * `mfence` veya `sfence` gibi CPU talimatları da kullanılmalıdır.
 */
static inline void mb(void) {
    __asm__ volatile("" ::: "memory");
}

/* Rust tarafınca sağlanan sanal→fiziksel adres çeviri fonksiyonu.
 * Kernel sayfa tablosu üzerinden verilen sanal adresi fiziksel adrese dönüştürür.
 * VirtIO, cihaza fiziksel adresler geçirmeyi gerektirir. */
extern uint64_t virt_to_phys_c(void* ptr);

/*
 * Sürücü durumu (module-level statik değişkenler)
 *
 * Bu değişkenler tek bir VirtIO disk örneğinin durumunu tutar.
 * Çoklu disk desteği için yapı dizisine dönüştürülmeli.
 */
static uint16_t virtio_base;           /* VirtIO cihazının PCI I/O taban adresi */
static struct virtq_desc* desc;        /* Tanımlayıcı tablosu işaretçisi */
static struct virtq_avail* avail;      /* Mevcut halka işaretçisi (sürücü → cihaz) */
static struct virtq_used* used;        /* Kullanılmış halka işaretçisi (cihaz → sürücü) */
static uint16_t used_idx;             /* Sürücünün gördüğü son used ring indeksi */
static struct virtio_blk_req blk_req;  /* Blok isteği başlığı (globalde tutulur, tek istek) */
static uint8_t blk_status;            /* Cihazdan dönen durum kodu (0=başarı, 1=hata, 2=desteklenmiyor) */

static int virtio_desc_chain_valid(uint16_t head) {
    uint16_t seen_mask = 0;
    uint16_t current = head;
    for (uint16_t depth = 0; depth < VIRTIO_RING_SIZE; depth++) {
        if (current >= VIRTIO_RING_SIZE) {
            return 0;
        }
        if ((seen_mask & (uint16_t)(1u << current)) != 0) {
            return 0;
        }
        seen_mask |= (uint16_t)(1u << current);
        if (desc[current].len == 0) {
            return 0;
        }
        if ((desc[current].flags & VIRTQ_DESC_F_INDIRECT) != 0) {
            return 0;
        }
        if ((desc[current].flags & VIRTQ_DESC_F_NEXT) == 0) {
            return 1;
        }
        current = desc[current].next;
    }
    return 0;
}

static int virtio_used_idx_delta_valid(uint16_t previous, uint16_t current) {
    uint16_t delta = (uint16_t)(current - previous);
    return delta <= VIRTIO_RING_SIZE;
}

/*
 * Virtqueue belleği: 8 KiB, 4096-bayt hizalı.
 *
 * Düzen:
 *  +--------------------------------+  <-- vq[0]
 *  | Descriptor Table               |  VIRTIO_RING_SIZE * sizeof(virtq_desc) bayt
 *  +--------------------------------+
 *  | Available Ring (virtq_avail)   |  küçük boyut
 *  +--------------------------------+
 *  ...padding to 4096-byte boundary...
 *  +--------------------------------+  <-- vq[4096]
 *  | Used Ring (virtq_used)         |
 *  +--------------------------------+
 *
 * Neden 4096-bayt hizalama? VirtIO Legacy, queue PFN'ini (Page Frame Number)
 * yani adresi 4096'ya bölerek kaydeder; bu nedenle adres 4096'nın katı olmalıdır.
 */
static uint8_t vq[8192] __attribute__((aligned(4096)));

/*
 * VirtIO disk cihazını başlatır.
 *
 * VirtIO Legacy (v0) protokolüne göre başlatma adımları:
 *
 *  1. Cihazı sıfırla (STATUS = 0)
 *  2. Misafir sayfa boyutunu bildir (4096)
 *  3. ACKNOWLEDGE bayrağını ayarla (cihazı gördük)
 *  4. DRIVER bayrağını ayarla (sürücü yüklendi)
 *  5. Özellik müzakeresi (features negotiation) — bu sürücü hiç özellik istemez
 *  6. FEATURES_OK bayrağını ayarla, cihazın kabul edip etmediğini doğrula
 *  7. Virtqueue 0'ı kur (descriptor, available, used halkaları)
 *  8. DRIVER_OK bayrağını ayarla (hazır)
 *
 *  Başlatma Akışı (durum bayrağı değişimleri):
 *  STATUS: 0 --> ACK --> ACK|DRIVER --> ACK|DRIVER|FEAT_OK --> ACK|DRIVER|FEAT_OK|DRV_OK
 *
 * @param base_port: PCI yapılandırma alanından okunan VirtIO I/O taban adresi
 */
void virtio_disk_init(uint16_t base_port) {
    virtio_base = base_port;

    /* Adım 1: Cihazı sıfırla — durum register'ı 0 yazarak cihaz yeniden başlatılır */
    outb(virtio_base + VIRTIO_PCI_STATUS, 0);

    /* Adım 2: Misafir (guest) sayfa boyutunu bildir — PFN hesaplamaları için gerekli */
    outl(virtio_base + VIRTIO_PCI_GUEST_PAGE_SIZE, 4096);

    /* Adım 3 & 4: ACKNOWLEDGE + DRIVER — cihazı gördük ve sürücü yüklendi */
    outb(virtio_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE);
    outb(virtio_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);

    /* Adım 5: Özellik müzakeresi — bu sürücü bu queue düzeni için hiçbir ek feature talep etmiyor */
    outl(virtio_base + VIRTIO_PCI_GUEST_FEATURES, 0);

    /* Adım 6: FEATURES_OK bayrağını ayarla ve cihazın kabul ettiğini doğrula */
    outb(virtio_base + VIRTIO_PCI_STATUS,
         VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK);
    uint8_t status = inb(virtio_base + VIRTIO_PCI_STATUS);
    if ((status & VIRTIO_STATUS_FEATURES_OK) == 0) {
        /* Cihaz özellikleri reddetti: FAILED bayrağıyla iptal et */
        outb(virtio_base + VIRTIO_PCI_STATUS, status | VIRTIO_STATUS_FAILED);
        return;
    }

    /* Virtqueue 0'ı seç ve boyutunu oku */
    outw(virtio_base + VIRTIO_PCI_QUEUE_SEL, 0);
    uint16_t qnum = inw(virtio_base + VIRTIO_PCI_QUEUE_NUM);
    if (qnum < VIRTIO_RING_SIZE) {
        /* Cihazın halka boyutu bizim tanımlayıcı sayımızdan küçük: başarısız */
        outb(virtio_base + VIRTIO_PCI_STATUS, status | VIRTIO_STATUS_FAILED);
        return;
    }

    /*
     * Halka bileşenlerini vq tamponu içinde konumlandır:
     *  - desc:  vq başında (descriptor table)
     *  - avail: descriptor table'dan hemen sonra (available ring)
     *  - used:  4096-bayt sınırında (ikinci sayfa — cihaz bu konumu bekler)
     */
    desc  = (struct virtq_desc*)(vq);
    avail = (struct virtq_avail*)(vq + sizeof(struct virtq_desc) * VIRTIO_RING_SIZE);
    used  = (struct virtq_used*)(vq + 4096);

    /* Halka sayaçlarını sıfırla */
    used_idx     = 0;
    avail->idx   = 0;
    avail->flags = 0;
    used->idx    = 0;
    used->flags  = 0;

    /*
     * Cihaza virtqueue fiziksel adresini bildir.
     * PFN (Page Frame Number) = fiziksel adres / 4096
     * Sağa 12 bit kaydırma = bölü 4096
     */
    uint64_t paddr = virt_to_phys_c(vq);
    if (paddr == VIRTIO_INVALID_PHYS_ADDR || (paddr & 0xFFFu) != 0) {
        outb(virtio_base + VIRTIO_PCI_STATUS, status | VIRTIO_STATUS_FAILED);
        return;
    }
    outl(virtio_base + VIRTIO_PCI_QUEUE_PFN, (uint32_t)(paddr >> 12));

    /* Adım 8: DRIVER_OK — sürücü tamamen hazır, cihaz artık istekleri kabul eder */
    outb(virtio_base + VIRTIO_PCI_STATUS,
         VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER |
         VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);
}

/*
 * VirtIO disk okuma/yazma işlemi (tek sektör).
 *
 * Her istek üç tanımlayıcı (descriptor) zincirinden oluşur:
 *
 *  Descriptor Zinciri:
 *  +----------+     +----------+     +----------+
 *  | desc[0]  | --> | desc[1]  | --> | desc[2]  |
 *  | istek hdr|     | veri buf |     | durum byt|
 *  | (sürücü) |     | (R veya W|     | (CIHAZ W)|
 *  +----------+     +----------+     +----------+
 *
 *  desc[0]: virtio_blk_req başlığı — cihaza okunur olarak gönderilir
 *  desc[1]: veri tamponu — okuma için cihaz yazar (F_WRITE), yazma için sürücü verir
 *  desc[2]: tek baytlık durum — cihaz işlem sonunder buraya 0 (başarı) veya hata yazar
 *
 * Meşgul Bekleme (Busy Wait / Polling):
 *  Cihaza QUEUE_NOTIFY yazıldıktan sonra used->idx değişene kadar beklenir.
 *  Bu yol kesinti yerine bounded polling kullanır; used->idx değişene kadar
 *  döner ve poll üst limiti ile cihaz yanıt vermediğinde fail-closed kalır.
 *
 * @param sector: Okunacak/yazılacak LBA sektör numarası (512 bayt/sektör)
 * @param buf: Veri tamponu (okumada dolacak, yazmada okunacak)
 * @param write: 0 = okuma, sıfır dışı = yazma
 */
void virtio_disk_rw(uint64_t sector, void* buf, int write) {
    if (buf == 0) {
        blk_status = 0xFD;
        return;
    }
    /* İstek başlığını doldur: tür, ayrılmış alan ve sektör numarası */
    blk_req.type     = write ? VIRTIO_BLK_T_OUT : VIRTIO_BLK_T_IN;
    blk_req.reserved = 0;
    blk_req.sector   = sector;
    uint64_t req_phys = virt_to_phys_c(&blk_req);
    uint64_t buf_phys = virt_to_phys_c(buf);
    uint64_t status_phys = virt_to_phys_c(&blk_status);
    if (req_phys == VIRTIO_INVALID_PHYS_ADDR ||
        buf_phys == VIRTIO_INVALID_PHYS_ADDR ||
        status_phys == VIRTIO_INVALID_PHYS_ADDR) {
        blk_status = 0xFD;
        return;
    }
    blk_status       = 0xFF; /* Başlangıç değeri: henüz işlenmedi */

    /*
     * Tanımlayıcı 0: Blok isteği başlığı (sürücü tarafından okunur)
     * F_NEXT bayrağı: zincirin devamı var demektir
     */
    desc[0].addr  = req_phys;
    desc[0].len   = sizeof(blk_req);
    desc[0].flags = VIRTQ_DESC_F_NEXT;
    desc[0].next  = 1;

    /*
     * Tanımlayıcı 1: Veri tamponu
     * Okuma (T_IN):  F_WRITE bayrağı — cihaz bu tampona YAZar
     * Yazma (T_OUT): F_WRITE yok    — cihaz bu tampondan OKUR
     */
    desc[1].addr  = buf_phys;
    desc[1].len   = VIRTIO_BLK_SECTOR_SIZE; /* Standart blok boyutu: 512 bayt */
    desc[1].flags = write ? VIRTQ_DESC_F_NEXT : (VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE);
    desc[1].next  = 2;

    /*
     * Tanımlayıcı 2: Durum baytı (cihaz tarafından yazılır)
     * F_WRITE: cihaz bu tek bayta sonuç kodunu yazar
     * 0 = başarı, 1 = G/Ç hatası, 2 = desteklenmeyen istek
     */
    desc[2].addr  = status_phys;
    desc[2].len   = 1;
    desc[2].flags = VIRTQ_DESC_F_WRITE;
    desc[2].next  = 0;

    if (!virtio_desc_chain_valid(0)) {
        blk_status = 0xFD;
        return;
    }

    /*
     * Descriptor zincirini available ring'e ekle ve cihazı bildir.
     *
     * Adım 1: available ring'e desc[0] indeksini yaz
     * Adım 2: Bellek bariyeri — descriptor yazımının tamamlandığını garantile
     * Adım 3: avail->idx'i artır — cihaz bu artışı görünce işlemeye başlar
     * Adım 4: Bellek bariyeri — idx artışının görünür olmasını garantile
     * Adım 5: QUEUE_NOTIFY register'ına yaz — cihaza "yeni istek var" işareti ver
     */
    uint16_t idx = avail->idx;
    avail->ring[idx % VIRTIO_RING_SIZE] = 0; /* Descriptor zinciri desc[0]'dan başlar */
    mb();                                     /* Descriptor'lar tamamen yazıldı */
    avail->idx = idx + 1;                    /* Cihaza yeni istek olduğunu bildir */
    mb();                                     /* idx güncellemesi görünür olsun */
    outw(virtio_base + VIRTIO_PCI_QUEUE_NOTIFY, 0); /* Virtqueue 0'ı bildir */

    /*
     * Cihazın isteği işlemesini bekle (meşgul bekleme — polling).
     *
     * used->idx cihaz tarafından artırıldığında istek tamamlanmış demektir.
     * volatile ile okunur: derleyici bu okumayı register'da saklayamaz.
     *
     * Güvenlik sınırı: ~50 milyon döngü (~50ms @ 1 GHz).
     * Sınır aşılırsa blk_status = 0xFE (zaman aşımı) olarak işaretlenir.
     *
     * Neden `pause` talimatı?
     *  - Hyper-Threading: fiziksel çekirdeği diğer sanal çekirdeğe verir
     *  - Güç tüketimi: polling döngüsündeki güç tüketimini azaltır
     *  - memory: derleyici bariyeri (cihaz bellek erişimleri atlatılmasın)
     */
    uint32_t spins = 0;
    while (*(volatile uint16_t*)&used->idx == used_idx) {
        if (spins++ > 50000000) {
            blk_status = 0xFE; /* Zaman aşımı hata kodu */
            return;
        }
        __asm__ volatile("pause" ::: "memory");
    }

    /* Bellek bariyeri: used ring okumadan önce tüm yazımlar tamamlandı */
    mb();

    uint16_t completed_idx = *(volatile uint16_t*)&used->idx;
    if (!virtio_used_idx_delta_valid(used_idx, completed_idx)) {
        blk_status = 0xFD;
        return;
    }

    struct virtq_used_elem completed = used->ring[used_idx % VIRTIO_RING_SIZE];
    if (completed.id != 0 || completed.len > (VIRTIO_BLK_SECTOR_SIZE + 1u)) {
        blk_status = 0xFD;
        return;
    }

    /* Sürücünün kendi used_idx'ini güncelle — bir sonraki isteği takip etmek için */
    used_idx = completed_idx;
}
