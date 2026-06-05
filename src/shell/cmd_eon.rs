use alloc::string::{String, ToString};
use core::fmt::Write;

use super::shell_api;

/// eon komutunu isle
pub fn handle_eon_command(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        return Ok(print_help());
    }

    match args[0] {
        "install" => handle_install(&args[1..]),
        "remove" => handle_remove(&args[1..]),
        "list" => handle_list(&args[1..]),
        "info" => handle_info(&args[1..]),
        "search" => handle_search(&args[1..]),
        "update" => handle_update(&args[1..]),
        "verify" => handle_verify(&args[1..]),
        _ => {
            let mut result = String::from("Bilinmeyen komut: ");
            result.push_str(args[0]);
            result.push('\n');
            result.push_str(&print_help());
            Ok(result)
        }
    }
}

fn print_help() -> String {
    let mut help = String::new();
    help.push_str("Kullanim: eon <komut> [secenekler]\n\n");
    help.push_str("Komutlar:\n");
    help.push_str("  install <paket>              - Paket kur\n");
    help.push_str("  remove <paket>               - Paket kaldir\n");
    help.push_str("  list                         - Kurulu paketleri listele\n");
    help.push_str("  info <paket>                 - Paket bilgilerini goster\n");
    help.push_str("  search <terim>               - Paket ara\n");
    help.push_str("  update inspect <yol|url>     - Signed update index incele\n");
    help.push_str("  update apply <yol|url>       - Signed update index uygula\n");
    help.push_str("  update status                - Son installer raporunu goster\n");
    help.push_str("  verify <paket>               - Paket imzasini dogrula\n");
    help
}

fn handle_install(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket yolu belirtilmedi\nKullanim: eon install <paket_yolu>\n",
        ));
    }

    let package_path = args[0];
    match shell_api::ipc_pkg_install(package_path) {
        Ok(msg) => Ok(alloc::format!(
            "Paket kuruluyor: {}\n{}\n",
            package_path,
            msg
        )),
        Err(_) => Ok(String::from(
            "Hata: Paket kurulumu basarisiz veya servis yanit vermedi\n",
        )),
    }
}

fn handle_remove(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket adi belirtilmedi\nKullanim: eon remove <paket_adi>\n",
        ));
    }

    let package_name = args[0];
    match shell_api::ipc_pkg_remove(package_name) {
        Ok(msg) => Ok(alloc::format!(
            "Paket kaldiriliyor: {}\n{}\n",
            package_name,
            msg
        )),
        Err(_) => Ok(String::from(
            "Hata: Paket kaldirma basarisiz veya servis yanit vermedi\n",
        )),
    }
}

fn handle_list(_args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    let mut result = String::new();
    result.push_str("Kurulu paketler:\n");
    result.push_str("================\n");

    match shell_api::ipc_pkg_list() {
        Ok(json) => {
            // Basit JSON array parse — [{"name":"...","version":"...","description":"..."},...]
            if json == "[]" {
                result.push_str("Hicbir paket kurulu degil\n");
            } else {
                // Her bir object'i ayıkla
                let mut i = 0;
                let bytes = json.as_bytes();
                while i < bytes.len() {
                    if bytes[i] == b'{' {
                        // name bul
                        let name = extract_json_field(&json[i..], "name").unwrap_or_default();
                        let version = extract_json_field(&json[i..], "version").unwrap_or_default();
                        let desc =
                            extract_json_field(&json[i..], "description").unwrap_or_default();
                        result.push_str("  ");
                        result.push_str(&name);
                        result.push_str(" - ");
                        result.push_str(&desc);
                        result.push_str(" (v");
                        result.push_str(&version);
                        result.push_str(")\n");
                    }
                    i += 1;
                }
            }
        }
        Err(_) => {
            result.push_str("PackageRegistry servisi yanit vermedi\n");
        }
    }

    Ok(result)
}

fn handle_info(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket adi belirtilmedi\nKullanim: eon info <paket_adi>\n",
        ));
    }

    let package_name = args[0];
    match shell_api::ipc_pkg_info(package_name) {
        Ok(json) => {
            let name = extract_json_field(&json, "name").unwrap_or_default();
            let version = extract_json_field(&json, "version").unwrap_or_default();
            let desc = extract_json_field(&json, "description").unwrap_or_default();
            let author = extract_json_field(&json, "author").unwrap_or_default();
            let mut result = String::new();
            result.push_str("Paket: ");
            result.push_str(&name);
            result.push('\n');
            result.push_str("Versiyon: ");
            result.push_str(&version);
            result.push('\n');
            result.push_str("Aciklama: ");
            result.push_str(&desc);
            result.push('\n');
            result.push_str("Yazar: ");
            result.push_str(&author);
            result.push('\n');
            Ok(result)
        }
        Err(-2) => Ok(alloc::format!("Paket bulunamadi: {}\n", package_name)),
        Err(_) => Ok(String::from("PackageRegistry servisi yanit vermedi\n")),
    }
}

fn handle_search(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        return Ok(String::from(
            "Hata: Arama terimi belirtilmedi\nKullanim: eon search <terim>\n",
        ));
    }

    let search_term = args[0];
    let mut result = String::new();
    result.push_str("Araniyor: '");
    result.push_str(search_term);
    result.push_str("'\n\n");

    match shell_api::ipc_pkg_search(search_term) {
        Ok(json) => {
            if json == "[]" {
                result.push_str("Hicbir sonuc bulunamadi\n");
            } else {
                let mut i = 0;
                let bytes = json.as_bytes();
                while i < bytes.len() {
                    if bytes[i] == b'{' {
                        let name = extract_json_field(&json[i..], "name").unwrap_or_default();
                        let desc =
                            extract_json_field(&json[i..], "description").unwrap_or_default();
                        result.push_str("  ");
                        result.push_str(&name);
                        result.push_str(" - ");
                        result.push_str(&desc);
                        result.push('\n');
                    }
                    i += 1;
                }
            }
        }
        Err(_) => {
            result.push_str("PackageRegistry servisi yanit vermedi\n");
        }
    }

    Ok(result)
}

fn handle_update(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        return Ok(String::from(
            "Hata: update alt komutu belirtilmedi\nKullanim: eon update <inspect|apply|status> [yol|url]\n",
        ));
    }

    match args[0] {
        "inspect" => {
            let Some(locator) = args.get(1) else {
                return Ok(String::from(
                    "Hata: update index yolu veya URL belirtilmedi\nKullanim: eon update inspect <yol|url>\n",
                ));
            };
            match shell_api::ipc_update_inspect(locator) {
                Ok(json) => {
                    let channel = extract_json_field(&json, "channel").unwrap_or_default();
                    let release = extract_json_field(&json, "release").unwrap_or_default();
                    let signer = extract_json_field(&json, "signer").unwrap_or_default();
                    let reboot = extract_json_field(&json, "requires_reboot").unwrap_or_default();
                    let mut result = String::new();
                    result.push_str("Update inspection\n");
                    result.push_str("================\n");
                    result.push_str("Channel: ");
                    result.push_str(&channel);
                    result.push('\n');
                    result.push_str("Release: ");
                    result.push_str(&release);
                    result.push('\n');
                    result.push_str("Signer: ");
                    result.push_str(&signer);
                    result.push('\n');
                    result.push_str("Requires reboot: ");
                    result.push_str(&reboot);
                    result.push('\n');
                    Ok(result)
                }
                Err(_) => Ok(String::from("UpdateInstaller servisi yanit vermedi\n")),
            }
        }
        "apply" => {
            let Some(locator) = args.get(1) else {
                return Ok(String::from(
                    "Hata: update index yolu veya URL belirtilmedi\nKullanim: eon update apply <yol|url>\n",
                ));
            };
            match shell_api::ipc_update_apply(locator) {
                Ok(json) => {
                    let state = extract_json_field(&json, "state").unwrap_or_default();
                    let release = extract_json_field(&json, "release").unwrap_or_default();
                    let reboot = extract_json_field(&json, "requires_reboot").unwrap_or_default();
                    let mut result = String::new();
                    result.push_str("Update apply sonucu\n");
                    result.push_str("===================\n");
                    result.push_str("State: ");
                    result.push_str(&state);
                    result.push('\n');
                    result.push_str("Release: ");
                    result.push_str(&release);
                    result.push('\n');
                    result.push_str("Requires reboot: ");
                    result.push_str(&reboot);
                    result.push('\n');
                    Ok(result)
                }
                Err(_) => Ok(String::from("UpdateInstaller servisi yanit vermedi\n")),
            }
        }
        "status" => {
            match shell_api::ipc_update_status() {
                Ok(json) => {
                    if json == "null" {
                        Ok(String::from("Installer raporu bulunamadi\n"))
                    } else {
                        let state = extract_json_field(&json, "state").unwrap_or_default();
                        let release = extract_json_field(&json, "release").unwrap_or_default();
                        let reboot = extract_json_field(&json, "requires_reboot").unwrap_or_default();
                        let mut result = String::new();
                        result.push_str("Son installer raporu\n");
                        result.push_str("====================\n");
                        result.push_str("State: ");
                        result.push_str(&state);
                        result.push('\n');
                        result.push_str("Release: ");
                        result.push_str(&release);
                        result.push('\n');
                        result.push_str("Requires reboot: ");
                        result.push_str(&reboot);
                        result.push('\n');
                        Ok(result)
                    }
                }
                Err(_) => Ok(String::from("UpdateInstaller servisi yanit vermedi\n")),
            }
        }
        _ => Ok(String::from(
            "Hata: bilinmeyen update alt komutu\nKullanim: eon update <inspect|apply|status> [yol|url]\n",
        )),
    }
}

fn handle_verify(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket adi belirtilmedi\nKullanim: eon verify <paket_adi>\n",
        ));
    }

    let package_name = args[0];
    let mut result = String::new();
    result.push_str("Paket imzasi dogrulaniyor: ");
    result.push_str(package_name);
    result.push('\n');

    match shell_api::ipc_pkg_verify(package_name) {
        Ok(_) => {
            result.push_str("OK ");
            result.push_str(package_name);
            result.push_str(" paket butunlugu dogrulandi\n");
        }
        Err(_) => {
            result.push_str("Hata: Paket dogrulanamadi veya servis yanit vermedi\n");
        }
    }

    Ok(result)
}

/// Basit JSON field extraction — "key":"value" formatı için
fn extract_json_field(json: &str, key: &str) -> Option<String> {
    let pattern = alloc::format!("\"{}\":\"", key);
    if let Some(pos) = json.find(&pattern) {
        let start = pos + pattern.len();
        let end = json[start..]
            .find('"')
            .map(|i| start + i)
            .unwrap_or(json.len());
        Some(json[start..end].to_string())
    } else {
        // Bool değerler için: "key":true veya "key":false
        let pattern_bool = alloc::format!("\"{}\":", key);
        if let Some(pos) = json.find(&pattern_bool) {
            let start = pos + pattern_bool.len();
            let end = json[start..]
                .find(|c: char| c == ',' || c == '}')
                .map(|i| start + i)
                .unwrap_or(json.len());
            Some(json[start..end].to_string())
        } else {
            None
        }
    }
}
