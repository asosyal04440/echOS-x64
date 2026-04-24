use ech_os::security::package::{
    build_revocation_feed, build_signed_bundle, inspect_signed_bundle,
};
use ech_os::update::{
    build_signed_index, hex_digest, inspect_signed_index, UpdateArtifact, UpdateArtifactKind,
    UpdateIndex, UpdateIndexSignatureProfile,
};
use echos_manifest::{CompiledAppManifest, SourceAppManifest, TrustDomain};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    if let Err(err) = run() {
        eprintln!("echsdk: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_help();
        return Ok(());
    };
    match command.as_str() {
        "new" => {
            let name = args
                .next()
                .ok_or_else(|| String::from("missing app name"))?;
            let template = args.next().unwrap_or_else(|| String::from("window-basic"));
            cmd_new(&name, &template)
        }
        "manifest" => {
            let sub = args.next().unwrap_or_default();
            if sub != "check" {
                return Err(String::from("usage: echosdk manifest check [path]"));
            }
            let path = args
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("echos-app.toml"));
            cmd_manifest_check(&path)
        }
        "build" => {
            let root = root_arg(args.next());
            cmd_build(&root)
        }
        "package" => {
            let root = root_arg(args.next());
            let out = args.next().map(PathBuf::from);
            cmd_package(&root, out, TrustDomain::Developer).map(|_| ())
        }
        "sign" => {
            let root = root_arg(args.next());
            let trust = parse_trust_domain(args.next().as_deref().unwrap_or("developer"))?;
            let out = args.next().map(PathBuf::from);
            cmd_package(&root, out, trust).map(|_| ())
        }
        "verify" => {
            let bundle = args
                .next()
                .ok_or_else(|| String::from("missing bundle path"))?;
            cmd_verify(Path::new(&bundle))
        }
        "revocation-feed" => {
            let output = args
                .next()
                .ok_or_else(|| String::from("missing output path"))?;
            let minimum_epoch = args
                .next()
                .ok_or_else(|| String::from("missing minimum epoch"))?
                .parse::<u32>()
                .map_err(|_| String::from("minimum epoch must be u32"))?;
            let revoked = args.collect::<Vec<_>>();
            cmd_revocation_feed(Path::new(&output), minimum_epoch, &revoked)
        }
        "exactness" => {
            let strict = matches!(args.next().as_deref(), Some("strict"));
            cmd_exactness(strict)
        }
        "field-profile" => {
            let sub = args
                .next()
                .ok_or_else(|| String::from("usage: echosdk field-profile <capture|validate> ..."))?;
            match sub.as_str() {
                "capture" => {
                    let output = args
                        .next()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("artifacts/field/physical_profile.json"));
                    cmd_field_profile_capture(&output)
                }
                "validate" => {
                    let profile = args
                        .next()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("artifacts/field/physical_profile.json"));
                    let admission = args
                        .next()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            PathBuf::from("docs/agent/single-pc-admission-profile.json")
                        });
                    cmd_field_profile_validate(&profile, &admission)
                }
                _ => Err(format!("unknown field-profile subcommand: {}", sub)),
            }
        }
        "install" => {
            let bundle = args
                .next()
                .ok_or_else(|| String::from("missing bundle path"))?;
            let stage_root = stage_root_arg(args.next());
            cmd_install(Path::new(&bundle), &stage_root)
        }
        "update" => {
            let sub = args
                .next()
                .ok_or_else(|| String::from("usage: echosdk update <publish|inspect|stage> ..."))?;
            match sub.as_str() {
                "publish" => {
                    let spec = args
                        .next()
                        .ok_or_else(|| String::from("missing update spec path"))?;
                    let out = args
                        .next()
                        .ok_or_else(|| String::from("missing signed index output path"))?;
                    let profile = parse_update_signing_profile(
                        args.next().as_deref().unwrap_or("engineering"),
                    )?;
                    cmd_update_publish(Path::new(&spec), Path::new(&out), profile)
                }
                "inspect" => {
                    let index = args
                        .next()
                        .ok_or_else(|| String::from("missing signed index path"))?;
                    cmd_update_inspect(Path::new(&index))
                }
                "stage" => {
                    let bundle = args
                        .next()
                        .ok_or_else(|| String::from("missing bundle path"))?;
                    let stage_root = stage_root_arg(args.next());
                    cmd_update_stage(Path::new(&bundle), &stage_root)
                }
                _ => Err(format!("unknown update subcommand: {}", sub)),
            }
        }
        "remove" => {
            let bundle_name = args
                .next()
                .ok_or_else(|| String::from("missing bundle name"))?;
            let stage_root = stage_root_arg(args.next());
            cmd_remove(&bundle_name, &stage_root)
        }
        "repair" => {
            let bundle = args
                .next()
                .ok_or_else(|| String::from("missing bundle path"))?;
            let stage_root = stage_root_arg(args.next());
            cmd_repair(Path::new(&bundle), &stage_root)
        }
        "launch" => {
            let bundle = args
                .next()
                .ok_or_else(|| String::from("missing bundle path"))?;
            cmd_launch(Path::new(&bundle))
        }
        "run" => {
            let root = root_arg(args.next());
            let bundle = cmd_package(&root, None, TrustDomain::Developer)?;
            let stage_root = PathBuf::from("artifacts/packaged-installs");
            cmd_install(&bundle, &stage_root)?;
            cmd_launch(&bundle)
        }
        "test" => {
            let root = root_arg(args.next());
            cmd_test(&root)
        }
        _ => Err(format!("unknown command: {}", command)),
    }
}

fn cmd_new(name: &str, template: &str) -> Result<(), String> {
    let root = PathBuf::from(name);
    if root.exists() {
        return Err(String::from("target directory already exists"));
    }
    fs::create_dir_all(root.join("src")).map_err(io_error)?;
    let cargo = format!(
        "[package]\nname = \"{0}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nechos-sdk = {{ path = \"../sdk/echos-sdk\" }}\n\n[profile.release]\npanic = \"abort\"\n",
        name
    );
    fs::write(root.join("Cargo.toml"), cargo).map_err(io_error)?;
    let manifest = format!("{}", manifest_template(name, template));
    fs::write(root.join("echos-app.toml"), manifest).map_err(io_error)?;
    let main_rs = match template {
        "document-basic" => document_template(name),
        "stateful-basic" => stateful_template(name),
        "service-basic" => service_template(name),
        _ => window_template(name),
    };
    fs::write(root.join("src/main.rs"), main_rs).map_err(io_error)?;
    println!("Created {}", root.display());
    Ok(())
}

fn cmd_manifest_check(path: &Path) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(io_error)?;
    let manifest = SourceAppManifest::parse(&text).map_err(|err| format!("{:?}", err))?;
    println!(
        "Manifest OK: {} -> {} (sdk v{}, runtime {}, state {})",
        manifest.app_id,
        manifest.entry,
        manifest.sdk_version,
        manifest.runtime.as_str(),
        manifest.state_contract.as_str()
    );
    Ok(())
}

fn cmd_build(root: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("x86_64-unknown-none")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .status()
        .map_err(io_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed with {}", status))
    }
}

fn cmd_package(
    root: &Path,
    out: Option<PathBuf>,
    trust_domain: TrustDomain,
) -> Result<PathBuf, String> {
    let manifest_path = root.join("echos-app.toml");
    let manifest_text = fs::read_to_string(&manifest_path).map_err(io_error)?;
    let source_manifest =
        SourceAppManifest::parse(&manifest_text).map_err(|err| format!("{:?}", err))?;
    let entry_path = root.join(&source_manifest.entry);
    let entry_bytes = fs::read(&entry_path).map_err(io_error)?;
    let entry_sha256 = sha256_array(&entry_bytes);
    let compiled = CompiledAppManifest::from_source(&source_manifest, entry_sha256)
        .map_err(|err| format!("{:?}", err))?;
    let output = out.unwrap_or_else(|| {
        root.join("dist").join(format!(
            "{}-{}.bhd",
            source_manifest.app_id, source_manifest.version
        ))
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let bundle = build_signed_bundle(&source_manifest, &compiled, &entry_bytes, trust_domain)
        .map_err(|err| err.to_string())?;
    fs::write(&output, bundle).map_err(io_error)?;
    println!("{}", output.display());
    Ok(output)
}

fn cmd_verify(bundle: &Path) -> Result<(), String> {
    let bytes = fs::read(bundle).map_err(io_error)?;
    let inspection = inspect_signed_bundle(&bytes).map_err(|err| err.to_string())?;
    println!(
        "Verified {} [{}] -> {} signer={} trust={}",
        inspection.compiled_manifest.app_id,
        inspection.compiled_manifest.runtime.as_str(),
        inspection.compiled_manifest.entry,
        inspection.signature_metadata.signer_key_id,
        inspection.signature_metadata.trust_domain.as_str()
    );
    Ok(())
}

fn cmd_revocation_feed(
    output: &Path,
    minimum_epoch: u32,
    revoked: &[String],
) -> Result<(), String> {
    let bytes = build_revocation_feed(minimum_epoch, revoked).map_err(|err| err.to_string())?;
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
    }
    fs::write(output, bytes).map_err(io_error)?;
    println!("{}", output.display());
    Ok(())
}

fn cmd_install(bundle: &Path, stage_root: &Path) -> Result<(), String> {
    let target = stage_root.join(
        bundle
            .file_name()
            .ok_or_else(|| String::from("invalid bundle path"))?,
    );
    fs::create_dir_all(stage_root).map_err(io_error)?;
    fs::copy(bundle, &target).map_err(io_error)?;
    println!("Staged {}", target.display());
    Ok(())
}

fn cmd_update_stage(bundle: &Path, stage_root: &Path) -> Result<(), String> {
    cmd_verify(bundle)?;
    cmd_install(bundle, stage_root)
}

fn cmd_update_publish(
    spec_path: &Path,
    output: &Path,
    profile: UpdateIndexSignatureProfile,
) -> Result<(), String> {
    let spec_text = fs::read_to_string(spec_path).map_err(io_error)?;
    let spec = parse_update_publish_spec(&spec_text, spec_path.parent())?;
    let mut artifacts = Vec::with_capacity(spec.artifacts.len());
    for artifact in spec.artifacts {
        let payload = fs::read(&artifact.payload_path).map_err(io_error)?;
        let bytes = payload.len() as u64;
        let digest = sha256_array(&payload);
        let mut built = match artifact.kind {
            UpdateArtifactKind::PlatformImage => UpdateArtifact::for_platform_image(
                artifact.id.as_str(),
                artifact.version.as_str(),
                artifact.source_locator.as_str(),
                bytes,
                digest,
            ),
            UpdateArtifactKind::ServiceBundle => UpdateArtifact::for_service_bundle(
                artifact
                    .package_id
                    .as_deref()
                    .unwrap_or(artifact.id.as_str()),
                artifact.version.as_str(),
                artifact.source_locator.as_str(),
                bytes,
                digest,
                artifact.reboot_required,
            ),
            UpdateArtifactKind::UserBundle => UpdateArtifact::for_user_bundle(
                artifact
                    .package_id
                    .as_deref()
                    .unwrap_or(artifact.id.as_str()),
                artifact.version.as_str(),
                artifact.source_locator.as_str(),
                bytes,
                digest,
            ),
            UpdateArtifactKind::SeedCatalog => UpdateArtifact::for_seed_catalog(
                artifact.id.as_str(),
                artifact.version.as_str(),
                artifact.source_locator.as_str(),
                bytes,
                digest,
            ),
            UpdateArtifactKind::RevocationFeed => UpdateArtifact::for_revocation_feed(
                artifact.version.as_str(),
                artifact.source_locator.as_str(),
                bytes,
                digest,
            ),
        };
        built.requested_slot = artifact.requested_slot;
        artifacts.push(built);
    }

    let index = UpdateIndex {
        channel: spec.channel,
        release: spec.release,
        published_epoch: spec.published_epoch,
        artifacts,
    };
    let bytes = build_signed_index(&index, profile).map_err(|err| err.to_string())?;
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
    }
    fs::write(output, bytes).map_err(io_error)?;
    println!("{}", output.display());
    Ok(())
}

fn cmd_update_inspect(index_path: &Path) -> Result<(), String> {
    let bytes = fs::read(index_path).map_err(io_error)?;
    let inspection = inspect_signed_index(&bytes, ech_os::boot::appliance::SlotId::SystemA)
        .map_err(|err| err.to_string())?;
    println!(
        "Update {} [{}] signer={} class={} reboot={}",
        inspection.index.release,
        inspection.index.channel,
        inspection.signature.signer_key_id,
        inspection.plan.class.as_str(),
        inspection.plan.requires_reboot
    );
    for artifact in inspection.index.artifacts {
        println!(
            "  {} {} v{} bytes={} digest={} source={}",
            artifact.kind.as_str(),
            artifact.id,
            artifact.version,
            artifact.bytes,
            hex_digest(&artifact.digest),
            artifact.source
        );
    }
    Ok(())
}

fn cmd_remove(bundle_name: &str, stage_root: &Path) -> Result<(), String> {
    let path = stage_root.join(bundle_name);
    if !path.exists() {
        return Err(String::from("staged bundle not found"));
    }
    fs::remove_file(&path).map_err(io_error)?;
    println!("Removed {}", path.display());
    Ok(())
}

fn cmd_repair(bundle: &Path, stage_root: &Path) -> Result<(), String> {
    cmd_verify(bundle)?;
    cmd_install(bundle, stage_root)?;
    println!("repair lane passed");
    Ok(())
}

fn cmd_launch(bundle: &Path) -> Result<(), String> {
    let bytes = fs::read(bundle).map_err(io_error)?;
    let inspection = inspect_signed_bundle(&bytes).map_err(|err| err.to_string())?;
    println!(
        "Launch-ready bundle: {} ({}) -> {}",
        inspection.compiled_manifest.app_id,
        inspection.source_manifest.version,
        inspection.compiled_manifest.entry
    );
    Ok(())
}

fn cmd_test(root: &Path) -> Result<(), String> {
    cmd_manifest_check(&root.join("echos-app.toml"))?;
    let bundle = cmd_package(root, None, TrustDomain::Developer)?;
    cmd_verify(&bundle)?;
    cmd_install(&bundle, &PathBuf::from("artifacts/packaged-installs"))?;
    cmd_launch(&bundle)?;
    cmd_exactness(true)?;
    println!("host manifest/package/sign/verify/install/launch/exactness lane passed");
    Ok(())
}

fn cmd_exactness(strict: bool) -> Result<(), String> {
    let snapshot = ech_os::ecosystem_exactness::snapshot();
    println!(
        "exactness: strict_ready={} win32_stubbed_exports={} known_behavior_boundaries={} runtime_counters={}",
        snapshot.strict_ready,
        snapshot.declared_win32_stub_exports,
        snapshot.known_behavior_boundaries.len(),
        snapshot.runtime_counters.len()
    );
    if !snapshot.declared_win32_stub_samples.is_empty() {
        println!(
            "win32 stub samples: {}",
            snapshot.declared_win32_stub_samples.join(", ")
        );
    }
    if !snapshot.known_behavior_boundaries.is_empty() {
        println!(
            "behavior boundaries: {}",
            snapshot.known_behavior_boundaries.join(" | ")
        );
    }
    for counter in snapshot.runtime_counters {
        println!(
            "runtime counter [{}] {} x{}",
            counter.kind.as_str(),
            counter.subject,
            counter.count
        );
    }
    if strict && !snapshot.strict_ready {
        return Err(String::from("ecosystem exactness blockers remain"));
    }
    Ok(())
}

fn cmd_field_profile_capture(output: &Path) -> Result<(), String> {
    run_powershell_script(
        Path::new("scripts/capture_physical_profile.ps1"),
        &[String::from("-OutputPath"), output.display().to_string()],
    )
}

fn cmd_field_profile_validate(profile: &Path, admission: &Path) -> Result<(), String> {
    run_powershell_script(
        Path::new("scripts/validate_physical_profile.ps1"),
        &[
            String::from("-ProfilePath"),
            profile.display().to_string(),
            String::from("-AdmissionProfilePath"),
            admission.display().to_string(),
        ],
    )
}

fn run_powershell_script(script: &Path, args: &[String]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(String::from(
            "field-profile tooling currently requires Windows PowerShell host access",
        ));
    }
    if !script.is_file() {
        return Err(format!("script not found: {}", script.display()));
    }
    let mut command = Command::new("powershell");
    command.arg("-NoProfile").arg("-File").arg(script);
    for arg in args {
        command.arg(arg);
    }
    let status = command
        .status()
        .map_err(|err| format!("failed to launch PowerShell for {}: {}", script.display(), err))?;
    if !status.success() {
        return Err(format!(
            "PowerShell script failed (exit {:?}): {}",
            status.code(),
            script.display()
        ));
    }
    Ok(())
}

fn root_arg(arg: Option<String>) -> PathBuf {
    arg.map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("cwd"))
}

fn stage_root_arg(arg: Option<String>) -> PathBuf {
    arg.map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("artifacts/packaged-installs"))
}

fn parse_trust_domain(value: &str) -> Result<TrustDomain, String> {
    TrustDomain::parse(value).map_err(|_| format!("unsupported trust domain: {}", value))
}

#[derive(Debug)]
struct UpdatePublishSpec {
    channel: String,
    release: String,
    published_epoch: u64,
    artifacts: Vec<UpdatePublishArtifactSpec>,
}

#[derive(Debug)]
struct UpdatePublishArtifactSpec {
    kind: UpdateArtifactKind,
    id: String,
    version: String,
    payload_path: PathBuf,
    source_locator: String,
    reboot_required: bool,
    package_id: Option<String>,
    requested_slot: Option<ech_os::boot::appliance::SlotId>,
}

fn parse_update_signing_profile(value: &str) -> Result<UpdateIndexSignatureProfile, String> {
    UpdateIndexSignatureProfile::parse(value)
        .ok_or_else(|| format!("unsupported update signing profile: {}", value))
}

fn parse_update_publish_spec(
    text: &str,
    base_dir: Option<&Path>,
) -> Result<UpdatePublishSpec, String> {
    let mut channel = None;
    let mut release = None;
    let mut published_epoch = None;
    let mut artifacts = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid update spec line: {}", line));
        };
        match key.trim() {
            "channel" => channel = Some(value.trim().to_string()),
            "release" => release = Some(value.trim().to_string()),
            "published_epoch" => {
                published_epoch = Some(
                    value
                        .trim()
                        .parse::<u64>()
                        .map_err(|_| String::from("published_epoch must be u64"))?,
                )
            }
            "artifact" => artifacts.push(parse_update_artifact_spec(value.trim(), base_dir)?),
            other => return Err(format!("unknown update spec key: {}", other)),
        }
    }

    let published_epoch = published_epoch.unwrap_or_else(current_unix_epoch);
    Ok(UpdatePublishSpec {
        channel: channel.ok_or_else(|| String::from("update spec missing channel"))?,
        release: release.ok_or_else(|| String::from("update spec missing release"))?,
        published_epoch,
        artifacts,
    })
}

fn parse_update_artifact_spec(
    value: &str,
    base_dir: Option<&Path>,
) -> Result<UpdatePublishArtifactSpec, String> {
    let fields = value.split('|').map(str::trim).collect::<Vec<_>>();
    if fields.len() != 8 {
        return Err(String::from(
            "artifact line must be kind|id|version|payload_path|source_locator|reboot_required|package_id_or_-|slot_or_-",
        ));
    }
    let kind = UpdateArtifactKind::parse(fields[0])
        .ok_or_else(|| format!("unsupported artifact kind: {}", fields[0]))?;
    let payload_path = match base_dir {
        Some(base) => base.join(fields[3]),
        None => PathBuf::from(fields[3]),
    };
    Ok(UpdatePublishArtifactSpec {
        kind,
        id: fields[1].to_string(),
        version: fields[2].to_string(),
        payload_path,
        source_locator: fields[4].to_string(),
        reboot_required: parse_bool(fields[5])?,
        package_id: parse_dash_option(fields[6]),
        requested_slot: parse_slot(fields[7])?,
    })
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err(format!("invalid boolean value: {}", value)),
    }
}

fn parse_dash_option(value: &str) -> Option<String> {
    if value == "-" {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_slot(value: &str) -> Result<Option<ech_os::boot::appliance::SlotId>, String> {
    match value {
        "-" => Ok(None),
        "system_a" => Ok(Some(ech_os::boot::appliance::SlotId::SystemA)),
        "system_b" => Ok(Some(ech_os::boot::appliance::SlotId::SystemB)),
        "recovery" => Ok(Some(ech_os::boot::appliance::SlotId::Recovery)),
        "none" => Ok(Some(ech_os::boot::appliance::SlotId::None)),
        _ => Err(format!("invalid slot value: {}", value)),
    }
}

fn current_unix_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn sha256_array(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn manifest_template(name: &str, template: &str) -> String {
    match template {
        "service-basic" => format!(
            "app_id = \"{0}\"\nname = \"{0}\"\nversion = \"0.1.0\"\nentry = \"target/x86_64-unknown-none/release/{0}\"\nsdk_version = 1\nruntime = \"native\"\npresentation = \"headless\"\ncapabilities = []\nstate_contract = \"stateless\"\nrestart_policy = \"bounded-retry:4\"\n",
            name
        ),
        _ => format!(
            "app_id = \"{0}\"\nname = \"{0}\"\nversion = \"0.1.0\"\nentry = \"target/x86_64-unknown-none/release/{0}\"\nsdk_version = 1\nruntime = \"native\"\npresentation = \"windowed\"\ncapabilities = [\"notifications.post\", \"clipboard.write\"]\ndefault_window.title = \"{0}\"\ndefault_window.width = 960\ndefault_window.height = 620\nstate_contract = \"cold-resume\"\nrestart_policy = \"bounded-retry:2\"\n",
            name
        ),
    }
}

fn window_template(name: &str) -> String {
    format!(
        "extern crate alloc;\n\nuse echos_sdk::{{run, Application, AppContext, Event, NotificationLevel, Scene, SceneOp, Window, WindowOptions}};\n\nstruct {1}App {{ counter: u32 }}\n\nimpl Application for {1}App {{\n    fn configure(&mut self, _ctx: &mut AppContext) -> Result<WindowOptions, echos_sdk::Error> {{\n        Ok(WindowOptions::new(\"{0}\", 960, 620))\n    }}\n\n    fn initial_scene(&mut self, _ctx: &mut AppContext) -> Result<Scene, echos_sdk::Error> {{\n        Ok(scene_for(self.counter))\n    }}\n\n    fn on_event(&mut self, ctx: &mut AppContext, _window: &mut Window, event: Event) -> Result<Option<Scene>, echos_sdk::Error> {{\n        if let Event::PointerButton {{ pressed: true, .. }} = event {{\n            self.counter = self.counter.saturating_add(1);\n            let _ = ctx.notifications().post(NotificationLevel::Info, \"{0}\", \"click received\");\n            return Ok(Some(scene_for(self.counter)));\n        }}\n        Ok(None)\n    }}\n}}\n\nfn scene_for(counter: u32) -> Scene {{\n    let mut scene = Scene::new();\n    scene.push(SceneOp::SolidRect {{ x: 0, y: 0, width: 960, height: 620, color: 0x112233ff, radius: 0, z_index: 0, opacity: 255 }});\n    scene.push(SceneOp::Text {{ x: 56, y: 72, width: 520, height: 48, color: 0xf5f7ffff, z_index: 1, opacity: 255, monospace: false, text: alloc::format!(\"{0} clicks: {{}}\", counter) }});\n    scene\n}}\n\n#[no_mangle]\npub extern \"C\" fn _start() -> ! {{\n    run({1}App {{ counter: 0 }})\n}}\n",
        name,
        pascal(name)
    )
}

fn document_template(name: &str) -> String {
    format!(
        "extern crate alloc;\n\nuse echos_sdk::{{run, Application, AppContext, Event, Scene, SceneOp, Window, WindowOptions}};\n\nstruct {1}Document;\n\nimpl Application for {1}Document {{\n    fn configure(&mut self, _ctx: &mut AppContext) -> Result<WindowOptions, echos_sdk::Error> {{\n        Ok(WindowOptions::new(\"{0}\", 1024, 720))\n    }}\n\n    fn initial_scene(&mut self, _ctx: &mut AppContext) -> Result<Scene, echos_sdk::Error> {{\n        let mut scene = Scene::new();\n        scene.push(SceneOp::SolidRect {{ x: 0, y: 0, width: 1024, height: 720, color: 0xf7f2e8ff, radius: 0, z_index: 0, opacity: 255 }});\n        scene.push(SceneOp::Text {{ x: 48, y: 56, width: 800, height: 32, color: 0x1d1d1dff, z_index: 1, opacity: 255, monospace: true, text: alloc::string::String::from(\"document-basic ready\") }});\n        Ok(scene)\n    }}\n\n    fn on_event(&mut self, ctx: &mut AppContext, _window: &mut Window, event: Event) -> Result<Option<Scene>, echos_sdk::Error> {{\n        if let Event::Key {{ pressed: true, .. }} = event {{\n            let _ = ctx.clipboard().set_text(\"echOS document-basic\");\n        }}\n        Ok(None)\n    }}\n}}\n\n#[no_mangle]\npub extern \"C\" fn _start() -> ! {{\n    run({1}Document)\n}}\n",
        name,
        pascal(name)
    )
}

fn stateful_template(name: &str) -> String {
    format!(
        "extern crate alloc;\n\nuse echos_sdk::{{run, Application, AppContext, Event, ResumeReason, RuntimeState, Scene, SceneOp, Window, WindowOptions}};\n\nstruct {1}Stateful {{ counter: u32 }}\n\nimpl Application for {1}Stateful {{\n    fn configure(&mut self, _ctx: &mut AppContext) -> Result<WindowOptions, echos_sdk::Error> {{\n        Ok(WindowOptions::new(\"{0}\", 920, 600))\n    }}\n\n    fn initial_scene(&mut self, _ctx: &mut AppContext) -> Result<Scene, echos_sdk::Error> {{\n        Ok(scene_for(self.counter))\n    }}\n\n    fn on_event(&mut self, _ctx: &mut AppContext, _window: &mut Window, event: Event) -> Result<Option<Scene>, echos_sdk::Error> {{\n        if let Event::PointerButton {{ pressed: true, .. }} = event {{\n            self.counter = self.counter.saturating_add(1);\n            return Ok(Some(scene_for(self.counter)));\n        }}\n        Ok(None)\n    }}\n\n    fn export_state(&mut self, ctx: &mut AppContext) -> Result<Option<RuntimeState>, echos_sdk::Error> {{\n        let bytes = self.counter.to_le_bytes();\n        let state = RuntimeState::inline(&bytes).map_err(|_| echos_sdk::Error::StateTooLarge)?;\n        ctx.validate_state(&state)?;\n        Ok(Some(state))\n    }}\n\n    fn import_state(&mut self, _ctx: &mut AppContext, state: &[u8]) -> Result<(), echos_sdk::Error> {{\n        if state.len() == 4 {{\n            self.counter = u32::from_le_bytes([state[0], state[1], state[2], state[3]]);\n        }}\n        Ok(())\n    }}\n\n    fn resume(&mut self, _ctx: &mut AppContext, _reason: ResumeReason) -> Result<(), echos_sdk::Error> {{\n        Ok(())\n    }}\n}}\n\nfn scene_for(counter: u32) -> Scene {{\n    let mut scene = Scene::new();\n    scene.push(SceneOp::SolidRect {{ x: 0, y: 0, width: 920, height: 600, color: 0x20343dff, radius: 0, z_index: 0, opacity: 255 }});\n    scene.push(SceneOp::Text {{ x: 48, y: 64, width: 540, height: 36, color: 0xf9f2e9ff, z_index: 1, opacity: 255, monospace: false, text: alloc::format!(\"stateful-basic generation counter = {{}}\", counter) }});\n    scene.push(SceneOp::Text {{ x: 48, y: 120, width: 680, height: 28, color: 0xd0dde2ff, z_index: 2, opacity: 255, monospace: true, text: alloc::string::String::from(\"suspend/resume export uses inline <= 1 MiB state\") }});\n    scene\n}}\n\n#[no_mangle]\npub extern \"C\" fn _start() -> ! {{\n    run({1}Stateful {{ counter: 0 }})\n}}\n",
        name,
        pascal(name)
    )
}

fn service_template(name: &str) -> String {
    format!(
        "use echos_sdk::{{run_service, ServiceApplication, ServiceContext}};\n\nstruct {0}Service;\n\nimpl ServiceApplication for {0}Service {{\n    fn bootstrap(&mut self, ctx: &mut ServiceContext) -> Result<(), echos_sdk::Error> {{\n        let _ = ctx.request_region().base;\n        let _ = ctx.response_region().base;\n        let _ = ctx.heartbeat()?;\n        Ok(())\n    }}\n\n    fn tick(&mut self, ctx: &mut ServiceContext) -> Result<(), echos_sdk::Error> {{\n        let _ = ctx.request_region().generation;\n        let _ = ctx.response_region().generation;\n        let _ = ctx.heartbeat()?;\n        Ok(())\n    }}\n}}\n\n#[no_mangle]\npub extern \"C\" fn _start() -> ! {{\n    run_service({0}Service)\n}}\n",
        pascal(name)
    )
}

fn pascal(value: &str) -> String {
    let mut out = String::new();
    for part in value.split(['-', '_']) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() {
        String::from("Native")
    } else {
        out
    }
}

fn print_help() {
    println!("echsdk <new|manifest check|build|package|sign|verify|revocation-feed|exactness [strict]|field-profile <capture|validate>|install|update <publish|inspect|stage>|remove|repair|launch|run|test>");
}

fn io_error(err: impl core::fmt::Display) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::{manifest_template, parse_update_publish_spec, service_template};
    use std::path::Path;

    #[test]
    fn service_manifest_is_headless_and_stateless() {
        let manifest = manifest_template("echo-service", "service-basic");
        assert!(manifest.contains("presentation = \"headless\""));
        assert!(manifest.contains("state_contract = \"stateless\""));
        assert!(manifest.contains("restart_policy = \"bounded-retry:4\""));
    }

    #[test]
    fn service_template_uses_pascal_service_name_and_runner() {
        let source = service_template("echo-service");
        assert!(source.contains("struct EchoServiceService;"));
        assert!(source.contains("impl ServiceApplication for EchoServiceService"));
        assert!(source.contains("run_service(EchoServiceService)"));
    }

    #[test]
    fn update_publish_spec_parses_artifacts_and_slots() {
        let spec = parse_update_publish_spec(
            "channel=engineering\nrelease=2026.04.08.1\npublished_epoch=42\nartifact=platform|echos-platform|2.1.0|payloads/platform.img|https://updates/echos-platform.img|true|-|system_b\nartifact=user|org.echos.editor|1.5.0|payloads/editor.bhd|https://updates/editor.bhd|false|org.echos.editor|-\n",
            Some(Path::new("C:/repo/specs")),
        )
        .expect("spec");
        assert_eq!(spec.channel, "engineering");
        assert_eq!(spec.release, "2026.04.08.1");
        assert_eq!(spec.artifacts.len(), 2);
        assert_eq!(
            spec.artifacts[0].requested_slot,
            Some(ech_os::boot::appliance::SlotId::SystemB)
        );
        assert_eq!(
            spec.artifacts[0].payload_path,
            Path::new("C:/repo/specs").join("payloads/platform.img")
        );
        assert_eq!(
            spec.artifacts[1].package_id.as_deref(),
            Some("org.echos.editor")
        );
    }
}
