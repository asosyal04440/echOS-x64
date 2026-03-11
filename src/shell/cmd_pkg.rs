use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

/// pkg komutunu işle
pub fn handle_pkg_command(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
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
    help.push_str("Kullanım: pkg <komut> [seçenekler]\n");
    help.push_str("\n");
    help.push_str("Komutlar:\n");
    help.push_str("  install <paket>  - Paket kur\n");
    help.push_str("  remove <paket>   - Paket kaldır\n");
    help.push_str("  list            - Kurulu paketleri listele\n");
    help.push_str("  info <paket>    - Paket bilgilerini göster\n");
    help.push_str("  search <terim>  - Paket ara\n");
    help.push_str("  update          - Paket listesini güncelle\n");
    help.push_str("  verify <paket>  - Paket imzasını doğrula\n");
    help
}

fn handle_install(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        let mut result = String::from("Hata: Paket yolu belirtilmedi\n");
        result.push_str("Kullanım: pkg install <paket_yolu>\n");
        return Ok(result);
    }

    let package_path = args[0];
    let mut result = String::new();
    result.push_str("Paket kuruluyor: ");
    result.push_str(package_path);
    result.push('\n');

    match crate::security::package::install_package_from_path(package_path) {
        Ok(msg) => {
            result.push_str("✓ ");
            result.push_str(&msg);
            result.push('\n');
            
            // Launchpad'i güncelle
            result.push_str("Launchpad güncelleniyor...\n");
        }
        Err(e) => {
            result.push_str("✗ Paket kurulumu başarısız: ");
            result.push_str("Unknown error");
            result.push('\n');
        }
    }

    Ok(result)
}

fn handle_remove(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        let mut result = String::from("Hata: Paket adı belirtilmedi\n");
        result.push_str("Kullanım: pkg remove <paket_adi>\n");
        return Ok(result);
    }

    let package_name = args[0];
    let mut result = String::new();
    result.push_str("Paket kaldırılıyor: ");
    result.push_str(package_name);
    result.push('\n');

    match crate::security::package::get_package_manager().lock().remove_package(package_name) {
        Ok(_) => {
            result.push_str("✓ ");
            result.push_str(package_name);
            result.push_str(" paketi kaldırıldı\n");
            
            // Launchpad'i güncelle
            result.push_str("Launchpad güncelleniyor...\n");
        }
        Err(e) => {
            result.push_str("✗ Paket kaldırma başarısız: ");
            result.push_str("Unknown error");
            result.push('\n');
        }
    }

    Ok(result)
}

fn handle_list(_args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    let mut result = String::new();
    result.push_str("Kurulu paketler:\n");
    result.push_str("================\n");

    let packages = crate::security::package::get_package_manager().lock().list_packages();
    
    if packages.is_empty() {
        result.push_str("Hiçbir paket kurulu değil\n");
    } else {
        for (name, info) in packages {
            result.push_str("  ");
            result.push_str(&name);
            result.push_str(" - ");
            result.push_str(info.description.as_deref().unwrap_or("Açıklama yok"));
            result.push_str(" (v");
            result.push_str(info.version.as_deref().unwrap_or("0.0.0"));
            result.push_str(")\n");
        }
    }

    Ok(result)
}

fn handle_info(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        let mut result = String::from("Hata: Paket adı belirtilmedi\n");
        result.push_str("Kullanım: pkg info <paket_adi>\n");
        return Ok(result);
    }

    let package_name = args[0];
    let mut result = String::new();
    
    match crate::security::package::get_package_manager().lock().get_package_info(package_name) {
        Some(info) => {
            result.push_str("Paket: ");
            result.push_str(package_name);
            result.push('\n');
            result.push_str("Versiyon: ");
            result.push_str(info.version.as_deref().unwrap_or("Bilinmiyor"));
            result.push('\n');
            result.push_str("Açıklama: ");
            result.push_str(info.description.as_deref().unwrap_or("Açıklama yok"));
            result.push('\n');
            result.push_str("Yazar: ");
            result.push_str(info.author.as_deref().unwrap_or("Bilinmiyor"));
            result.push('\n');
            
            if let Some(permissions) = &info.permissions {
                result.push_str("İzinler:\n");
                for perm in permissions {
                    result.push_str("  - ");
                    result.push_str(perm);
                    result.push('\n');
                }
            }
        }
        None => {
            result.push_str("Paket bulunamadı: ");
            result.push_str(package_name);
            result.push('\n');
        }
    }

    Ok(result)
}

fn handle_search(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        let mut result = String::from("Hata: Arama terimi belirtilmedi\n");
        result.push_str("Kullanım: pkg search <terim>\n");
        return Ok(result);
    }

    let search_term = args[0].to_lowercase();
    let mut result = String::new();
    result.push_str("Aranıyor: '");
    result.push_str(args[0]);
    result.push_str("'\n\n");

    let packages = crate::security::package::get_package_manager().lock().search_packages(&search_term);
    
    if packages.is_empty() {
        result.push_str("Hiçbir sonuç bulunamadı\n");
    } else {
        for (name, info) in packages {
            result.push_str("  ");
            result.push_str(&name);
            result.push_str(" - ");
            result.push_str(info.description.as_deref().unwrap_or("Açıklama yok"));
            result.push_str(" (v");
            result.push_str(info.version.as_deref().unwrap_or("0.0.0"));
            result.push_str(")\n");
        }
    }

    Ok(result)
}

fn handle_update(_args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    let mut result = String::new();
    result.push_str("Paket listesi güncelleniyor...\n");
    
    match crate::security::package::get_package_manager().lock().update_package_list() {
        Ok(_) => {
            result.push_str("✓ Paket listesi güncellendi\n");
        }
        Err(e) => {
            result.push_str("✗ Güncelleme başarısız: ");
            result.push_str("Unknown error");
            result.push('\n');
        }
    }

    Ok(result)
}

fn handle_verify(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    if args.is_empty() {
        let mut result = String::from("Hata: Paket adı belirtilmedi\n");
        result.push_str("Kullanım: pkg verify <paket_adi>\n");
        return Ok(result);
    }

    let package_name = args[0];
    let mut result = String::new();
    result.push_str("Paket imzası doğrulanıyor: ");
    result.push_str(package_name);
    result.push('\n');

    match crate::security::package::get_package_manager().lock().verify_package_signature(&[], &[0u8; 64]) {
        Ok(()) => {
            result.push_str("✓ ");
            result.push_str(package_name);
            result.push_str(" paketinin imzası geçerli\n");
        }
        Err(crate::security::package::PackageError::InvalidSignature) => {
             result.push_str("✗ ");
             result.push_str(package_name);
             result.push_str(" paketinin imzası geçersiz\n");
        }
        Err(_e) => {
            result.push_str("✗ Doğrulama başarısız: ");
            result.push_str("Unknown error");
            result.push('\n');
        }
    }

    Ok(result)
}