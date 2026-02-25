#include "virtio.h"
#include <stdint.h>

static inline void outb(uint16_t port, uint8_t value) {
    __asm__ volatile("outb %0, %1" : : "a"(value), "Nd"(port));
}

static inline uint8_t inb(uint16_t port) {
    uint8_t value;
    __asm__ volatile("inb %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

static inline void outw(uint16_t port, uint16_t value) {
    __asm__ volatile("outw %0, %1" : : "a"(value), "Nd"(port));
}

static inline uint16_t inw(uint16_t port) {
    uint16_t value;
    __asm__ volatile("inw %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

static inline void outl(uint16_t port, uint32_t value) {
    __asm__ volatile("outl %0, %1" : : "a"(value), "Nd"(port));
}

static inline uint32_t inl(uint16_t port) {
    uint32_t value;
    __asm__ volatile("inl %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

static inline void mb(void) {
    __asm__ volatile("" ::: "memory");
}

extern uint64_t virt_to_phys_c(void* ptr);

static uint16_t virtio_base;
static struct virtq_desc* desc;
static struct virtq_avail* avail;
static struct virtq_used* used;
static uint16_t used_idx;
static struct virtio_blk_req blk_req;
static uint8_t blk_status;
static uint8_t vq[8192] __attribute__((aligned(4096)));

void virtio_disk_init(uint16_t base_port) {
    virtio_base = base_port;
    outb(virtio_base + VIRTIO_PCI_STATUS, 0);
    outl(virtio_base + VIRTIO_PCI_GUEST_PAGE_SIZE, 4096);
    outb(virtio_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE);
    outb(virtio_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);
    outl(virtio_base + VIRTIO_PCI_GUEST_FEATURES, 0);
    outb(virtio_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK);
    uint8_t status = inb(virtio_base + VIRTIO_PCI_STATUS);
    if ((status & VIRTIO_STATUS_FEATURES_OK) == 0) {
        outb(virtio_base + VIRTIO_PCI_STATUS, status | VIRTIO_STATUS_FAILED);
        return;
    }
    outw(virtio_base + VIRTIO_PCI_QUEUE_SEL, 0);
    uint16_t qnum = inw(virtio_base + VIRTIO_PCI_QUEUE_NUM);
    if (qnum < VIRTIO_RING_SIZE) {
        outb(virtio_base + VIRTIO_PCI_STATUS, status | VIRTIO_STATUS_FAILED);
        return;
    }
    desc = (struct virtq_desc*)(vq);
    avail = (struct virtq_avail*)(vq + sizeof(struct virtq_desc) * VIRTIO_RING_SIZE);
    used = (struct virtq_used*)(vq + 4096);
    used_idx = 0;
    avail->idx = 0;
    avail->flags = 0;
    used->idx = 0;
    used->flags = 0;
    uint64_t paddr = virt_to_phys_c(vq);
    outl(virtio_base + VIRTIO_PCI_QUEUE_PFN, (uint32_t)(paddr >> 12));
    outb(virtio_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);
}

void virtio_disk_rw(uint64_t sector, void* buf, int write) {
    blk_req.type = write ? VIRTIO_BLK_T_OUT : VIRTIO_BLK_T_IN;
    blk_req.reserved = 0;
    blk_req.sector = sector;
    blk_status = 0xFF;

    desc[0].addr = virt_to_phys_c(&blk_req);
    desc[0].len = sizeof(blk_req);
    desc[0].flags = VIRTQ_DESC_F_NEXT;
    desc[0].next = 1;

    desc[1].addr = virt_to_phys_c(buf);
    desc[1].len = 512;
    desc[1].flags = write ? VIRTQ_DESC_F_NEXT : (VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE);
    desc[1].next = 2;

    desc[2].addr = virt_to_phys_c(&blk_status);
    desc[2].len = 1;
    desc[2].flags = VIRTQ_DESC_F_WRITE;
    desc[2].next = 0;

    uint16_t idx = avail->idx;
    avail->ring[idx % VIRTIO_RING_SIZE] = 0;
    mb();
    avail->idx = idx + 1;
    mb();
    outw(virtio_base + VIRTIO_PCI_QUEUE_NOTIFY, 0);
    uint32_t spins = 0;
    while (*(volatile uint16_t*)&used->idx == used_idx) {
        if (spins++ > 50000000) {
            blk_status = 0xFE;
            return;
        }
        __asm__ volatile("pause" ::: "memory");
    }
    mb();
    used_idx = used->idx;
}
