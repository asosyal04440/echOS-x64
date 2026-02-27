//! echOS Derleme Betiği (build.rs)
//!
//! Bu dosya; Cargo derleme sürecini yönetir. İki temel görevi vardır:
//! 1. C tabanlı virtio sürücüsünü (src/c_drivers/virtio.c) derleyip bağlar.
//! 2. ECHOS_VERIFY=1 ortam değişkeni ayarlandığında `cargo fmt --check`,
//!    `cargo check` ve `cargo clippy` aşamalarından oluşan bir doğrulama
//!    ardışık düzeni (verify pipeline) çalıştırır.

fn main() {
    println!("cargo:rerun-if-changed=src/c_drivers/virtio.c");
    println!("cargo:rerun-if-changed=src/c_drivers/virtio.h");
    run_verify_pipeline();
    let compiler = match std::env::var("CC") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            return;
        }
    };
    cc::Build::new()
        .compiler(compiler)
        .file("src/c_drivers/virtio.c")
        .include("src/c_drivers")
        .flag("-ffreestanding")
        .flag("-fno-builtin")
        .flag("-fno-stack-protector")
        .compile("virtio_c");
}

fn run_verify_pipeline() {
    let enabled = std::env::var("ECHOS_VERIFY").ok().as_deref() == Some("1");
    let already_running = std::env::var("ECHOS_VERIFY_RUNNING").ok().as_deref() == Some("1");
    if !enabled || already_running {
        return;
    }
    let _ = run_cargo(&["fmt", "--", "--check"]);
    let _ = run_cargo(&["check", "--all-targets"]);
    let _ = run_cargo(&["clippy", "--all-targets", "--", "-D", "warnings"]);
}

fn run_cargo(args: &[&str]) -> std::process::ExitStatus {
    let status = std::process::Command::new("cargo")
        .args(args)
        .env("ECHOS_VERIFY_RUNNING", "1")
        .status()
        .expect("verify pipeline failed to invoke cargo");
    if !status.success() {
        let mut cmd = String::from("cargo");
        for arg in args {
            cmd.push(' ');
            cmd.push_str(arg);
        }
        panic!("verify pipeline failed: {}", cmd);
    }
    status
}
