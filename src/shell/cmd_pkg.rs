use alloc::string::{String, ToString};
use core::fmt::Write;

/// pkg komutunu isle
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
    help.push_str("Kullanim: pkg <komut> [secenekler]\n\n");
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
    use crate::runtime_layer::service_control::{PackageRegistryCommand, PackageRegistryResponse};

    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket yolu belirtilmedi\nKullanim: pkg install <paket_yolu>\n",
        ));
    }

    let package_path = args[0];
    let response = crate::ipc::request_package_registry_sync(
        0,
        PackageRegistryCommand::InstallFromPath(package_path.to_string()),
    );
    let Some(response) = response else {
        return Ok(String::from("PackageRegistry servisi yanit vermedi\n"));
    };
    match response {
        PackageRegistryResponse::Lifecycle(report) => {
            let mut result = String::new();
            result.push_str("Paket kuruluyor: ");
            result.push_str(report.subject.as_str());
            result.push('\n');
            result.push_str("OK ");
            result.push_str(report.detail.as_str());
            result.push('\n');
            result.push_str("Launchpad guncelleniyor...\n");
            Ok(result)
        }
        PackageRegistryResponse::Error(error) => Ok(alloc::format!(
            "Hata: Paket kurulumu basarisiz: {} ({})\n",
            error.detail,
            error.kind.as_str()
        )),
        _ => Ok(String::from(
            "Hata: PackageRegistry beklenmeyen yanit verdi\n",
        )),
    }
}

fn handle_remove(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    use crate::runtime_layer::service_control::{PackageRegistryCommand, PackageRegistryResponse};

    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket adi belirtilmedi\nKullanim: pkg remove <paket_adi>\n",
        ));
    }

    let package_name = args[0];
    let response = crate::ipc::request_package_registry_sync(
        0,
        PackageRegistryCommand::RemovePackage(package_name.to_string()),
    );
    let Some(response) = response else {
        return Ok(String::from("PackageRegistry servisi yanit vermedi\n"));
    };
    match response {
        PackageRegistryResponse::Lifecycle(report) => {
            let mut result = String::new();
            result.push_str("Paket kaldiriliyor: ");
            result.push_str(report.subject.as_str());
            result.push('\n');
            result.push_str("OK ");
            result.push_str(report.subject.as_str());
            result.push_str(" paketi kaldirildi\n");
            result.push_str("Launchpad guncelleniyor...\n");
            Ok(result)
        }
        PackageRegistryResponse::Error(error) => Ok(alloc::format!(
            "Hata: Paket kaldirma basarisiz: {} ({})\n",
            error.detail,
            error.kind.as_str()
        )),
        _ => Ok(String::from(
            "Hata: PackageRegistry beklenmeyen yanit verdi\n",
        )),
    }
}

fn handle_list(_args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    use crate::runtime_layer::service_control::{PackageRegistryCommand, PackageRegistryResponse};

    let mut result = String::new();
    result.push_str("Kurulu paketler:\n");
    result.push_str("================\n");

    let response =
        crate::ipc::request_package_registry_sync(0, PackageRegistryCommand::ListPackages);
    let Some(response) = response else {
        result.push_str("PackageRegistry servisi yanit vermedi\n");
        return Ok(result);
    };
    let packages = match response {
        PackageRegistryResponse::Packages(packages) => packages,
        PackageRegistryResponse::Error(error) => {
            let _ = writeln!(result, "Hata: {} ({})", error.detail, error.kind.as_str());
            return Ok(result);
        }
        _ => {
            result.push_str("Hata: PackageRegistry beklenmeyen yanit verdi\n");
            return Ok(result);
        }
    };

    if packages.is_empty() {
        result.push_str("Hicbir paket kurulu degil\n");
    } else {
        for record in packages {
            result.push_str("  ");
            result.push_str(record.name.as_str());
            result.push_str(" - ");
            result.push_str(record.info.description.as_deref().unwrap_or("Aciklama yok"));
            result.push_str(" (v");
            result.push_str(record.info.version.as_deref().unwrap_or("0.0.0"));
            result.push_str(")\n");
        }
    }

    Ok(result)
}

fn handle_info(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    use crate::runtime_layer::service_control::{PackageRegistryCommand, PackageRegistryResponse};

    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket adi belirtilmedi\nKullanim: pkg info <paket_adi>\n",
        ));
    }

    let package_name = args[0];
    let mut result = String::new();

    let response = crate::ipc::request_package_registry_sync(
        0,
        PackageRegistryCommand::DescribePackage(package_name.to_string()),
    );
    let Some(response) = response else {
        result.push_str("PackageRegistry servisi yanit vermedi\n");
        return Ok(result);
    };
    match response {
        PackageRegistryResponse::Package(Some(record)) => {
            let info = record.info;
            result.push_str("Paket: ");
            result.push_str(record.name.as_str());
            result.push('\n');
            result.push_str("Versiyon: ");
            result.push_str(info.version.as_deref().unwrap_or("Bilinmiyor"));
            result.push('\n');
            result.push_str("Aciklama: ");
            result.push_str(info.description.as_deref().unwrap_or("Aciklama yok"));
            result.push('\n');
            result.push_str("Yazar: ");
            result.push_str(info.author.as_deref().unwrap_or("Bilinmiyor"));
            result.push('\n');

            if let Some(permissions) = &info.permissions {
                result.push_str("Izinler:\n");
                for perm in permissions {
                    result.push_str("  - ");
                    result.push_str(perm);
                    result.push('\n');
                }
            }
        }
        PackageRegistryResponse::Package(None) => {
            result.push_str("Paket bulunamadi: ");
            result.push_str(package_name);
            result.push('\n');
        }
        PackageRegistryResponse::Error(error) => {
            let _ = writeln!(result, "Hata: {} ({})", error.detail, error.kind.as_str());
        }
        _ => result.push_str("Hata: PackageRegistry beklenmeyen yanit verdi\n"),
    }

    Ok(result)
}

fn handle_search(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    use crate::runtime_layer::service_control::{PackageRegistryCommand, PackageRegistryResponse};

    if args.is_empty() {
        return Ok(String::from(
            "Hata: Arama terimi belirtilmedi\nKullanim: pkg search <terim>\n",
        ));
    }

    let search_term = args[0].to_lowercase();
    let mut result = String::new();
    result.push_str("Araniyor: '");
    result.push_str(args[0]);
    result.push_str("'\n\n");

    let response = crate::ipc::request_package_registry_sync(
        0,
        PackageRegistryCommand::SearchPackages(search_term),
    );
    let Some(response) = response else {
        result.push_str("PackageRegistry servisi yanit vermedi\n");
        return Ok(result);
    };
    let packages = match response {
        PackageRegistryResponse::Packages(packages) => packages,
        PackageRegistryResponse::Error(error) => {
            let _ = writeln!(result, "Hata: {} ({})", error.detail, error.kind.as_str());
            return Ok(result);
        }
        _ => {
            result.push_str("Hata: PackageRegistry beklenmeyen yanit verdi\n");
            return Ok(result);
        }
    };

    if packages.is_empty() {
        result.push_str("Hicbir sonuc bulunamadi\n");
    } else {
        for record in packages {
            result.push_str("  ");
            result.push_str(record.name.as_str());
            result.push_str(" - ");
            result.push_str(record.info.description.as_deref().unwrap_or("Aciklama yok"));
            result.push_str(" (v");
            result.push_str(record.info.version.as_deref().unwrap_or("0.0.0"));
            result.push_str(")\n");
        }
    }

    Ok(result)
}

fn handle_update(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    use crate::runtime_layer::service_control::{UpdateInstallerCommand, UpdateInstallerResponse};

    if args.is_empty() {
        return Ok(String::from(
            "Hata: update alt komutu belirtilmedi\nKullanim: pkg update <inspect|apply|status> [yol|url]\n",
        ));
    }

    let response = match args[0] {
        "inspect" => {
            let Some(locator) = args.get(1) else {
                return Ok(String::from(
                    "Hata: update index yolu veya URL belirtilmedi\nKullanim: pkg update inspect <yol|url>\n",
                ));
            };
            crate::ipc::request_update_installer_sync(
                0,
                UpdateInstallerCommand::Inspect((*locator).to_string()),
            )
        }
        "apply" => {
            let Some(locator) = args.get(1) else {
                return Ok(String::from(
                    "Hata: update index yolu veya URL belirtilmedi\nKullanim: pkg update apply <yol|url>\n",
                ));
            };
            crate::ipc::request_update_installer_sync(
                0,
                UpdateInstallerCommand::Apply((*locator).to_string()),
            )
        }
        "status" => crate::ipc::request_update_installer_sync(0, UpdateInstallerCommand::Status),
        _ => {
            return Ok(String::from(
                "Hata: bilinmeyen update alt komutu\nKullanim: pkg update <inspect|apply|status> [yol|url]\n",
            ))
        }
    };

    let Some(response) = response else {
        return Ok(String::from("UpdateInstaller servisi yanit vermedi\n"));
    };

    let mut result = String::new();
    match response {
        UpdateInstallerResponse::Inspection(inspection) => {
            result.push_str("Update inspection\n");
            result.push_str("================\n");
            result.push_str("Channel: ");
            result.push_str(inspection.index.channel.as_str());
            result.push('\n');
            result.push_str("Release: ");
            result.push_str(inspection.index.release.as_str());
            result.push('\n');
            result.push_str("Signer: ");
            result.push_str(inspection.signature.signer_key_id.as_str());
            result.push('\n');
            result.push_str("Plan: ");
            result.push_str(inspection.plan.class.as_str());
            result.push('\n');
            result.push_str("Requires reboot: ");
            result.push_str(if inspection.plan.requires_reboot {
                "evet"
            } else {
                "hayir"
            });
            result.push('\n');
            result.push_str("Artifacts:\n");
            for artifact in inspection.index.artifacts {
                result.push_str("  - ");
                result.push_str(artifact.kind.as_str());
                result.push_str(": ");
                result.push_str(artifact.id.as_str());
                result.push_str(" v");
                result.push_str(artifact.version.as_str());
                result.push('\n');
            }
        }
        UpdateInstallerResponse::Apply(report) => {
            result.push_str("Update apply sonucu\n");
            result.push_str("===================\n");
            result.push_str("State: ");
            result.push_str(report.state.as_str());
            result.push('\n');
            result.push_str("Release: ");
            result.push_str(report.release.as_str());
            result.push('\n');
            result.push_str("Plan: ");
            result.push_str(report.plan_class.as_str());
            result.push('\n');
            result.push_str("Requires reboot: ");
            result.push_str(if report.requires_reboot {
                "evet"
            } else {
                "hayir"
            });
            result.push('\n');
            result.push_str("Applied artifacts:\n");
            for artifact in report.applied_artifacts {
                result.push_str("  - ");
                result.push_str(artifact.as_str());
                result.push('\n');
            }
        }
        UpdateInstallerResponse::Status(Some(report)) => {
            result.push_str("Son installer raporu\n");
            result.push_str("====================\n");
            result.push_str("State: ");
            result.push_str(report.state.as_str());
            result.push('\n');
            result.push_str("Release: ");
            result.push_str(report.release.as_str());
            result.push('\n');
            result.push_str("Plan: ");
            result.push_str(report.plan_class.as_str());
            result.push('\n');
            result.push_str("Requires reboot: ");
            result.push_str(if report.requires_reboot {
                "evet"
            } else {
                "hayir"
            });
            result.push('\n');
        }
        UpdateInstallerResponse::Status(None) => {
            result.push_str("Installer raporu bulunamadi\n");
        }
        UpdateInstallerResponse::Error(error) => {
            result.push_str("Hata: Update installer: ");
            result.push_str(error.detail.as_str());
            result.push_str(" (");
            result.push_str(error.kind.as_str());
            result.push(')');
            result.push('\n');
        }
    }

    Ok(result)
}

fn handle_verify(args: &[&str]) -> Result<String, crate::shell::scripting::ScriptError> {
    use crate::runtime_layer::service_control::{PackageRegistryCommand, PackageRegistryResponse};

    if args.is_empty() {
        return Ok(String::from(
            "Hata: Paket adi belirtilmedi\nKullanim: pkg verify <paket_adi>\n",
        ));
    }

    let package_name = args[0];
    let mut result = String::new();
    result.push_str("Paket imzasi dogrulaniyor: ");
    result.push_str(package_name);
    result.push('\n');

    let response = crate::ipc::request_package_registry_sync(
        0,
        PackageRegistryCommand::VerifyPackage(package_name.to_string()),
    );
    let Some(response) = response else {
        result.push_str("PackageRegistry servisi yanit vermedi\n");
        return Ok(result);
    };
    match response {
        PackageRegistryResponse::Lifecycle(_) => {
            result.push_str("OK ");
            result.push_str(package_name);
            result.push_str(" paket butunlugu dogrulandi\n");
        }
        PackageRegistryResponse::Error(error) if error.kind.as_str() == "invalid-signature" => {
            result.push_str("Hata: ");
            result.push_str(package_name);
            result.push_str(" paketinin imzasi gecersiz\n");
        }
        PackageRegistryResponse::Error(error) => {
            result.push_str("Hata: Paket dogrulanamadi: ");
            result.push_str(error.detail.as_str());
            result.push_str(" (");
            result.push_str(error.kind.as_str());
            result.push_str(")\n");
        }
        _ => result.push_str("Hata: PackageRegistry beklenmeyen yanit verdi\n"),
    }

    Ok(result)
}
