use std::env;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SECTOR_SIZE: usize = 512;
const BLOCK_SIZE: usize = 4096;
const MIB: u64 = 1024 * 1024;

const ESP_TYPE_GUID_LE: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];
const LINUX_FS_GUID_LE: [u8; 16] = [
    0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4,
];

const BOOT_CONTROL_MAGIC: u32 = 0x4342_4345;
const BOOT_CONTROL_VERSION: u16 = 1;
const BOOT_CONTROL_SIZE: usize = 136;
const BOOT_FLAG_AUTO_LOGIN: u8 = 1 << 0;
const BOOT_FLAG_SUSPEND_RESUME_SMOKE: u8 = 1 << 1;

const F2FS_MAGIC: u32 = 0xF2F52010;
const F2FS_SUPERBLOCK_SECTOR_OFFSET: usize = 2;
const F2FS_SUPERBLOCK_SIZE: usize = 4096;
const SUMMARY_ENTRY_SIZE: usize = 7;
const SUPER_MAGIC_OFFSET: usize = 0;
const SUPER_LOG_SECTORSIZE_OFFSET: usize = 8;
const SUPER_LOG_SECTORS_PER_BLOCK_OFFSET: usize = 12;
const SUPER_LOG_BLOCKSIZE_OFFSET: usize = 16;
const SUPER_LOG_BLOCKS_PER_SEG_OFFSET: usize = 20;
const SUPER_SEGMENT_COUNT_SIT_OFFSET: usize = 56;
const SUPER_SEGMENT_COUNT_NAT_OFFSET: usize = 60;
const SUPER_SEGMENT_COUNT_SSA_OFFSET: usize = 64;
const SUPER_SEGMENT_COUNT_MAIN_OFFSET: usize = 68;
const SUPER_CP_BLKADDR_OFFSET: usize = 76;
const SUPER_SIT_BLKADDR_OFFSET: usize = 80;
const SUPER_NAT_BLKADDR_OFFSET: usize = 84;
const SUPER_SSA_BLKADDR_OFFSET: usize = 88;
const SUPER_MAIN_BLKADDR_OFFSET: usize = 92;
const SUPER_ROOT_INO_OFFSET: usize = 0x60;
const SUPER_CP_PAYLOAD_OFFSET: usize = 0x680;
const CP_CHECKPOINT_VER_OFFSET: usize = 0;
const CP_CKPT_FLAGS_OFFSET: usize = 132;
const CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET: usize = 136;
const CP_SIT_VER_BITMAP_BYTESIZE_OFFSET: usize = 156;
const CP_NAT_VER_BITMAP_BYTESIZE_OFFSET: usize = 160;
const CP_CHECKSUM_OFFSET_OFFSET: usize = 164;
const INODE_I_MODE_OFFSET: usize = 0;
const INODE_I_INLINE_OFFSET: usize = 3;
const INODE_I_SIZE_OFFSET: usize = 16;
const INODE_I_NLINK_OFFSET: usize = 24;
const INODE_I_ADDR_OFFSET: usize = 360;
const S_IFDIR: u16 = 0o040000;
const S_IFREG: u16 = 0o100000;
const DENTRY_BITMAP_SIZE: usize = 27;
const DENTRY_RESERVED_SIZE: usize = 3;
const DENTRY_ENTRY_SIZE: usize = 11;
const DENTRY_SLOT_LEN: usize = 8;
const DENTRY_SLOTS: usize = 214;
const DENTRY_ENTRIES_OFFSET: usize = DENTRY_BITMAP_SIZE + DENTRY_RESERVED_SIZE;
const DENTRY_FILENAME_OFFSET: usize = DENTRY_ENTRIES_OFFSET + (DENTRY_ENTRY_SIZE * DENTRY_SLOTS);
const NAT_ENTRY_SIZE: usize = 9;
const SIT_VBLOCK_MAP_SIZE: usize = 64;
const SIT_ENTRY_SIZE: usize = 2 + SIT_VBLOCK_MAP_SIZE + 8;
const NODE_OFS_SENTINEL: u16 = 0xFFFF;

fn main() {
    if let Err(err) = run() {
        eprintln!("echos_appliance: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = Args::from_env();
    match args.peek().as_deref() {
        Some("slot-image") => {
            args.next();
            run_slot_image(args)
        }
        Some("appliance") => {
            args.next();
            run_appliance(args)
        }
        _ => run_appliance(args),
    }
}

struct Args {
    items: Vec<String>,
    index: usize,
}

impl Args {
    fn from_env() -> Self {
        Self {
            items: env::args().skip(1).collect(),
            index: 0,
        }
    }

    fn peek(&self) -> Option<String> {
        self.items.get(self.index).cloned()
    }

    fn next(&mut self) -> Option<String> {
        let value = self.items.get(self.index).cloned();
        if value.is_some() {
            self.index += 1;
        }
        value
    }

    fn value(&mut self, name: &str) -> Result<String, String> {
        self.next()
            .ok_or_else(|| format!("missing value for {name}"))
    }
}

#[derive(Clone)]
struct FatNode {
    name: String,
    directory: bool,
    data: Vec<u8>,
    children: Vec<FatNode>,
    first_cluster: usize,
}

impl FatNode {
    fn dir(name: &str) -> Self {
        Self {
            name: name.to_string(),
            directory: true,
            data: Vec::new(),
            children: Vec::new(),
            first_cluster: 0,
        }
    }

    fn file(name: &str, data: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            directory: false,
            data,
            children: Vec::new(),
            first_cluster: 0,
        }
    }

    fn size(&self) -> u32 {
        self.data.len() as u32
    }
}

struct Partition {
    name: &'static str,
    first_lba: u64,
    last_lba: u64,
    type_guid_le: [u8; 16],
    unique_guid_le: [u8; 16],
}

struct ApplianceConfig {
    efi: PathBuf,
    bootctrl: Option<PathBuf>,
    output: PathBuf,
    disk_mib: u64,
    system_image_mib: u64,
    active_slot: String,
    pending_slot: String,
    auto_login: bool,
    suspend_resume_smoke: bool,
    update_smoke_request_url: Option<String>,
    pe_smoke_bundle: Option<PathBuf>,
    bundles: Vec<PathBuf>,
    esp_extra_files: Vec<String>,
    esp_fat: String,
}

fn run_appliance(mut args: Args) -> Result<(), String> {
    let mut cfg = ApplianceConfig {
        efi: PathBuf::new(),
        bootctrl: None,
        output: PathBuf::new(),
        disk_mib: 512,
        system_image_mib: 8,
        active_slot: "system_a".to_string(),
        pending_slot: "none".to_string(),
        auto_login: false,
        suspend_resume_smoke: false,
        update_smoke_request_url: None,
        pe_smoke_bundle: None,
        bundles: Vec::new(),
        esp_extra_files: Vec::new(),
        esp_fat: "fat32".to_string(),
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--efi" => cfg.efi = PathBuf::from(args.value("--efi")?),
            "--bootctrl" => cfg.bootctrl = Some(PathBuf::from(args.value("--bootctrl")?)),
            "--output" => cfg.output = PathBuf::from(args.value("--output")?),
            "--disk-mib" => cfg.disk_mib = parse_u64(&args.value("--disk-mib")?, "--disk-mib")?,
            "--system-image-mib" => {
                cfg.system_image_mib =
                    parse_u64(&args.value("--system-image-mib")?, "--system-image-mib")?
            }
            "--active-slot" => cfg.active_slot = args.value("--active-slot")?,
            "--pending-slot" => cfg.pending_slot = args.value("--pending-slot")?,
            "--auto-login" => cfg.auto_login = true,
            "--suspend-resume-smoke" => cfg.suspend_resume_smoke = true,
            "--update-smoke-request-url" => {
                cfg.update_smoke_request_url = Some(args.value("--update-smoke-request-url")?)
            }
            "--pe-smoke-bundle" => {
                cfg.pe_smoke_bundle = Some(PathBuf::from(args.value("--pe-smoke-bundle")?))
            }
            "--bundle" => cfg.bundles.push(PathBuf::from(args.value("--bundle")?)),
            "--esp-extra-file" => cfg.esp_extra_files.push(args.value("--esp-extra-file")?),
            "--esp-fat" => cfg.esp_fat = args.value("--esp-fat")?,
            _ => return Err(format!("unknown appliance argument: {arg}")),
        }
    }

    if cfg.efi.as_os_str().is_empty() {
        return Err("--efi is required".to_string());
    }
    if cfg.output.as_os_str().is_empty() {
        return Err("--output is required".to_string());
    }
    if cfg.esp_fat != "fat16" && cfg.esp_fat != "fat32" {
        return Err("--esp-fat must be fat16 or fat32".to_string());
    }

    build_appliance(&cfg)
}

fn run_slot_image(mut args: Args) -> Result<(), String> {
    let mut output = PathBuf::new();
    let mut image_mib = 8_u64;
    let mut request_url = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => output = PathBuf::from(args.value("--output")?),
            "--image-mib" => image_mib = parse_u64(&args.value("--image-mib")?, "--image-mib")?,
            "--request-url" => request_url = Some(args.value("--request-url")?),
            _ => return Err(format!("unknown slot-image argument: {arg}")),
        }
    }
    if output.as_os_str().is_empty() {
        return Err("--output is required".to_string());
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create output dir: {err}"))?;
    }
    let image = build_system_slot_image((image_mib * MIB) as usize, request_url.as_deref())?;
    fs::write(&output, image).map_err(|err| format!("write slot image: {err}"))?;
    println!("{}", output.display());
    Ok(())
}

fn build_appliance(cfg: &ApplianceConfig) -> Result<(), String> {
    let efi_bytes =
        fs::read(&cfg.efi).map_err(|err| format!("read EFI {}: {err}", cfg.efi.display()))?;
    let bootctrl_bytes = match &cfg.bootctrl {
        Some(path) => {
            fs::read(path).map_err(|err| format!("read bootctrl {}: {err}", path.display()))?
        }
        None => build_boot_control(
            &cfg.active_slot,
            &cfg.pending_slot,
            cfg.auto_login,
            cfg.suspend_resume_smoke,
        )?,
    };
    let pe_smoke_bundle = match &cfg.pe_smoke_bundle {
        Some(path) => Some(fs::read(path).map_err(|err| format!("read PE smoke bundle: {err}"))?),
        None => None,
    };
    let curated_bundles = cfg
        .bundles
        .iter()
        .map(|path| fs::read(path).map_err(|err| format!("read bundle {}: {err}", path.display())))
        .collect::<Result<Vec<_>, _>>()?;
    let (esp_extra_files, esp_extra_manifest) = read_esp_extra_files(&cfg.esp_extra_files)?;

    let seed_loop_bytes = build_seed_loop_image(&curated_bundles);
    let esp_required_bytes = efi_bytes.len() as u64
        + bootctrl_bytes.len() as u64
        + curated_bundles.iter().map(|b| b.len() as u64).sum::<u64>()
        + pe_smoke_bundle.as_ref().map_or(0, |b| b.len() as u64)
        + esp_extra_files
            .iter()
            .map(|(_, b)| b.len() as u64)
            .sum::<u64>()
        + 16 * MIB;
    let seed_required_bytes = seed_loop_bytes.len() as u64
        + curated_bundles.iter().map(|b| b.len() as u64).sum::<u64>()
        + 16 * MIB;
    let minimum_disk_bytes = max64(64 * MIB, align_up_u64(esp_required_bytes, MIB))
        + max64(64 * MIB, align_up_u64(seed_required_bytes, MIB))
        + 96 * MIB
        + 96 * MIB
        + 192 * MIB
        + 128 * MIB;
    let disk_bytes = max64(cfg.disk_mib * MIB, align_up_u64(minimum_disk_bytes, MIB));
    let disk_sectors = disk_bytes / SECTOR_SIZE as u64;
    let mut layout = create_layout(disk_bytes, esp_required_bytes)?;
    resize_seed_and_recovery(&mut layout, disk_sectors, seed_required_bytes)?;

    if let Some(parent) = cfg.output.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create output dir: {err}"))?;
    }
    let mut image = File::create(&cfg.output).map_err(|err| format!("create image: {err}"))?;
    image
        .set_len(disk_bytes)
        .map_err(|err| format!("resize image: {err}"))?;

    let esp = part(&layout, "esp")?;
    let esp_bytes = ((esp.last_lba - esp.first_lba + 1) * SECTOR_SIZE as u64) as usize;
    let esp_image = build_esp_image(
        &cfg.esp_fat,
        esp_bytes,
        esp.first_lba as u32,
        efi_bytes,
        bootctrl_bytes.clone(),
        pe_smoke_bundle,
        curated_bundles.clone(),
        esp_extra_files,
    )?;
    let seed = part(&layout, "seed")?;
    let seed_bytes = ((seed.last_lba - seed.first_lba + 1) * SECTOR_SIZE as u64) as usize;
    let seed_image = build_seed_fat16_image(seed_bytes, seed.first_lba as u32, &curated_bundles)?;
    let system_image = build_system_slot_image((cfg.system_image_mib * MIB) as usize, None)?;
    let active_system_image = build_system_slot_image(
        (cfg.system_image_mib * MIB) as usize,
        cfg.update_smoke_request_url.as_deref(),
    )?;
    let (protective_mbr, primary_gpt, backup_gpt) = build_partition_table(disk_sectors, &layout);

    write_at(&mut image, 0, &protective_mbr)?;
    write_at(&mut image, SECTOR_SIZE as u64, &primary_gpt)?;
    write_at(&mut image, esp.first_lba * SECTOR_SIZE as u64, &esp_image)?;
    write_at(&mut image, seed.first_lba * SECTOR_SIZE as u64, &seed_image)?;
    for p in &layout {
        if !matches!(p.name, "system_a" | "system_b" | "recovery") {
            continue;
        }
        let data = if p.name == cfg.active_slot && cfg.update_smoke_request_url.is_some() {
            &active_system_image
        } else {
            &system_image
        };
        write_at(&mut image, p.first_lba * SECTOR_SIZE as u64, data)?;
    }
    write_at(
        &mut image,
        (disk_sectors - 33) * SECTOR_SIZE as u64,
        &backup_gpt,
    )?;
    image.flush().map_err(|err| format!("flush image: {err}"))?;

    let manifest_path = cfg.output.with_extension("json");
    let bootctrl_path = cfg.output.with_extension("bootctrl.bin");
    fs::write(
        &manifest_path,
        build_manifest(cfg, disk_bytes, &layout, esp_extra_manifest),
    )
    .map_err(|err| format!("write manifest: {err}"))?;
    fs::write(&bootctrl_path, bootctrl_bytes).map_err(|err| format!("write bootctrl: {err}"))?;
    println!("appliance image written: {}", cfg.output.display());
    Ok(())
}

fn build_esp_image(
    filesystem: &str,
    total_bytes: usize,
    hidden_sectors: u32,
    efi_bytes: Vec<u8>,
    bootctrl_bytes: Vec<u8>,
    pe_smoke_bundle: Option<Vec<u8>>,
    curated_bundles: Vec<Vec<u8>>,
    esp_extra_files: Vec<(String, Vec<u8>)>,
) -> Result<Vec<u8>, String> {
    let root = build_esp_tree(
        efi_bytes,
        bootctrl_bytes,
        pe_smoke_bundle,
        curated_bundles,
        esp_extra_files,
    );
    match filesystem {
        "fat16" => build_fat16_image(
            total_bytes,
            hidden_sectors,
            root,
            "ECHOSF16",
            "ECHOS_ESP  ",
            0xEC71A11E,
        ),
        "fat32" => build_fat32_image(total_bytes, hidden_sectors, root),
        _ => Err(format!("unsupported ESP filesystem: {filesystem}")),
    }
}

fn build_esp_tree(
    efi_bytes: Vec<u8>,
    bootctrl_bytes: Vec<u8>,
    pe_smoke_bundle: Option<Vec<u8>>,
    curated_bundles: Vec<Vec<u8>>,
    esp_extra_files: Vec<(String, Vec<u8>)>,
) -> FatNode {
    let mut root = FatNode::dir("ROOT");
    let mut efi = FatNode::dir("EFI");
    let mut boot = FatNode::dir("BOOT");
    boot.children.push(FatNode::file("BOOTX64.EFI", efi_bytes));
    boot.children
        .push(FatNode::file("BOOTCTRL.BIN", bootctrl_bytes));
    if let Some(bundle) = pe_smoke_bundle {
        boot.children.push(FatNode::file("PESMOKE.BHD", bundle));
    }
    for (idx, bundle) in curated_bundles.into_iter().enumerate() {
        boot.children
            .push(FatNode::file(&format!("APP{:04}.BHD", idx + 1), bundle));
    }
    for (guest_name, payload) in esp_extra_files {
        boot.children.push(FatNode::file(&guest_name, payload));
    }
    boot.children.push(FatNode::file(
        "APPLINFO.TXT",
        b"echOS appliance image\nboot target: EFI/BOOT/BOOTX64.EFI\n".to_vec(),
    ));
    efi.children.push(boot);
    root.children.push(efi);
    root
}

fn build_fat16_image(
    total_bytes: usize,
    hidden_sectors: u32,
    mut root: FatNode,
    oem: &str,
    label: &str,
    serial: u32,
) -> Result<Vec<u8>, String> {
    let total_sectors = total_bytes / SECTOR_SIZE;
    let reserved_sectors = 1usize;
    let root_entries = 512usize;
    let root_dir_sectors = (root_entries * 32 + (SECTOR_SIZE - 1)) / SECTOR_SIZE;
    let fat_count = 2usize;
    let mut sectors_per_cluster = 4usize;
    let (fat_sectors, data_sectors, cluster_count) = loop {
        let mut fat_sectors = 1usize;
        loop {
            let data_sectors =
                total_sectors - reserved_sectors - fat_count * fat_sectors - root_dir_sectors;
            let cluster_count = data_sectors / sectors_per_cluster;
            let next_fat_sectors = div_ceil((cluster_count + 2) * 2, SECTOR_SIZE);
            if next_fat_sectors <= fat_sectors {
                break;
            }
            fat_sectors = next_fat_sectors;
        }
        let data_sectors =
            total_sectors - reserved_sectors - fat_count * fat_sectors - root_dir_sectors;
        let cluster_count = data_sectors / sectors_per_cluster;
        if cluster_count <= 0xFFF5 {
            break (fat_sectors, data_sectors, cluster_count);
        }
        sectors_per_cluster *= 2;
        if sectors_per_cluster > 128 {
            return Err("FAT16 geometry exceeds supported cluster size".to_string());
        }
    };
    let cluster_bytes = sectors_per_cluster * SECTOR_SIZE;
    let mut fat_entries = vec![0u16; cluster_count + 2];
    fat_entries[0] = 0xFFF8;
    fat_entries[1] = 0xFFFF;
    let mut next_cluster = 2usize;
    allocate_fat16(
        &mut root,
        cluster_bytes,
        &mut next_cluster,
        &mut fat_entries,
    )?;

    let mut boot_sector = vec![0u8; SECTOR_SIZE];
    boot_sector[0..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    copy_padded(&mut boot_sector[3..11], oem.as_bytes());
    write_u16(&mut boot_sector, 11, SECTOR_SIZE as u16);
    boot_sector[13] = sectors_per_cluster as u8;
    write_u16(&mut boot_sector, 14, reserved_sectors as u16);
    boot_sector[16] = fat_count as u8;
    write_u16(&mut boot_sector, 17, root_entries as u16);
    write_u16(
        &mut boot_sector,
        19,
        if total_sectors >= 0x10000 {
            0
        } else {
            total_sectors as u16
        },
    );
    boot_sector[21] = 0xF8;
    write_u16(&mut boot_sector, 22, fat_sectors as u16);
    write_u16(&mut boot_sector, 24, 32);
    write_u16(&mut boot_sector, 26, 64);
    write_u32(&mut boot_sector, 28, hidden_sectors);
    write_u32(
        &mut boot_sector,
        32,
        if total_sectors >= 0x10000 {
            total_sectors as u32
        } else {
            0
        },
    );
    boot_sector[36] = 0x80;
    boot_sector[38] = 0x29;
    write_u32(&mut boot_sector, 39, serial);
    copy_padded(&mut boot_sector[43..54], label.as_bytes());
    boot_sector[54..62].copy_from_slice(b"FAT16   ");
    boot_sector[510..512].copy_from_slice(&[0x55, 0xAA]);

    let mut fat = vec![0u8; fat_sectors * SECTOR_SIZE];
    for (index, entry) in fat_entries.iter().take(fat.len() / 2).enumerate() {
        write_u16(&mut fat, index * 2, *entry);
    }
    let mut root_dir = vec![0u8; root_dir_sectors * SECTOR_SIZE];
    let mut offset = 0usize;
    for child in &root.children {
        write_dir_entry(
            &mut root_dir[offset..offset + 32],
            &short_name(&child.name),
            child.directory,
            child.first_cluster,
            child.size(),
        );
        offset += 32;
    }
    let mut data = vec![0u8; data_sectors * SECTOR_SIZE];
    write_fat16_children(&root, 0, cluster_bytes, &fat_entries, &mut data)?;

    let mut image = vec![0u8; total_bytes];
    let mut cursor = 0usize;
    image[cursor..cursor + SECTOR_SIZE].copy_from_slice(&boot_sector);
    cursor += SECTOR_SIZE;
    image[cursor..cursor + fat.len()].copy_from_slice(&fat);
    cursor += fat.len();
    image[cursor..cursor + fat.len()].copy_from_slice(&fat);
    cursor += fat.len();
    image[cursor..cursor + root_dir.len()].copy_from_slice(&root_dir);
    cursor += root_dir.len();
    image[cursor..cursor + data.len()].copy_from_slice(&data);
    Ok(image)
}

fn build_fat32_image(
    total_bytes: usize,
    hidden_sectors: u32,
    mut root: FatNode,
) -> Result<Vec<u8>, String> {
    let total_sectors = total_bytes / SECTOR_SIZE;
    let reserved_sectors = 32usize;
    let fat_count = 2usize;
    let root_cluster = 2usize;
    let mut sectors_per_cluster = 1usize;
    let (fat_sectors, data_sectors, cluster_count) = loop {
        let mut fat_sectors = 1usize;
        loop {
            let data_sectors = total_sectors - reserved_sectors - fat_count * fat_sectors;
            let cluster_count = data_sectors / sectors_per_cluster;
            let next_fat_sectors = div_ceil((cluster_count + 2) * 4, SECTOR_SIZE);
            if next_fat_sectors <= fat_sectors {
                break;
            }
            fat_sectors = next_fat_sectors;
        }
        let data_sectors = total_sectors - reserved_sectors - fat_count * fat_sectors;
        let cluster_count = data_sectors / sectors_per_cluster;
        if cluster_count >= 0xFFF5 {
            break (fat_sectors, data_sectors, cluster_count);
        }
        sectors_per_cluster *= 2;
        if sectors_per_cluster > 128 {
            return Err("ESP FAT32 geometry cannot reach FAT32 cluster range".to_string());
        }
    };
    let cluster_bytes = sectors_per_cluster * SECTOR_SIZE;
    let mut fat_entries = vec![0u32; cluster_count + 2];
    fat_entries[0] = 0x0FFFFFF8;
    fat_entries[1] = 0x0FFFFFFF;
    let mut next_cluster = root_cluster;
    allocate_fat32(
        &mut root,
        cluster_bytes,
        &mut next_cluster,
        &mut fat_entries,
    )?;
    if root.first_cluster != root_cluster {
        return Err("FAT32 root cluster allocation invariant failed".to_string());
    }

    let mut boot_sector = vec![0u8; SECTOR_SIZE];
    boot_sector[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    boot_sector[3..11].copy_from_slice(b"ECHOSF32");
    write_u16(&mut boot_sector, 11, SECTOR_SIZE as u16);
    boot_sector[13] = sectors_per_cluster as u8;
    write_u16(&mut boot_sector, 14, reserved_sectors as u16);
    boot_sector[16] = fat_count as u8;
    boot_sector[21] = 0xF8;
    write_u16(&mut boot_sector, 24, 32);
    write_u16(&mut boot_sector, 26, 64);
    write_u32(&mut boot_sector, 28, hidden_sectors);
    write_u32(&mut boot_sector, 32, total_sectors as u32);
    write_u32(&mut boot_sector, 36, fat_sectors as u32);
    write_u32(&mut boot_sector, 44, root_cluster as u32);
    write_u16(&mut boot_sector, 48, 1);
    write_u16(&mut boot_sector, 50, 6);
    boot_sector[64] = 0x80;
    boot_sector[66] = 0x29;
    write_u32(&mut boot_sector, 67, 0xEC71A132);
    boot_sector[71..82].copy_from_slice(b"ECHOS_ESP  ");
    boot_sector[82..90].copy_from_slice(b"FAT32   ");
    boot_sector[510..512].copy_from_slice(&[0x55, 0xAA]);

    let mut fsinfo = vec![0u8; SECTOR_SIZE];
    write_u32(&mut fsinfo, 0, 0x41615252);
    write_u32(&mut fsinfo, 484, 0x61417272);
    write_u32(&mut fsinfo, 488, 0xFFFFFFFF);
    write_u32(&mut fsinfo, 492, 0xFFFFFFFF);
    fsinfo[510..512].copy_from_slice(&[0x55, 0xAA]);

    let mut fat = vec![0u8; fat_sectors * SECTOR_SIZE];
    for (index, entry) in fat_entries.iter().take(fat.len() / 4).enumerate() {
        write_u32(&mut fat, index * 4, *entry);
    }
    let mut data = vec![0u8; data_sectors * SECTOR_SIZE];
    write_fat32_node(&root, root_cluster, cluster_bytes, &fat_entries, &mut data)?;

    let mut image = vec![0u8; total_bytes];
    image[0..SECTOR_SIZE].copy_from_slice(&boot_sector);
    image[SECTOR_SIZE..SECTOR_SIZE * 2].copy_from_slice(&fsinfo);
    image[SECTOR_SIZE * 6..SECTOR_SIZE * 7].copy_from_slice(&boot_sector);
    image[SECTOR_SIZE * 7..SECTOR_SIZE * 8].copy_from_slice(&fsinfo);
    let mut cursor = reserved_sectors * SECTOR_SIZE;
    image[cursor..cursor + fat.len()].copy_from_slice(&fat);
    cursor += fat.len();
    image[cursor..cursor + fat.len()].copy_from_slice(&fat);
    cursor = (reserved_sectors + fat_count * fat_sectors) * SECTOR_SIZE;
    image[cursor..cursor + data.len()].copy_from_slice(&data);
    Ok(image)
}

fn build_seed_loop_image(curated_bundles: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"echSID01");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(curated_bundles.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut payload_offset = 16usize
        + curated_bundles
            .iter()
            .enumerate()
            .map(|(idx, _)| 20 + format!("bundle-{}", idx + 1).len())
            .sum::<usize>();
    let mut payload = Vec::new();
    for (idx, bundle) in curated_bundles.iter().enumerate() {
        let identity = format!("bundle-{}", idx + 1);
        out.extend_from_slice(&(payload_offset as u64).to_le_bytes());
        out.extend_from_slice(&(bundle.len() as u64).to_le_bytes());
        out.extend_from_slice(&(identity.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(identity.as_bytes());
        payload.extend_from_slice(bundle);
        payload_offset += bundle.len();
    }
    out.extend_from_slice(&payload);
    out
}

fn build_seed_fat16_image(
    total_bytes: usize,
    hidden_sectors: u32,
    curated_bundles: &[Vec<u8>],
) -> Result<Vec<u8>, String> {
    let seed_loop = build_seed_loop_image(curated_bundles);
    let mut root = FatNode::dir("ROOT");
    let mut apps = FatNode::dir("APPS");
    for (idx, bundle) in curated_bundles.iter().enumerate() {
        apps.children.push(FatNode::file(
            &format!("APP{:04}.BHD", idx + 1),
            bundle.clone(),
        ));
    }
    root.children.push(apps);
    root.children.push(FatNode::file("APPS.IMG", seed_loop));
    root.children.push(FatNode::file(
        "SEEDINFO.TXT",
        b"echOS seed partition\napps under /APPS\nloop image APPS.IMG\n".to_vec(),
    ));
    build_fat16_image(
        total_bytes,
        hidden_sectors,
        root,
        "ECHOSSED",
        "ECHOS_SEED ",
        0xEC71A55E,
    )
}

fn build_system_slot_image(
    total_bytes: usize,
    request_url: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut builder = F2fsSlotImageBuilder::new(total_bytes)?;
    if let Some(url) = request_url {
        builder.create_file(
            "/config/update/smoke/request.txt",
            format!("{url}\n").as_bytes(),
        )?;
    }
    builder.build()
}

struct DirectoryState {
    inode_nid: u32,
    data_block: u32,
    entries: Vec<(String, u32, bool)>,
}

struct F2fsSlotImageBuilder {
    image: Vec<u8>,
    blocks_per_seg: u32,
    segment_count_sit: u32,
    segment_count_nat: u32,
    segment_count_ssa: u32,
    cp_blkaddr: u32,
    sit_blkaddr: u32,
    nat_blkaddr: u32,
    ssa_blkaddr: u32,
    main_blkaddr: u32,
    segment_count_main: u32,
    root_ino: u32,
    next_nid: u32,
    next_main_block: u32,
    nat_entries: Vec<(u32, u32, u32)>,
    sit_entries: Vec<(u16, Vec<u8>)>,
    summary_blocks: Vec<Vec<u8>>,
    inode_blocks: Vec<(u32, u32)>,
    directories: Vec<DirectoryState>,
    children: Vec<(u32, String, u32, bool)>,
}

impl F2fsSlotImageBuilder {
    fn new(total_bytes: usize) -> Result<Self, String> {
        if total_bytes < 4 * MIB as usize || total_bytes % BLOCK_SIZE != 0 {
            return Err("slot image must be >= 4 MiB and 4 KiB aligned".to_string());
        }
        let total_blocks = (total_bytes / BLOCK_SIZE) as u32;
        let blocks_per_seg = 32;
        let segment_count_sit = 1;
        let segment_count_nat = 1;
        let segment_count_ssa = 2;
        let cp_blkaddr = 1;
        let sit_blkaddr = cp_blkaddr + (2 * blocks_per_seg);
        let nat_blkaddr = sit_blkaddr + (2 * segment_count_sit * blocks_per_seg);
        let ssa_blkaddr = nat_blkaddr + (2 * segment_count_nat * blocks_per_seg);
        let main_blkaddr = ssa_blkaddr + (segment_count_ssa * blocks_per_seg);
        let main_blocks = total_blocks - main_blkaddr;
        let segment_count_main = std::cmp::max(1, main_blocks / blocks_per_seg);
        if segment_count_main < 8 {
            return Err("slot image too small for F2FS main area".to_string());
        }
        if segment_count_ssa * blocks_per_seg < segment_count_main {
            return Err("SSA area too small for main segment summaries".to_string());
        }
        let sit_capacity =
            segment_count_sit * blocks_per_seg * (BLOCK_SIZE as u32 / SIT_ENTRY_SIZE as u32);
        if segment_count_main > sit_capacity {
            return Err("SIT area cannot describe requested main segments".to_string());
        }

        let mut builder = Self {
            image: vec![0u8; total_bytes],
            blocks_per_seg,
            segment_count_sit,
            segment_count_nat,
            segment_count_ssa,
            cp_blkaddr,
            sit_blkaddr,
            nat_blkaddr,
            ssa_blkaddr,
            main_blkaddr,
            segment_count_main,
            root_ino: 1,
            next_nid: 2,
            next_main_block: main_blkaddr,
            nat_entries: Vec::new(),
            sit_entries: vec![(0, vec![0u8; SIT_VBLOCK_MAP_SIZE]); segment_count_main as usize],
            summary_blocks: vec![vec![0u8; BLOCK_SIZE]; segment_count_main as usize],
            inode_blocks: Vec::new(),
            directories: Vec::new(),
            children: Vec::new(),
        };
        let root_inode_block = builder.allocate_main_block(builder.root_ino, NODE_OFS_SENTINEL)?;
        let root_dir_block = builder.allocate_main_block(builder.root_ino, 0)?;
        builder
            .inode_blocks
            .push((builder.root_ino, root_inode_block));
        builder.directories.push(DirectoryState {
            inode_nid: builder.root_ino,
            data_block: root_dir_block,
            entries: vec![
                (".".to_string(), builder.root_ino, true),
                ("..".to_string(), builder.root_ino, true),
            ],
        });
        builder.rewrite_dir_by_nid(builder.root_ino)?;
        builder.write_inode_block(builder.root_ino, true, BLOCK_SIZE as u64, &[root_dir_block])?;
        Ok(builder)
    }

    fn allocate_nid(&mut self) -> u32 {
        let nid = self.next_nid;
        self.next_nid += 1;
        nid
    }

    fn allocate_main_block(&mut self, nid: u32, ofs_in_node: u16) -> Result<u32, String> {
        let max_block = self.main_blkaddr + (self.segment_count_main * self.blocks_per_seg);
        if self.next_main_block >= max_block {
            return Err("slot image exhausted main blocks".to_string());
        }
        let block_addr = self.next_main_block;
        self.next_main_block += 1;
        let relative = block_addr - self.main_blkaddr;
        let segno = relative / self.blocks_per_seg;
        let offset = relative % self.blocks_per_seg;
        let (vblocks, valid_map) = &mut self.sit_entries[segno as usize];
        let byte_index = (offset / 8) as usize;
        let bit = 1u8 << (offset % 8);
        if valid_map[byte_index] & bit != 0 {
            return Err("double allocation in SIT".to_string());
        }
        valid_map[byte_index] |= bit;
        *vblocks += 1;
        self.write_summary(segno as usize, offset as usize, nid, ofs_in_node)?;
        Ok(block_addr)
    }

    fn write_summary(
        &mut self,
        segno: usize,
        offset: usize,
        nid: u32,
        ofs_in_node: u16,
    ) -> Result<(), String> {
        let entry_offset = offset * SUMMARY_ENTRY_SIZE;
        if entry_offset + SUMMARY_ENTRY_SIZE > BLOCK_SIZE {
            return Err("summary entry exceeds block".to_string());
        }
        let block = &mut self.summary_blocks[segno];
        write_u32(block, entry_offset, nid);
        block[entry_offset + 4] = 0;
        write_u16(block, entry_offset + 5, ofs_in_node);
        Ok(())
    }

    fn write_block(&mut self, block_addr: u32, data: &[u8]) -> Result<(), String> {
        if data.len() != BLOCK_SIZE {
            return Err("F2FS blocks must be exactly 4 KiB".to_string());
        }
        let start = block_addr as usize * BLOCK_SIZE;
        self.image[start..start + BLOCK_SIZE].copy_from_slice(data);
        Ok(())
    }

    fn dir_index(&self, nid: u32) -> Result<usize, String> {
        self.directories
            .iter()
            .position(|dir| dir.inode_nid == nid)
            .ok_or_else(|| format!("directory nid {nid} not found"))
    }

    fn rewrite_dir_by_nid(&mut self, nid: u32) -> Result<(), String> {
        let idx = self.dir_index(nid)?;
        let data_block = self.directories[idx].data_block;
        let entries = self.directories[idx].entries.clone();
        let mut block = vec![0u8; BLOCK_SIZE];
        for (slot, (name, ino, is_dir)) in entries.iter().enumerate() {
            if slot >= DENTRY_SLOTS {
                return Err("F2FS dentry block exhausted".to_string());
            }
            let name_bytes = name.as_bytes();
            let slots_needed = div_ceil(name_bytes.len(), DENTRY_SLOT_LEN);
            block[slot / 8] |= 1 << (slot % 8);
            let entry_offset = DENTRY_ENTRIES_OFFSET + (slot * DENTRY_ENTRY_SIZE);
            write_u32(&mut block, entry_offset + 4, *ino);
            write_u16(&mut block, entry_offset + 8, name_bytes.len() as u16);
            block[entry_offset + 10] = if *is_dir { 2 } else { 1 };
            let name_offset = DENTRY_FILENAME_OFFSET + (slot * DENTRY_SLOT_LEN);
            block[name_offset..name_offset + name_bytes.len()].copy_from_slice(name_bytes);
            for extra in 1..slots_needed {
                let extra_slot = slot + extra;
                if extra_slot < DENTRY_SLOTS {
                    block[extra_slot / 8] |= 1 << (extra_slot % 8);
                }
            }
        }
        self.write_block(data_block, &block)
    }

    fn inode_block_addr(&self, nid: u32) -> Result<u32, String> {
        self.inode_blocks
            .iter()
            .find_map(|(entry_nid, block)| (*entry_nid == nid).then_some(*block))
            .ok_or_else(|| format!("inode block for nid {nid} not found"))
    }

    fn write_inode_block(
        &mut self,
        nid: u32,
        is_dir: bool,
        size: u64,
        direct_blocks: &[u32],
    ) -> Result<(), String> {
        let block_addr = self.inode_block_addr(nid)?;
        let mut block = vec![0u8; BLOCK_SIZE];
        write_u16(
            &mut block,
            INODE_I_MODE_OFFSET,
            (if is_dir { S_IFDIR } else { S_IFREG }) | if is_dir { 0o755 } else { 0o644 },
        );
        block[INODE_I_INLINE_OFFSET] = 0;
        write_u64(&mut block, INODE_I_SIZE_OFFSET, size);
        write_u32(&mut block, INODE_I_NLINK_OFFSET, if is_dir { 2 } else { 1 });
        for (index, direct) in direct_blocks.iter().enumerate() {
            write_u32(&mut block, INODE_I_ADDR_OFFSET + index * 4, *direct);
        }
        self.write_block(block_addr, &block)?;
        upsert_nat(&mut self.nat_entries, nid, 1, block_addr);
        Ok(())
    }

    fn child(&self, parent: u32, name: &str) -> Option<(u32, bool)> {
        self.children
            .iter()
            .find_map(|(p, n, nid, is_dir)| (*p == parent && n == name).then_some((*nid, *is_dir)))
    }

    fn create_dir(&mut self, parent_nid: u32, name: &str) -> Result<u32, String> {
        let nid = self.allocate_nid();
        let inode_block = self.allocate_main_block(nid, NODE_OFS_SENTINEL)?;
        let data_block = self.allocate_main_block(nid, 0)?;
        self.inode_blocks.push((nid, inode_block));
        self.directories.push(DirectoryState {
            inode_nid: nid,
            data_block,
            entries: vec![
                (".".to_string(), nid, true),
                ("..".to_string(), parent_nid, true),
            ],
        });
        self.rewrite_dir_by_nid(nid)?;
        self.write_inode_block(nid, true, BLOCK_SIZE as u64, &[data_block])?;
        let parent_idx = self.dir_index(parent_nid)?;
        self.directories[parent_idx]
            .entries
            .push((name.to_string(), nid, true));
        self.children
            .push((parent_nid, name.to_string(), nid, true));
        self.rewrite_dir_by_nid(parent_nid)?;
        Ok(nid)
    }

    fn ensure_dir(&mut self, path: &str) -> Result<u32, String> {
        if path == "/" {
            return Ok(self.root_ino);
        }
        let mut current = self.root_ino;
        for component in path.split('/').filter(|entry| !entry.is_empty()) {
            match self.child(current, component) {
                Some((nid, true)) => current = nid,
                Some((_, false)) => {
                    return Err(format!("path component '{component}' is not a directory"))
                }
                None => current = self.create_dir(current, component)?,
            }
        }
        Ok(current)
    }

    fn create_file(&mut self, path: &str, payload: &[u8]) -> Result<(), String> {
        let normalized = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let parts = normalized
            .split('/')
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return Err("file path cannot be root".to_string());
        }
        let parent_path = if parts.len() > 1 {
            format!("/{}", parts[..parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let name = parts[parts.len() - 1];
        let parent_nid = self.ensure_dir(&parent_path)?;
        if self.child(parent_nid, name).is_some() {
            return Err(format!("duplicate file path '{normalized}'"));
        }
        let nid = self.allocate_nid();
        let inode_block = self.allocate_main_block(nid, NODE_OFS_SENTINEL)?;
        self.inode_blocks.push((nid, inode_block));
        let mut direct_blocks = Vec::new();
        for (index, chunk) in payload.chunks(BLOCK_SIZE).enumerate() {
            let block_addr = self.allocate_main_block(nid, index as u16)?;
            let mut block = vec![0u8; BLOCK_SIZE];
            block[..chunk.len()].copy_from_slice(chunk);
            self.write_block(block_addr, &block)?;
            direct_blocks.push(block_addr);
        }
        self.write_inode_block(nid, false, payload.len() as u64, &direct_blocks)?;
        let parent_idx = self.dir_index(parent_nid)?;
        self.directories[parent_idx]
            .entries
            .push((name.to_string(), nid, false));
        self.children
            .push((parent_nid, name.to_string(), nid, false));
        self.rewrite_dir_by_nid(parent_nid)?;
        Ok(())
    }

    fn write_superblock(&mut self) {
        let mut block = vec![0u8; F2FS_SUPERBLOCK_SIZE];
        write_u32(&mut block, SUPER_MAGIC_OFFSET, F2FS_MAGIC);
        write_u32(&mut block, SUPER_LOG_SECTORSIZE_OFFSET, 9);
        write_u32(&mut block, SUPER_LOG_SECTORS_PER_BLOCK_OFFSET, 3);
        write_u32(&mut block, SUPER_LOG_BLOCKSIZE_OFFSET, 12);
        write_u32(&mut block, SUPER_LOG_BLOCKS_PER_SEG_OFFSET, 5);
        write_u32(
            &mut block,
            SUPER_SEGMENT_COUNT_SIT_OFFSET,
            self.segment_count_sit,
        );
        write_u32(
            &mut block,
            SUPER_SEGMENT_COUNT_NAT_OFFSET,
            self.segment_count_nat,
        );
        write_u32(
            &mut block,
            SUPER_SEGMENT_COUNT_SSA_OFFSET,
            self.segment_count_ssa,
        );
        write_u32(
            &mut block,
            SUPER_SEGMENT_COUNT_MAIN_OFFSET,
            self.segment_count_main,
        );
        write_u32(&mut block, SUPER_CP_BLKADDR_OFFSET, self.cp_blkaddr);
        write_u32(&mut block, SUPER_SIT_BLKADDR_OFFSET, self.sit_blkaddr);
        write_u32(&mut block, SUPER_NAT_BLKADDR_OFFSET, self.nat_blkaddr);
        write_u32(&mut block, SUPER_SSA_BLKADDR_OFFSET, self.ssa_blkaddr);
        write_u32(&mut block, SUPER_MAIN_BLKADDR_OFFSET, self.main_blkaddr);
        write_u32(&mut block, SUPER_ROOT_INO_OFFSET, self.root_ino);
        write_u32(&mut block, SUPER_CP_PAYLOAD_OFFSET, 0);
        let offset = F2FS_SUPERBLOCK_SECTOR_OFFSET * SECTOR_SIZE;
        self.image[offset..offset + block.len()].copy_from_slice(&block);
    }

    fn write_checkpoint_block(&mut self, block_addr: u32, version: u64) -> Result<(), String> {
        let mut block = vec![0u8; BLOCK_SIZE];
        write_u64(&mut block, CP_CHECKPOINT_VER_OFFSET, version);
        write_u32(&mut block, CP_CKPT_FLAGS_OFFSET, 0x00000005);
        write_u32(&mut block, CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET, 1);
        write_u32(&mut block, CP_SIT_VER_BITMAP_BYTESIZE_OFFSET, 0);
        write_u32(&mut block, CP_NAT_VER_BITMAP_BYTESIZE_OFFSET, 0);
        let checksum_offset = BLOCK_SIZE - 4;
        write_u32(
            &mut block,
            CP_CHECKSUM_OFFSET_OFFSET,
            checksum_offset as u32,
        );
        let crc = crc32(&block[..checksum_offset]);
        write_u32(&mut block, checksum_offset, crc);
        self.write_block(block_addr, &block)
    }

    fn write_nat_tables(&mut self) -> Result<(), String> {
        let primary_blocks = self.segment_count_nat * self.blocks_per_seg;
        let nat_region_blocks = primary_blocks * 2;
        for block_index in 0..nat_region_blocks {
            self.write_block(self.nat_blkaddr + block_index, &vec![0u8; BLOCK_SIZE])?;
        }
        let entries_per_block = BLOCK_SIZE / NAT_ENTRY_SIZE;
        let nat_entries = self.nat_entries.clone();
        for (nid, version, block_addr) in nat_entries {
            let table_index = nid as usize / entries_per_block;
            let entry_index = nid as usize % entries_per_block;
            for base in [self.nat_blkaddr, self.nat_blkaddr + primary_blocks] {
                let start =
                    (base as usize + table_index) * BLOCK_SIZE + entry_index * NAT_ENTRY_SIZE;
                self.image[start] = version as u8;
                write_u32(&mut self.image, start + 1, nid);
                write_u32(&mut self.image, start + 5, block_addr);
            }
        }
        Ok(())
    }

    fn write_sit_tables(&mut self) -> Result<(), String> {
        let sit_copy_blocks = self.segment_count_sit * self.blocks_per_seg;
        let total_blocks = sit_copy_blocks * 2;
        for block_index in 0..total_blocks {
            self.write_block(self.sit_blkaddr + block_index, &vec![0u8; BLOCK_SIZE])?;
        }
        let entries_per_block = BLOCK_SIZE / SIT_ENTRY_SIZE;
        for (segno, (vblocks, valid_map)) in self.sit_entries.iter().enumerate() {
            let table_index = segno / entries_per_block;
            let entry_index = segno % entries_per_block;
            for base in [self.sit_blkaddr, self.sit_blkaddr + sit_copy_blocks] {
                let start =
                    (base as usize + table_index) * BLOCK_SIZE + entry_index * SIT_ENTRY_SIZE;
                write_u16(&mut self.image, start, *vblocks);
                self.image[start + 2..start + 2 + SIT_VBLOCK_MAP_SIZE].copy_from_slice(valid_map);
            }
        }
        Ok(())
    }

    fn write_summary_area(&mut self) -> Result<(), String> {
        let total_blocks = self.segment_count_ssa * self.blocks_per_seg;
        for block_index in 0..total_blocks {
            let block_addr = self.ssa_blkaddr + block_index;
            let block = if (block_index as usize) < self.summary_blocks.len() {
                self.summary_blocks[block_index as usize].clone()
            } else {
                vec![0u8; BLOCK_SIZE]
            };
            self.write_block(block_addr, &block)?;
        }
        Ok(())
    }

    fn build(mut self) -> Result<Vec<u8>, String> {
        self.write_superblock();
        self.write_checkpoint_block(self.cp_blkaddr, 1)?;
        self.write_checkpoint_block(self.cp_blkaddr + self.blocks_per_seg, 2)?;
        self.write_nat_tables()?;
        self.write_sit_tables()?;
        self.write_summary_area()?;
        Ok(self.image)
    }
}

fn allocate_fat16(
    node: &mut FatNode,
    cluster_bytes: usize,
    next_cluster: &mut usize,
    fat: &mut [u16],
) -> Result<(), String> {
    if node.name != "ROOT" {
        let needed = if node.directory {
            1
        } else {
            std::cmp::max(1, div_ceil(node.data.len(), cluster_bytes))
        };
        node.first_cluster = *next_cluster;
        for idx in 0..needed {
            let cluster = *next_cluster + idx;
            if cluster >= fat.len() {
                return Err("FAT16 cluster budget exhausted".to_string());
            }
            fat[cluster] = if idx == needed - 1 {
                0xFFFF
            } else {
                (cluster + 1) as u16
            };
        }
        *next_cluster += needed;
    }
    for child in &mut node.children {
        allocate_fat16(child, cluster_bytes, next_cluster, fat)?;
    }
    Ok(())
}

fn allocate_fat32(
    node: &mut FatNode,
    cluster_bytes: usize,
    next_cluster: &mut usize,
    fat: &mut [u32],
) -> Result<(), String> {
    let needed = if node.directory {
        1
    } else {
        std::cmp::max(1, div_ceil(node.data.len(), cluster_bytes))
    };
    node.first_cluster = *next_cluster;
    for idx in 0..needed {
        let cluster = *next_cluster + idx;
        if cluster >= fat.len() {
            return Err("FAT32 cluster budget exhausted".to_string());
        }
        fat[cluster] = if idx == needed - 1 {
            0x0FFFFFFF
        } else {
            (cluster + 1) as u32
        };
    }
    *next_cluster += needed;
    for child in &mut node.children {
        allocate_fat32(child, cluster_bytes, next_cluster, fat)?;
    }
    Ok(())
}

fn write_fat16_children(
    root: &FatNode,
    parent_cluster: usize,
    cluster_bytes: usize,
    fat: &[u16],
    data: &mut [u8],
) -> Result<(), String> {
    for child in &root.children {
        write_fat16_node(child, parent_cluster, cluster_bytes, fat, data)?;
    }
    Ok(())
}

fn write_fat16_node(
    node: &FatNode,
    parent_cluster: usize,
    cluster_bytes: usize,
    fat: &[u16],
    data: &mut [u8],
) -> Result<(), String> {
    if node.directory {
        write_cluster(
            node.first_cluster,
            cluster_bytes,
            data,
            &build_directory(node, parent_cluster, cluster_bytes),
        );
        for child in &node.children {
            write_fat16_node(child, node.first_cluster, cluster_bytes, fat, data)?;
        }
    } else {
        write_file_clusters(
            node.first_cluster,
            &node.data,
            cluster_bytes,
            data,
            |cluster| {
                let next = fat[cluster];
                if next == 0xFFFF {
                    None
                } else {
                    Some(next as usize)
                }
            },
        )?;
    }
    Ok(())
}

fn write_fat32_node(
    node: &FatNode,
    parent_cluster: usize,
    cluster_bytes: usize,
    fat: &[u32],
    data: &mut [u8],
) -> Result<(), String> {
    if node.directory {
        write_cluster(
            node.first_cluster,
            cluster_bytes,
            data,
            &build_directory(node, parent_cluster, cluster_bytes),
        );
        for child in &node.children {
            write_fat32_node(child, node.first_cluster, cluster_bytes, fat, data)?;
        }
    } else {
        write_file_clusters(
            node.first_cluster,
            &node.data,
            cluster_bytes,
            data,
            |cluster| {
                let next = fat[cluster];
                if next >= 0x0FFFFFF8 {
                    None
                } else {
                    Some(next as usize)
                }
            },
        )?;
    }
    Ok(())
}

fn build_directory(node: &FatNode, parent_cluster: usize, cluster_bytes: usize) -> Vec<u8> {
    let mut dir = vec![0u8; cluster_bytes];
    write_dir_entry(&mut dir[0..32], b".          ", true, node.first_cluster, 0);
    write_dir_entry(&mut dir[32..64], b"..         ", true, parent_cluster, 0);
    let mut offset = 64usize;
    for child in &node.children {
        write_dir_entry(
            &mut dir[offset..offset + 32],
            &short_name(&child.name),
            child.directory,
            child.first_cluster,
            child.size(),
        );
        offset += 32;
    }
    dir
}

fn write_dir_entry(
    dst: &mut [u8],
    name_83: &[u8],
    directory: bool,
    first_cluster: usize,
    size: u32,
) {
    dst[0..11].copy_from_slice(&name_83[0..11]);
    dst[11] = if directory { 0x10 } else { 0x20 };
    write_u16(dst, 20, ((first_cluster >> 16) & 0xFFFF) as u16);
    write_u16(dst, 26, (first_cluster & 0xFFFF) as u16);
    write_u32(dst, 28, size);
}

fn write_cluster(cluster: usize, cluster_bytes: usize, data: &mut [u8], payload: &[u8]) {
    let start = (cluster - 2) * cluster_bytes;
    data[start..start + payload.len()].copy_from_slice(payload);
}

fn write_file_clusters<F>(
    first_cluster: usize,
    payload: &[u8],
    cluster_bytes: usize,
    data: &mut [u8],
    mut next: F,
) -> Result<(), String>
where
    F: FnMut(usize) -> Option<usize>,
{
    if payload.is_empty() {
        return Ok(());
    }
    let mut cluster = first_cluster;
    for chunk in payload.chunks(cluster_bytes) {
        write_cluster(cluster, cluster_bytes, data, chunk);
        if let Some(next_cluster) = next(cluster) {
            cluster = next_cluster;
        }
    }
    Ok(())
}

fn build_partition_table(
    total_sectors: u64,
    partitions: &[Partition],
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut primary_entries = vec![0u8; 128 * 128];
    let mut backup_entries = vec![0u8; 128 * 128];
    for (index, part) in partitions.iter().enumerate() {
        let offset = index * 128;
        for entries in [&mut primary_entries, &mut backup_entries] {
            entries[offset..offset + 16].copy_from_slice(&part.type_guid_le);
            entries[offset + 16..offset + 32].copy_from_slice(&part.unique_guid_le);
            write_u64(entries, offset + 32, part.first_lba);
            write_u64(entries, offset + 40, part.last_lba);
            write_u64(entries, offset + 48, 0);
            entries[offset + 56..offset + 128].copy_from_slice(&utf16le_name(part.name));
        }
    }
    let primary_entries_crc = crc32(&primary_entries);
    let backup_entries_crc = crc32(&backup_entries);
    let primary_header = gpt_header(total_sectors, 1, total_sectors - 1, 2, primary_entries_crc);
    let backup_entries_lba = total_sectors - 33;
    let backup_header = gpt_header(
        total_sectors,
        total_sectors - 1,
        1,
        backup_entries_lba,
        backup_entries_crc,
    );
    let mut protective_mbr = vec![0u8; SECTOR_SIZE];
    protective_mbr[446] = 0;
    protective_mbr[447..450].copy_from_slice(&[0x00, 0x02, 0x00]);
    protective_mbr[450] = 0xEE;
    protective_mbr[451..454].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
    write_u32(&mut protective_mbr, 454, 1);
    write_u32(
        &mut protective_mbr,
        458,
        std::cmp::min(total_sectors - 1, u32::MAX as u64) as u32,
    );
    protective_mbr[510..512].copy_from_slice(&[0x55, 0xAA]);
    let mut primary = primary_header;
    primary.extend_from_slice(&primary_entries);
    let mut backup = backup_entries;
    backup.extend_from_slice(&backup_header);
    (protective_mbr, primary, backup)
}

fn gpt_header(
    total_sectors: u64,
    current_lba: u64,
    backup_lba: u64,
    entries_lba: u64,
    entries_crc: u32,
) -> Vec<u8> {
    let mut header = vec![0u8; SECTOR_SIZE];
    header[0..8].copy_from_slice(b"EFI PART");
    write_u32(&mut header, 8, 0x00010000);
    write_u32(&mut header, 12, 92);
    write_u64(&mut header, 24, current_lba);
    write_u64(&mut header, 32, backup_lba);
    write_u64(&mut header, 40, 34);
    write_u64(&mut header, 48, total_sectors - 34);
    header[56..72].copy_from_slice(&make_guid_le("disk", current_lba));
    write_u64(&mut header, 72, entries_lba);
    write_u32(&mut header, 80, 128);
    write_u32(&mut header, 84, 128);
    write_u32(&mut header, 88, entries_crc);
    let crc = crc32(&header[..92]);
    write_u32(&mut header, 16, crc);
    header
}

fn create_layout(disk_bytes: u64, esp_bytes: u64) -> Result<Vec<Partition>, String> {
    let disk_sectors = disk_bytes / SECTOR_SIZE as u64;
    let mut start_lba = 2048u64;
    let mut layout = Vec::new();
    for name in ["esp", "seed", "system_a", "system_b", "data"] {
        let bytes = match name {
            "esp" => max64(64 * MIB, align_up_u64(esp_bytes, MIB)),
            "seed" => 64 * MIB,
            "system_a" | "system_b" => 96 * MIB,
            "data" => 192 * MIB,
            _ => unreachable!(),
        };
        let sectors = bytes / SECTOR_SIZE as u64;
        let first_lba = start_lba;
        let last_lba = first_lba + sectors - 1;
        layout.push(Partition {
            name,
            first_lba,
            last_lba,
            type_guid_le: if name == "esp" {
                ESP_TYPE_GUID_LE
            } else {
                LINUX_FS_GUID_LE
            },
            unique_guid_le: make_guid_le(name, first_lba),
        });
        start_lba = align_up_u64(last_lba + 1, 2048);
    }
    let recovery_last = disk_sectors - 34;
    layout.push(Partition {
        name: "recovery",
        first_lba: start_lba,
        last_lba: recovery_last,
        type_guid_le: LINUX_FS_GUID_LE,
        unique_guid_le: make_guid_le("recovery", start_lba),
    });
    if layout.last().unwrap().first_lba > layout.last().unwrap().last_lba {
        return Err("disk image too small for appliance layout".to_string());
    }
    Ok(layout)
}

fn resize_seed_and_recovery(
    layout: &mut [Partition],
    disk_sectors: u64,
    seed_required_bytes: u64,
) -> Result<(), String> {
    if let Some(seed) = layout.iter_mut().find(|part| part.name == "seed") {
        let part_sectors =
            max64(64 * MIB, align_up_u64(seed_required_bytes, MIB)) / SECTOR_SIZE as u64;
        seed.last_lba = seed.first_lba + part_sectors - 1;
    }
    for index in 1..layout.len() {
        let prev_last = layout[index - 1].last_lba;
        if layout[index].first_lba <= prev_last {
            let current_size = layout[index].last_lba - layout[index].first_lba + 1;
            layout[index].first_lba = align_up_u64(prev_last + 1, 2048);
            layout[index].last_lba = layout[index].first_lba + current_size - 1;
        }
    }
    if let Some(recovery) = layout.iter_mut().find(|part| part.name == "recovery") {
        recovery.last_lba = disk_sectors - 34;
        if recovery.first_lba > recovery.last_lba {
            return Err("disk image too small after seed partition sizing".to_string());
        }
    }
    Ok(())
}

fn build_boot_control(
    active_slot: &str,
    pending_slot: &str,
    auto_login: bool,
    suspend_resume_smoke: bool,
) -> Result<Vec<u8>, String> {
    let active = slot_id(active_slot)?;
    let pending = slot_id(pending_slot)?;
    let mut blob = vec![0u8; BOOT_CONTROL_SIZE];
    write_u32(&mut blob, 0, BOOT_CONTROL_MAGIC);
    write_u16(&mut blob, 4, BOOT_CONTROL_VERSION);
    write_u16(&mut blob, 6, BOOT_CONTROL_SIZE as u16);
    blob[8] = active;
    blob[9] = pending;
    blob[10] = 3;
    blob[11] = if pending != 0 { 0 } else { 1 };
    blob[12] = 1;
    write_u64(&mut blob, 24, 1);
    let mut flags = 0u8;
    if auto_login {
        flags |= BOOT_FLAG_AUTO_LOGIN;
    }
    if suspend_resume_smoke {
        flags |= BOOT_FLAG_SUSPEND_RESUME_SMOKE;
    }
    blob[48] = flags;
    write_u32(&mut blob, 128, 0);
    let crc = crc32(&blob);
    write_u32(&mut blob, 128, crc);
    Ok(blob)
}

fn read_esp_extra_files(
    specs: &[String],
) -> Result<(Vec<(String, Vec<u8>)>, Vec<(String, String)>), String> {
    let mut files = Vec::new();
    let mut manifest = Vec::new();
    for spec in specs {
        let (host_text, guest_name) = match spec.split_once("::") {
            Some((host, guest)) => (host.to_string(), guest.to_string()),
            None => {
                let path = PathBuf::from(spec);
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| format!("ESP extra file has no guest name: {spec}"))?
                    .to_string();
                (spec.clone(), file_name)
            }
        };
        if !is_fat83_name(&guest_name) {
            return Err(format!(
                "ESP extra guest name must be FAT 8.3 compatible: {guest_name}"
            ));
        }
        let host_path = PathBuf::from(&host_text);
        let payload = fs::read(&host_path).map_err(|err| {
            format!(
                "ESP extra file not found/readable {}: {err}",
                host_path.display()
            )
        })?;
        files.push((guest_name.clone(), payload));
        manifest.push((host_text, guest_name));
    }
    Ok((files, manifest))
}

fn build_manifest(
    cfg: &ApplianceConfig,
    disk_bytes: u64,
    layout: &[Partition],
    esp_extra_manifest: Vec<(String, String)>,
) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"disk_bytes\": {disk_bytes},\n"));
    out.push_str(&format!(
        "  \"esp_fat\": \"{}\",\n",
        json_escape(&cfg.esp_fat)
    ));
    out.push_str("  \"boot_control_seed\": {\n");
    out.push_str(&format!(
        "    \"active_slot\": \"{}\",\n",
        json_escape(&cfg.active_slot)
    ));
    out.push_str(&format!(
        "    \"pending_slot\": \"{}\",\n",
        json_escape(&cfg.pending_slot)
    ));
    out.push_str(&format!("    \"auto_login\": {},\n", cfg.auto_login));
    out.push_str(&format!(
        "    \"suspend_resume_smoke\": {},\n",
        cfg.suspend_resume_smoke
    ));
    match &cfg.update_smoke_request_url {
        Some(url) => out.push_str(&format!(
            "    \"update_smoke_request_url\": \"{}\",\n",
            json_escape(url)
        )),
        None => out.push_str("    \"update_smoke_request_url\": null,\n"),
    }
    match &cfg.pe_smoke_bundle {
        Some(path) => out.push_str(&format!(
            "    \"pe_smoke_bundle\": \"{}\",\n",
            json_escape(&path.display().to_string())
        )),
        None => out.push_str("    \"pe_smoke_bundle\": null,\n"),
    }
    out.push_str("    \"bundles\": [");
    for (idx, path) in cfg.bundles.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("\"{}\"", json_escape(&path.display().to_string())));
    }
    out.push_str("],\n");
    out.push_str("    \"esp_extra_files\": [");
    for (idx, (host, guest)) in esp_extra_manifest.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"host_path\": \"{}\", \"guest_name\": \"{}\"}}",
            json_escape(host),
            json_escape(guest)
        ));
    }
    out.push_str("]\n");
    out.push_str("  },\n");
    out.push_str("  \"partitions\": [\n");
    for (idx, part) in layout.iter().enumerate() {
        let size_bytes = (part.last_lba - part.first_lba + 1) * SECTOR_SIZE as u64;
        out.push_str(&format!(
            "    {{\"name\": \"{}\", \"first_lba\": {}, \"last_lba\": {}, \"size_bytes\": {}}}",
            part.name, part.first_lba, part.last_lba, size_bytes
        ));
        if idx + 1 != layout.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn write_at(file: &mut File, offset: u64, data: &[u8]) -> Result<(), String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| format!("seek {offset}: {err}"))?;
    file.write_all(data)
        .map_err(|err| format!("write at {offset}: {err}"))
}

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an integer"))
}

fn slot_id(slot: &str) -> Result<u8, String> {
    match slot {
        "none" => Ok(0),
        "system_a" => Ok(1),
        "system_b" => Ok(2),
        "recovery" => Ok(3),
        _ => Err(format!("unsupported slot: {slot}")),
    }
}

fn part<'a>(layout: &'a [Partition], name: &str) -> Result<&'a Partition, String> {
    layout
        .iter()
        .find(|part| part.name == name)
        .ok_or_else(|| format!("partition not found: {name}"))
}

fn upsert_nat(entries: &mut Vec<(u32, u32, u32)>, nid: u32, version: u32, block_addr: u32) {
    if let Some(entry) = entries.iter_mut().find(|entry| entry.0 == nid) {
        *entry = (nid, version, block_addr);
    } else {
        entries.push((nid, version, block_addr));
    }
}

fn short_name(name: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    let (base, ext) = match name.split_once('.') {
        Some((base, ext)) => (base, ext),
        None => (name, ""),
    };
    for (idx, byte) in base
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(8)
        .map(|ch| ch.to_ascii_uppercase() as u8)
        .enumerate()
    {
        out[idx] = byte;
    }
    for (idx, byte) in ext
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(3)
        .map(|ch| ch.to_ascii_uppercase() as u8)
        .enumerate()
    {
        out[8 + idx] = byte;
    }
    out
}

fn is_fat83_name(name: &str) -> bool {
    let (base, ext) = match name.split_once('.') {
        Some((base, ext)) => (base, ext),
        None => (name, ""),
    };
    if base.is_empty() || base.len() > 8 {
        return false;
    }
    if name.contains('.') && (ext.is_empty() || ext.len() > 3) {
        return false;
    }
    base.chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && ext
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn utf16le_name(name: &str) -> [u8; 72] {
    let mut out = [0u8; 72];
    for (idx, unit) in name.encode_utf16().take(36).enumerate() {
        let bytes = unit.to_le_bytes();
        out[idx * 2] = bytes[0];
        out[idx * 2 + 1] = bytes[1];
    }
    out
}

fn make_guid_le(label: &str, salt: u64) -> [u8; 16] {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut state = now ^ ((salt as u128) << 32);
    for byte in label.as_bytes() {
        state = state
            .wrapping_mul(0x100000001b3)
            .wrapping_add(*byte as u128 + 0x9e3779b97f4a7c15);
    }
    let mut out = state.to_le_bytes();
    out[7] = (out[7] & 0x0F) | 0x40;
    out[8] = (out[8] & 0x3F) | 0x80;
    out
}

fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn copy_padded(dst: &mut [u8], src: &[u8]) {
    let count = std::cmp::min(dst.len(), src.len());
    dst[..count].copy_from_slice(&src[..count]);
}

fn align_up_u64(value: u64, alignment: u64) -> u64 {
    ((value + alignment - 1) / alignment) * alignment
}

fn max64(left: u64, right: u64) -> u64 {
    if left > right {
        left
    } else {
        right
    }
}

fn div_ceil(value: usize, divisor: usize) -> usize {
    (value + divisor - 1) / divisor
}

fn write_u16(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(buf: &mut [u8], offset: usize, value: u64) {
    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
