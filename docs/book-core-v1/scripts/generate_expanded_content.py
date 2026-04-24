from __future__ import annotations

from pathlib import Path
import re


BOOK_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = BOOK_ROOT.parents[1]
SRC_OUT = BOOK_ROOT / "src"


def read_lines(rel: str) -> list[str]:
    return (REPO_ROOT / rel).read_text(encoding="utf-8", errors="ignore").splitlines()


def find_line(rel: str, symbol: str) -> int | None:
    lines = read_lines(rel)
    for idx, line in enumerate(lines, start=1):
        if symbol in line:
            return idx
    return None


def extract_top_docs(rel: str, max_lines: int = 80) -> list[str]:
    lines = read_lines(rel)
    out: list[str] = []
    for line in lines:
        s = line.strip()
        if s.startswith("//!"):
            out.append(s[3:].strip())
            if len(out) >= max_lines:
                break
        elif s == "":
            if out:
                out.append("")
        elif out:
            break
        else:
            continue
    return [x for x in out if x.strip() != ""]


TOPICS = [
    {
        "id": "boot",
        "title": "Boot, platform init ve erken dogruluk",
        "file": "src/main.rs",
        "symbols": [
            "init_platform_iommu",
            "parse_swap_cmdline",
            "serial_init",
            "panic_handler",
        ],
        "equation": "L_boot = L_firmware + L_loader + L_kernel_early + L_subsystem_init",
        "risk": "Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.",
        "mitigation": "Fail-closed init, capability bazli acilis, adim bazli loglama.",
    },
    {
        "id": "frame",
        "title": "Bootstrap frame allocator ve fiziksel aralik korumasi",
        "file": "src/memory/frame_allocator.rs",
        "symbols": [
            "allocate_frame_internal",
            "allocate_contiguous",
            "overlaps_kernel",
            "kernel_phys_range",
        ],
        "equation": "F_free = F_total - F_used - F_reserved",
        "risk": "Kernel image araligi korunmazsa self-corruption olusur.",
        "mitigation": "Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.",
    },
    {
        "id": "sched-core",
        "title": "SMP scheduler karar modeli",
        "file": "src/task/scheduler.rs",
        "symbols": [
            "choose_spawn_cpu",
            "enqueue_boxed_task",
            "publish_worker_load",
            "update_cpu_count",
        ],
        "equation": "Skew = max(queue_len_i) - min(queue_len_i)",
        "risk": "Load skew artarsa tail latency patlar.",
        "mitigation": "Work stealing + queue telemetrisi + affinity filtreleri.",
    },
    {
        "id": "rt",
        "title": "RT scheduler: FIFO/RR ve runtime limiti",
        "file": "src/task/rt_scheduler.rs",
        "symbols": [
            "calculate_timeslice",
            "enqueue",
            "tick",
            "set_sched_param",
        ],
        "equation": "slice = s_min + alpha(prio) * (s_max - s_min)",
        "risk": "Yanlis policy secimi starvation ve jitter uretir.",
        "mitigation": "RR dilimi ve RT bandwidth governance.",
    },
    {
        "id": "cfs",
        "title": "CFS: vruntime adalet motoru",
        "file": "src/task/cfs.rs",
        "symbols": [
            "weight_to_vruntime",
            "enqueue",
            "pick_next",
            "check_preempt_wakeup",
        ],
        "equation": "Delta_v = (Delta_t * NICE0) / weight",
        "risk": "Wakup-heavy yukte asiri preemption ve fairness gerilimi.",
        "mitigation": "Wakeup granularity ve min_vruntime clamp.",
    },
    {
        "id": "eevdf",
        "title": "EEVDF: eligible_vtime ve virtual deadline",
        "file": "src/task/eevdf.rs",
        "symbols": [
            "update_runtime",
            "pick_next",
            "should_preempt",
            "stats",
        ],
        "equation": "lag = rq_vtime - vruntime",
        "risk": "Yanlis slice ve lag dengesi wakeup davranisini bozar.",
        "mitigation": "Lag tabanli eligibility + deadline siralama.",
    },
    {
        "id": "deadline",
        "title": "Deadline scheduler: EDF/CBS admission ve replenish",
        "file": "src/task/deadline.rs",
        "symbols": [
            "compute_bandwidth",
            "check_replenishments",
            "consume_runtime",
            "enqueue",
        ],
        "equation": "U = C/T, sum(U_i) <= 1",
        "risk": "Admission ihlali deadline miss patlamasi uretir.",
        "mitigation": "Bandwidth limiti + periodik replenish + throttle.",
    },
    {
        "id": "deque",
        "title": "Chase-Lev deque: lock-free race analizi",
        "file": "src/task/deque.rs",
        "symbols": [
            "push",
            "pop",
            "steal",
            "compare_exchange",
        ],
        "equation": "contention ~= P(last_element_race)",
        "risk": "Ordering bug'i sessiz veri bozulmasi yaratir.",
        "mitigation": "Acquire/Release/SeqCst sinirlarinin explicit kullanimi.",
    },
    {
        "id": "wheel",
        "title": "Hiyerarsik timing wheel",
        "file": "src/task/timer.rs",
        "symbols": [
            "schedule",
            "tick",
            "cascade",
            "WHEEL_SIZE",
        ],
        "equation": "T_manage ~= O(1) amortized",
        "risk": "Cascade atlanirsa wakeup gecikmeleri birikir.",
        "mitigation": "Level wrap noktalarinda zorunlu cascade yolu.",
    },
    {
        "id": "pmm",
        "title": "Zone-aware PMM fallback mimarisi",
        "file": "src/memory/fibonacci_pmm.rs",
        "symbols": [
            "fallback",
            "allocate_from_zone",
            "allocate_contiguous_from_zone",
            "zone_stats",
        ],
        "equation": "Pressure_zone = fallback_count / req_count",
        "risk": "Sik fallback gizli kapasite krizini maskeler.",
        "mitigation": "Zone telemetrisi ve reclaim tetigi.",
    },
    {
        "id": "buddy",
        "title": "Fibonacci buddy split/coalesce",
        "file": "src/memory/fibonacci_buddy.rs",
        "symbols": [
            "split_block",
            "try_coalesce",
            "find_buddy",
            "utilization",
        ],
        "equation": "F(n) = F(n-1) + F(n-2)",
        "risk": "Yanlis buddy hesabinda leak veya overlap olur.",
        "mitigation": "Adres bazli buddy aritmetigi + recursive coalesce.",
    },
    {
        "id": "tlsf",
        "title": "TLSF heap wrapper guvenligi",
        "file": "src/allocator/tlsf.rs",
        "symbols": [
            "insert_free_region_ptr",
            "alloc_from_main_heap",
            "dealloc_to_main_heap",
            "check_integrity",
        ],
        "equation": "T_alloc ~= O(1), T_free ~= O(1)",
        "risk": "Heap metadata bozulmasi gec fark edilir.",
        "mitigation": "Canary, tracker, boundary guard ve erken heap ayrimi.",
    },
    {
        "id": "fault",
        "title": "User page fault, COW ve THP karari",
        "file": "src/memory/mod.rs",
        "symbols": [
            "handle_user_page_fault",
            "handle_cow_fault",
            "try_map_thp_anon",
            "sanitize_user_map_flags",
        ],
        "equation": "Fault_path = decision(prot, write, present, vma_kind)",
        "risk": "Yanlis fault ayrimi permission bypass veya crash uretir.",
        "mitigation": "Fail-closed fault ayrimi ve map flag sanitization.",
    },
    {
        "id": "reclaim",
        "title": "Reclaim daemon, writeback budget ve pressure",
        "file": "src/memory/mod.rs",
        "symbols": [
            "memory_reclaim_daemon",
            "reclaim_pages_global",
            "process_writeback_budget",
            "start_reclaim_daemon",
        ],
        "equation": "rho = lambda_dirty / mu_writeback",
        "risk": "rho > 1 kalirsa writeback kuyrugu patlar.",
        "mitigation": "Budget tabanli writeback ve pressure sinyali.",
    },
    {
        "id": "mglru",
        "title": "MGLRU generation ve victim secimi",
        "file": "src/memory/mglru.rs",
        "symbols": [
            "on_access",
            "age_tick",
            "pick_victim",
            "record_refault",
        ],
        "equation": "victim = argmin(generation, hot_score)",
        "risk": "Yanlis aging policy refault dalgasi uretir.",
        "mitigation": "Generation + access_count + refault promotion.",
    },
    {
        "id": "zswap",
        "title": "ZSwap compression pipeline",
        "file": "src/memory/zswap.rs",
        "symbols": [
            "compress",
            "decompress",
            "ZSWAP_DEFAULT_POOL_PERCENT",
            "Compressor",
        ],
        "equation": "Gain = IO_saved - CPU_compress_cost",
        "risk": "Yanlis algoritma secimi CPU'yu bogar.",
        "mitigation": "Pool limiti, compressor secimi ve fallback yolu.",
    },
    {
        "id": "uring",
        "title": "Lock-free io_uring publication boundaries",
        "file": "src/posix/io_uring_ring.rs",
        "symbols": [
            "push",
            "pop",
            "pop_batch",
            "process_submissions",
        ],
        "equation": "occupancy = tail - head",
        "risk": "Tail erken publish edilirse stale read olur.",
        "mitigation": "smp_wmb/smp_rmb ve Acquire/Release disiplini.",
    },
    {
        "id": "tls",
        "title": "TLS 1.3 handshake ve key schedule",
        "file": "src/net/tls.rs",
        "symbols": [
            "derive_handshake_secret",
            "derive_master_secret",
            "hkdf_expand_label",
            "process_server_hello",
        ],
        "equation": "Master = HKDF(HandshakeSecret, 0)",
        "risk": "State gecisi veya transcript hatasi guven modeli kirar.",
        "mitigation": "Tipli handshake state ve explicit key schedule adimlari.",
    },
    {
        "id": "quic",
        "title": "QUIC frame parser ve ACK guard",
        "file": "src/net/quic.rs",
        "symbols": [
            "encode_varint",
            "decode_varint",
            "decode",
            "MAX_ACK_RANGES",
        ],
        "equation": "RTT_connect ~= 1 * RTT (1-RTT)",
        "risk": "Parser limitsizligi memory amplification yapar.",
        "mitigation": "ACK range limiti ve frame decode guardlari.",
    },
    {
        "id": "wireguard",
        "title": "WireGuard handshake, nonce ve replay koruma",
        "file": "src/net/wireguard.rs",
        "symbols": [
            "initiate_handshake",
            "encrypt_packet",
            "decrypt_packet",
            "is_allowed_ip",
        ],
        "equation": "nonce_next > nonce_prev",
        "risk": "Nonce tekrarinda replay kabul riski.",
        "mitigation": "Monoton nonce kontrolu ve session state.",
    },
    {
        "id": "hpack",
        "title": "HPACK Huffman decode fail-closed modeli",
        "file": "src/net/http2_huffman.rs",
        "symbols": [
            "decode_huffman",
            "BitIterator",
            "InvalidPadding",
            "EosInString",
        ],
        "equation": "Decode = traverse(bits) + padding_validation",
        "risk": "EOS/padding hatalari parser acigi uretir.",
        "mitigation": "InvalidPadding ve EosInString ile fail-closed cikis.",
    },
]


def write_monograph() -> None:
    out = SRC_OUT / "cilt1-core-monograf-expanded.md"
    lines: list[str] = []
    lines.append("# Cilt 1 Genisletilmis Monograf")
    lines.append("")
    lines.append(
        "Bu monograf, Cilt 1'in derin katmanidir. Her baslikta kod, algoritma, worst-case ve olcum disiplini birlikte verilir."
    )
    lines.append("")

    for idx, t in enumerate(TOPICS, start=1):
        lines.append(f"## M{idx:02d} - {t['title']}")
        lines.append("")
        lines.append("### Kod baglami")
        lines.append("")
        lines.append(f"- Ana dosya: `{t['file']}`")
        for sym in t["symbols"]:
            ln = find_line(t["file"], sym)
            if ln:
                lines.append(f"- Sembol: `{sym}` -> `{t['file']}:{ln}`")
            else:
                lines.append(f"- Sembol: `{sym}` -> `{t['file']}` icinde dogrulanamadi")
        lines.append("")

        lines.append("### Cekirdek fikir")
        lines.append("")
        lines.append(
            f"Bu alt sistemde ana karar, `{t['file']}` icindeki ownership ve state publication sinirlarini net tutmaktir."
        )
        lines.append(
            "Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir."
        )
        lines.append(
            "Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar."
        )
        lines.append("")

        lines.append("### Matematik modeli")
        lines.append("")
        lines.append(f"- Model: `{t['equation']}`")
        lines.append(
            "- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz."
        )
        lines.append("")

        lines.append("### Worst-case-first")
        lines.append("")
        lines.append(f"- En kotu durum: {t['risk']}")
        lines.append(
            "- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi."
        )
        lines.append(
            "- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi."
        )
        lines.append("")

        lines.append("### Algoritma otopsisi")
        lines.append("")
        lines.append(
            "1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar."
        )
        lines.append(
            "2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir."
        )
        lines.append(f"3. echOS mitigasyonu: {t['mitigation']}")
        lines.append("")

        lines.append("### Kod okuma gorevleri")
        lines.append("")
        lines.append("1. Dosyadaki tum atomik veya state degiskenlerini listele.")
        lines.append("2. Hangi fonksiyonun ownership devrettigini isaretle.")
        lines.append("3. Hata donen tum yollari tabloya dok.")
        lines.append("4. Bir invarianti sec ve nasil bozulabilecegini yaz.")
        lines.append("5. O invarianti koruyan satiri dosyada bulup not al.")
        lines.append("")

        lines.append("### Olcum ve benchmark paketi")
        lines.append("")
        lines.append("- Metrik 1: p50/p95/p99 latency")
        lines.append("- Metrik 2: throughput veya servis hizi")
        lines.append("- Metrik 3: hata/geri alma sayaci")
        lines.append("- Metrik 4: queue depth veya pressure sinyali")
        lines.append("- Metrik 5: regressions arasi fark tablosu")
        lines.append("")

        lines.append("### Vaka analizi")
        lines.append("")
        lines.append(
            "Vaka A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir."
        )
        lines.append(
            "Vaka B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali."
        )
        lines.append(
            "Vaka C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir."
        )
        lines.append("")

        lines.append("### Hedeflenen ogrenme ciktilari")
        lines.append("")
        lines.append("- Bu alt sistemin karar agacini ezbersiz aciklayabilmek")
        lines.append("- Bir bug raporunu dogru katmana indirebilmek")
        lines.append("- Performans iddiasini metrikle savunabilmek")
        lines.append("- Mitigasyon secimini nedenleriyle yazabilmek")
        lines.append("")

        docs = extract_top_docs(t["file"], max_lines=24)
        if docs:
            lines.append("### Dosya basi notlarindan alinti")
            lines.append("")
            for d in docs:
                lines.append(f"> {d}")
            lines.append("")

        lines.append("### Mini sorular")
        lines.append("")
        for q in range(1, 11):
            lines.append(
                f"{q}. `{t['title']}` icin {q}. kritik kontrol noktasi sence hangi fonksiyonda ve neden?"
            )
        lines.append("")
        lines.append("---")
        lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")


def write_1000_questions() -> None:
    out = SRC_OUT / "cilt1-soru-bankasi-1000.md"
    lines: list[str] = []
    lines.append("# Cilt 1 - 1000 Soruluk Buyuk Soru Bankasi")
    lines.append("")
    lines.append(
        "Bu bankada her topic icin 50 soru uretilir. 20 topic x 50 soru = 1000 soru."
    )
    lines.append("")

    q_templates = [
        "`{title}` baglaminda `{sym}` fonksiyonunun ownership sinirini acikla.",
        "`{title}` icin en kotu gecikme patikasi nasil olusur?",
        "`{title}` alt sisteminde fail-closed davranis hangi satirda baslar?",
        "`{file}` dosyasinda guard kaldirilirsa ilk hangi test kirilmali?",
        "`{title}` icin bir invariant yaz ve ihlal semptomunu belirt.",
        "`{title}` modelinde telemetry olmadan hangi karar yanlis kalir?",
        "`{title}` icin p99 odakli tuning planini 3 adimda yaz.",
        "`{title}` alt sisteminde publication boundary neden kritiktir?",
        "`{title}` ile bagli bir admission limiti oner ve gerekcesini yaz.",
        "`{title}` kodunda hata donuslerinin fail-open olmasi niye riskli?",
    ]

    global_idx = 1
    for t_idx, t in enumerate(TOPICS, start=1):
        lines.append(f"## Topic {t_idx:02d} - {t['title']}")
        lines.append("")
        for local in range(1, 51):
            sym = t["symbols"][(local - 1) % len(t["symbols"])]
            tpl = q_templates[(local - 1) % len(q_templates)]
            q = tpl.format(title=t["title"], sym=sym, file=t["file"])
            lines.append(f"{global_idx}. {q}")
            global_idx += 1
        lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")


def collect_public_api(rel: str) -> list[tuple[str, str, int]]:
    lines = read_lines(rel)
    out: list[tuple[str, str, int]] = []

    patterns = [
        ("struct", re.compile(r"^\s*pub\s+struct\s+([A-Za-z0-9_]+)")),
        ("enum", re.compile(r"^\s*pub\s+enum\s+([A-Za-z0-9_]+)")),
        ("fn", re.compile(r"^\s*pub\s+fn\s+([A-Za-z0-9_]+)")),
        ("const", re.compile(r"^\s*pub\s+const\s+([A-Za-z0-9_]+)")),
        ("type", re.compile(r"^\s*pub\s+type\s+([A-Za-z0-9_]+)")),
    ]

    for idx, line in enumerate(lines, start=1):
        for kind, pat in patterns:
            m = pat.search(line)
            if m:
                out.append((kind, m.group(1), idx))
                break
    return out


def write_api_catalog() -> None:
    out = SRC_OUT / "cilt1-core-api-katalogu.md"
    lines: list[str] = []
    lines.append("# Cilt 1 Core API Katalogu")
    lines.append("")
    lines.append(
        "Bu katalog, core ciltte kullandigimiz dosyalardaki public API yuzeyini tek tabloda toplar."
    )
    lines.append("")

    total = 0
    for idx, t in enumerate(TOPICS, start=1):
        api = collect_public_api(t["file"])
        total += len(api)
        lines.append(f"## A{idx:02d} - {t['title']}")
        lines.append("")
        lines.append(f"- Dosya: `{t['file']}`")
        lines.append(f"- Public sembol sayisi: {len(api)}")
        lines.append("")
        lines.append("| Tip | Sembol | Konum |")
        lines.append("|---|---|---|")
        for kind, name, line_no in api:
            lines.append(f"| {kind} | `{name}` | `{t['file']}:{line_no}` |")
        lines.append("")

    lines.append("---")
    lines.append("")
    lines.append(f"Toplam listelenen public sembol sayisi: **{total}**")
    lines.append("")
    out.write_text("\n".join(lines), encoding="utf-8")


def write_casebook() -> None:
    out = SRC_OUT / "cilt1-vaka-ve-cozumler.md"
    lines: list[str] = []
    lines.append("# Cilt 1 Vaka ve Cozumler")
    lines.append("")
    lines.append(
        "Bu bolumde her topic icin operasyonel vaka setleri verilir. Amac, kodu gercek semptomla baglamak."
    )
    lines.append("")

    case_no = 1
    for idx, t in enumerate(TOPICS, start=1):
        lines.append(f"## Vaka Kumesi {idx:02d} - {t['title']}")
        lines.append("")
        for local in range(1, 16):
            sym = t["symbols"][(local - 1) % len(t["symbols"])]
            lines.append(f"### Vaka {case_no:03d} - {t['title']} / senaryo {local}")
            lines.append("")
            lines.append(
                f"- Senaryo: `{t['file']}` icinde `{sym}` etrafinda yuksek yuk altinda anlik performans dususu goruluyor."
            )
            lines.append(
                "- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var."
            )
            lines.append(f"- Muhtemel kok neden: {t['risk']}")
            lines.append(
                "- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar."
            )
            lines.append(
                "- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok."
            )
            lines.append(
                "- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir."
            )
            lines.append(f"- Cozum yaklasimi: {t['mitigation']}")
            lines.append(
                "- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla."
            )
            lines.append(
                "- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz."
            )
            lines.append("")
            case_no += 1
        lines.append("---")
        lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")


def write_curriculum_pack() -> None:
    out = SRC_OUT / "cilt1-ders-plani-ve-haftalik-uygulama.md"
    lines: list[str] = []
    lines.append("# Cilt 1 Ders Plani ve Haftalik Uygulama Paketi")
    lines.append("")
    lines.append(
        "Bu paket, 14 haftalik dersin her hafta icin sinif ici ve sinif disi akisini ayrintilandirir."
    )
    lines.append("")

    weekly_topics = [
        "Mimari giris",
        "Boot ve init",
        "Scheduler temel",
        "RT scheduler",
        "CFS",
        "EEVDF/Deadline",
        "Deque/Timing wheel",
        "Zone PMM",
        "Buddy/TLSF",
        "Fault/COW/THP",
        "MGLRU/Reclaim/ZSwap",
        "io_uring",
        "TLS/HPACK",
        "QUIC/WireGuard + final",
    ]

    for week, name in enumerate(weekly_topics, start=1):
        lines.append(f"## Hafta {week:02d} - {name}")
        lines.append("")
        lines.append("### Sinif ici 90 dk plan")
        lines.append("")
        lines.append("- 0-15 dk: onceki haftanin kritik ozet tekrar")
        lines.append("- 15-45 dk: yeni kavramin sezgisel anlatimi")
        lines.append("- 45-70 dk: kod yuruyusu ve satir referansi")
        lines.append("- 70-90 dk: mini lab kickoff")
        lines.append("")
        lines.append("### Sinif disi 180 dk plan")
        lines.append("")
        lines.append("- 60 dk: kaynak dosya inceleme")
        lines.append("- 60 dk: lab gorevinin raporu")
        lines.append("- 60 dk: soru bankasi cozumleri")
        lines.append("")
        lines.append("### Haftalik odev paketi")
        lines.append("")
        for i in range(1, 11):
            lines.append(
                f"{i}. Bu hafta konusu `{name}` icin {i}. teknik not: bir invariant yaz ve ihlal semptomunu belirt."
            )
        lines.append("")
        lines.append("### Degerlendirme")
        lines.append("")
        lines.append("- Teknik dogruluk: %40")
        lines.append("- Analiz kalitesi: %35")
        lines.append("- Raporlama disiplini: %25")
        lines.append("")
        lines.append("---")
        lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")


if __name__ == "__main__":
    write_monograph()
    write_1000_questions()
    write_api_catalog()
    write_casebook()
    write_curriculum_pack()
    print(
        "Generated: cilt1-core-monograf-expanded.md, cilt1-soru-bankasi-1000.md, "
        "cilt1-core-api-katalogu.md, cilt1-vaka-ve-cozumler.md, cilt1-ders-plani-ve-haftalik-uygulama.md"
    )
