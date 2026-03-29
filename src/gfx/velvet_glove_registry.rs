use crate::gui::launch_pipeline::{
    AbiPersonality, AppDescriptor, AppInstallRoot, AppPresentation, AppTrust, CapabilityProfile,
    LoaderDispatch, PackageRecord, StateContract,
};

const TERMINAL_APP_ID: u32 = 10;
const FILES_APP_ID: u32 = 11;
const SETTINGS_APP_ID: u32 = 12;
const EDITOR_APP_ID: u32 = 13;
const BROWSER_APP_ID: u32 = 14;
const RECYCLE_SHORTCUT_APP_ID: u32 = 15;
const FIREFOX_BINARY_APP_ID: u32 = 16;
const CHROMIUM_BINARY_APP_ID: u32 = 17;
const CEF_BINARY_APP_ID: u32 = 18;

const FIREFOX_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/firefox/firefox.exe",
    "/downloads/firefox.exe",
    "/programs/firefox/firefox.exe",
    "/apps/firefox/firefox.exe",
];

const CHROMIUM_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/chromium/chrome.exe",
    "/downloads/chromium/chromium.exe",
    "/downloads/chrome.exe",
    "/programs/chromium/chrome.exe",
    "/apps/chromium/chrome.exe",
];

pub(crate) const CEF_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/cef/cefclient.exe",
    "/downloads/cefclient.exe",
    "/programs/cef/cefclient.exe",
    "/apps/cef/cefclient.exe",
];

fn native_window_descriptor(
    app_id: u32,
    slug: &'static str,
    title: &'static str,
    capabilities: CapabilityProfile,
) -> AppDescriptor {
    AppDescriptor::new(
        app_id,
        slug,
        title,
        LoaderDispatch::Native,
        AbiPersonality::Native,
        AppPresentation::Windowed,
        capabilities,
    )
    .with_install_root(AppInstallRoot::SystemApps)
    .with_trust(AppTrust::Platform)
}

fn terminal_descriptor() -> AppDescriptor {
    native_window_descriptor(
        TERMINAL_APP_ID,
        "Terminal",
        "Terminal",
        CapabilityProfile::shell_defaults(),
    )
    .with_package_id("echos.terminal")
    .with_state_contract(StateContract::WarmSuspend)
}

fn files_descriptor() -> AppDescriptor {
    native_window_descriptor(
        FILES_APP_ID,
        "Files",
        "Files",
        CapabilityProfile::file_worker(),
    )
    .with_package_id("echos.files")
    .with_state_contract(StateContract::WarmSuspend)
}

fn settings_descriptor() -> AppDescriptor {
    native_window_descriptor(
        SETTINGS_APP_ID,
        "Settings",
        "Settings",
        CapabilityProfile::shell_defaults(),
    )
    .with_package_id("echos.settings")
    .with_state_contract(StateContract::WarmSuspend)
}

fn editor_descriptor() -> AppDescriptor {
    native_window_descriptor(
        EDITOR_APP_ID,
        "Editor",
        "Editor",
        CapabilityProfile::file_worker(),
    )
    .with_package_id("echos.editor")
    .with_file_associations(&[".txt", ".md", ".log"])
    .with_state_contract(StateContract::ColdResume)
}

fn web_descriptor() -> AppDescriptor {
    native_window_descriptor(
        BROWSER_APP_ID,
        "Web",
        "Web",
        CapabilityProfile::file_worker(),
    )
    .with_package_id("echos.web")
    .with_file_associations(&[".html", ".htm", ".url"])
    .with_state_contract(StateContract::WarmSuspend)
}

fn recycle_bin_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        RECYCLE_SHORTCUT_APP_ID,
        "recycle-bin",
        "Recycle Bin",
        LoaderDispatch::Native,
        AbiPersonality::Native,
        AppPresentation::SpecialAction,
        CapabilityProfile::shell_defaults(),
    )
    .with_package_id("echos.recycle-bin")
    .with_install_root(AppInstallRoot::SystemApps)
    .with_trust(AppTrust::Platform)
}

fn browser_binary_descriptor(
    app_id: u32,
    slug: &'static str,
    title: &'static str,
) -> AppDescriptor {
    AppDescriptor::new(
        app_id,
        slug,
        title,
        LoaderDispatch::Pe,
        AbiPersonality::Win32,
        AppPresentation::ShellOwned,
        CapabilityProfile::shell_defaults(),
    )
    .with_package_id(match slug {
        "firefox-binary" => "org.mozilla.firefox",
        "chromium-binary" => "org.chromium.browser",
        "cef-binary" => "org.cef.browser",
        _ => slug,
    })
    .with_install_root(AppInstallRoot::UserApps)
    .with_trust(AppTrust::Installed)
    .with_state_contract(StateContract::WarmSuspend)
}

pub(crate) fn desktop_launch_registry() -> [PackageRecord; 9] {
    [
        PackageRecord {
            aliases: &["terminal", "shell", "console"],
            descriptor: terminal_descriptor(),
            external_candidates: &[],
        },
        PackageRecord {
            aliases: &["files", "file manager", "explorer"],
            descriptor: files_descriptor(),
            external_candidates: &[],
        },
        PackageRecord {
            aliases: &["settings", "preferences", "control"],
            descriptor: settings_descriptor(),
            external_candidates: &[],
        },
        PackageRecord {
            aliases: &["editor", "notes", "edit"],
            descriptor: editor_descriptor(),
            external_candidates: &[],
        },
        PackageRecord {
            aliases: &["web", "browser", "internet"],
            descriptor: web_descriptor(),
            external_candidates: &[],
        },
        PackageRecord {
            aliases: &["recycle", "recycle bin", "trash"],
            descriptor: recycle_bin_descriptor(),
            external_candidates: &[],
        },
        PackageRecord {
            aliases: &["firefox", "mozilla firefox", "browser-firefox"],
            descriptor: browser_binary_descriptor(
                FIREFOX_BINARY_APP_ID,
                "firefox-binary",
                "Firefox",
            ),
            external_candidates: FIREFOX_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["chromium", "chrome", "google chrome", "browser-chromium"],
            descriptor: browser_binary_descriptor(
                CHROMIUM_BINARY_APP_ID,
                "chromium-binary",
                "Chromium",
            ),
            external_candidates: CHROMIUM_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["cef", "cefclient", "cef browser"],
            descriptor: browser_binary_descriptor(CEF_BINARY_APP_ID, "cef-binary", "CEF Browser"),
            external_candidates: CEF_BINARY_CANDIDATES,
        },
    ]
}
