fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Bare-metal / UEFI hedefleri için echshell derle
    if target_os == "none" || target_os == "uefi" {
        build_echshell();
        return;
    }

    // Host hedefleri için normal build (echshell gerektirmez)
    link_msvc_crt_for_curated_c();
    build_sqlite();
    build_lua();
}

/// echshell binary'sini user-mode ELF olarak derle ve OUT_DIR'e kopyala
fn build_echshell() {
    use std::process::Command;

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let echshell_dir = format!("{}/echshell", manifest_dir);

    // echshell'i derle
    println!("cargo:warning=Building echshell for Ring 3...");

    // Stale binary'yi sil (config değişimlerinde cache eski kalabilir)
    let workspace_echshell = format!(
        "{}/target/x86_64-unknown-none/release/echshell",
        manifest_dir
    );
    if std::path::Path::new(&workspace_echshell).exists() {
        let _ = std::fs::remove_file(&workspace_echshell);
    }
    let echshell_local = format!(
        "{}/target/x86_64-unknown-none/release/echshell",
        echshell_dir
    );
    if std::path::Path::new(&echshell_local).exists() {
        let _ = std::fs::remove_file(&echshell_local);
    }

    let linker_script = std::path::Path::new(&echshell_dir)
        .join("linker.ld")
        .canonicalize()
        .expect("Failed to canonicalize echshell linker script")
        .display()
        .to_string()
        .replace('\\', "/");
    let linker_arg = format!("link-arg=-T{}", linker_script);
    let rustflags = [
        "-C".to_string(),
        linker_arg.clone(),
        "-C".to_string(),
        "relocation-model=static".to_string(),
        "-C".to_string(),
        "link-arg=-static".to_string(),
        "-C".to_string(),
        "link-arg=-no-pie".to_string(),
        "-C".to_string(),
        "link-arg=-z".to_string(),
        "-C".to_string(),
        "link-arg=norelro".to_string(),
    ];
    let rustflags_config = format!("target.x86_64-unknown-none.rustflags={:?}", rustflags);
    let rustflags_env = rustflags.join(" ");

    let status = Command::new("cargo")
        .args([
            "--config",
            rustflags_config.as_str(),
            "rustc",
            "--release",
            "--manifest-path",
            &format!("{}/Cargo.toml", echshell_dir),
            "--target",
            "x86_64-unknown-none",
            "--",
            "-C",
            linker_arg.as_str(),
            "-C",
            "relocation-model=static",
            "-C",
            "link-arg=-static",
            "-C",
            "link-arg=-no-pie",
            "-C",
            "link-arg=-z",
            "-C",
            "link-arg=norelro",
        ])
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS", rustflags_env)
        .current_dir(&echshell_dir)
        .status();

    match status {
        Ok(s) if s.success() => {
            // Workspace member olduğu için binary workspace target'ında
            let workspace_target = format!(
                "{}/target/x86_64-unknown-none/release/echshell",
                manifest_dir
            );
            let echshell_target = format!(
                "{}/target/x86_64-unknown-none/release/echshell",
                echshell_dir
            );
            let src = if std::path::Path::new(&workspace_target).exists() {
                workspace_target.clone()
            } else {
                echshell_target.clone()
            };
            let dst = format!("{}/echshell.bin", out_dir);

            if std::path::Path::new(&src).exists() {
                let (elf_type, entry) =
                    read_elf_type_and_entry(&src).expect("Failed to inspect echshell ELF header");
                if elf_type != 2 {
                    panic!(
                        "echshell must be ET_EXEC before embedding, got e_type={} e_entry={:#x}",
                        elf_type, entry
                    );
                }
                std::fs::copy(&src, &dst).expect("Failed to copy echshell.bin to OUT_DIR");
                println!(
                    "cargo:warning=echshell.bin built successfully -> {} (ET_EXEC entry={:#x})",
                    dst, entry
                );
            } else {
                println!(
                    "cargo:warning=echshell binary not found at {} or {}",
                    src, echshell_target
                );
            }
        }
        Ok(s) => {
            panic!("echshell build failed with status: {}", s);
        }
        Err(e) => {
            panic!("Failed to run cargo for echshell: {}", e);
        }
    }

    // echshell kaynak dosyalarını rerun-if-changed olarak işaretle
    for src_file in &[
        "main.rs", "shell_syscall.rs", "tokenizer.rs", "builtins.rs",
        "executor.rs", "environment.rs", "history.rs", "scripting.rs",
    ] {
        println!("cargo:rerun-if-changed={}/src/{}", echshell_dir, src_file);
    }
    println!("cargo:rerun-if-changed={}/linker.ld", echshell_dir);
    println!("cargo:rerun-if-changed={}/Cargo.toml", echshell_dir);
    println!("cargo:rerun-if-changed={}/.cargo/config.toml", manifest_dir);
    println!("cargo:rerun-if-changed={}/.cargo/config.toml", echshell_dir);
    println!("cargo:rerun-if-changed={}/build.rs", manifest_dir);
}

fn read_elf_type_and_entry(path: &str) -> std::io::Result<(u16, u64)> {
    let header = std::fs::read(path)?;
    if header.len() < 0x20 || &header[0..4] != b"\x7FELF" || header[4] != 2 || header[5] != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "echshell is not a little-endian ELF64 image",
        ));
    }

    let elf_type = u16::from_le_bytes([header[0x10], header[0x11]]);
    let entry = u64::from_le_bytes([
        header[0x18],
        header[0x19],
        header[0x1A],
        header[0x1B],
        header[0x1C],
        header[0x1D],
        header[0x1E],
        header[0x1F],
    ]);
    Ok((elf_type, entry))
}

fn link_msvc_crt_for_curated_c() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    println!("cargo:rustc-link-lib=ucrt");
    println!("cargo:rustc-link-lib=vcruntime");
    println!("cargo:rustc-link-lib=legacy_stdio_definitions");
    println!("cargo:rustc-link-lib=oldnames");
}

fn build_sqlite() {
    let sqlite_dir = "third_party/curated/sqlite";
    println!("cargo:rerun-if-changed={sqlite_dir}/sqlite3.c");
    println!("cargo:rerun-if-changed={sqlite_dir}/sqlite3.h");
    println!("cargo:rerun-if-changed={sqlite_dir}/sqlite3ext.h");

    let mut build = cc::Build::new();
    build
        .file(format!("{sqlite_dir}/sqlite3.c"))
        .include(sqlite_dir)
        .define("SQLITE_THREADSAFE", "0")
        .define("SQLITE_OMIT_LOAD_EXTENSION", "1")
        .define("SQLITE_ENABLE_API_ARMOR", "1")
        .warnings(false);

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.define("_CRT_SECURE_NO_WARNINGS", "1");
    }

    build.compile("echos_sqlite");
}

fn build_lua() {
    let lua_dir = "third_party/curated/lua/src";
    for file in [
        "lapi.c",
        "lauxlib.c",
        "lbaselib.c",
        "lcode.c",
        "lcorolib.c",
        "lctype.c",
        "ldebug.c",
        "ldo.c",
        "ldump.c",
        "lfunc.c",
        "lgc.c",
        "llex.c",
        "lmathlib.c",
        "lmem.c",
        "lobject.c",
        "lopcodes.c",
        "lparser.c",
        "lstate.c",
        "lstring.c",
        "lstrlib.c",
        "ltable.c",
        "ltablib.c",
        "ltm.c",
        "lundump.c",
        "lutf8lib.c",
        "lvm.c",
        "lzio.c",
    ] {
        println!("cargo:rerun-if-changed={lua_dir}/{file}");
    }
    println!("cargo:rerun-if-changed={lua_dir}/lua.h");
    println!("cargo:rerun-if-changed={lua_dir}/lauxlib.h");
    println!("cargo:rerun-if-changed={lua_dir}/lualib.h");
    println!("cargo:rerun-if-changed={lua_dir}/luaconf.h");

    let mut build = cc::Build::new();
    build
        .include(lua_dir)
        .define("LUA_USE_C89", "1")
        .warnings(false);

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.define("_CRT_SECURE_NO_WARNINGS", "1");
    }

    for file in [
        "lapi.c",
        "lauxlib.c",
        "lbaselib.c",
        "lcode.c",
        "lcorolib.c",
        "lctype.c",
        "ldebug.c",
        "ldo.c",
        "ldump.c",
        "lfunc.c",
        "lgc.c",
        "llex.c",
        "lmathlib.c",
        "lmem.c",
        "lobject.c",
        "lopcodes.c",
        "lparser.c",
        "lstate.c",
        "lstring.c",
        "lstrlib.c",
        "ltable.c",
        "ltablib.c",
        "ltm.c",
        "lundump.c",
        "lutf8lib.c",
        "lvm.c",
        "lzio.c",
    ] {
        build.file(format!("{lua_dir}/{file}"));
    }

    build.compile("echos_lua");
}
