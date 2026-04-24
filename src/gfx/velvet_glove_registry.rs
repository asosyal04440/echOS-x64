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
const HELIX_BINARY_APP_ID: u32 = 19;
const YAZI_BINARY_APP_ID: u32 = 20;
const ZELLIJ_BINARY_APP_ID: u32 = 21;
const BOTTOM_BINARY_APP_ID: u32 = 22;
const GITUI_BINARY_APP_ID: u32 = 23;
const POSTING_BINARY_APP_ID: u32 = 24;
const GLOW_BINARY_APP_ID: u32 = 25;
const RIPGREP_BINARY_APP_ID: u32 = 26;
const FD_BINARY_APP_ID: u32 = 27;
const BAT_BINARY_APP_ID: u32 = 28;

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

const HELIX_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/helix/hx.exe",
    "/downloads/hx.exe",
    "/programs/helix/hx.exe",
    "/apps/helix/hx.exe",
];

const YAZI_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/yazi/yazi.exe",
    "/downloads/yazi.exe",
    "/programs/yazi/yazi.exe",
    "/apps/yazi/yazi.exe",
];

const ZELLIJ_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/zellij/zellij.exe",
    "/downloads/zellij.exe",
    "/programs/zellij/zellij.exe",
    "/apps/zellij/zellij.exe",
];

const BOTTOM_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/bottom/btm.exe",
    "/downloads/btm.exe",
    "/programs/bottom/btm.exe",
    "/apps/bottom/btm.exe",
];

const GITUI_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/gitui/gitui.exe",
    "/downloads/gitui.exe",
    "/programs/gitui/gitui.exe",
    "/apps/gitui/gitui.exe",
];

const POSTING_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/posting/posting.exe",
    "/downloads/posting.exe",
    "/programs/posting/posting.exe",
    "/apps/posting/posting.exe",
];

const GLOW_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/glow/glow.exe",
    "/downloads/glow.exe",
    "/programs/glow/glow.exe",
    "/apps/glow/glow.exe",
];

const RIPGREP_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/ripgrep/rg.exe",
    "/downloads/rg.exe",
    "/programs/ripgrep/rg.exe",
    "/apps/ripgrep/rg.exe",
];

const FD_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/fd/fd.exe",
    "/downloads/fd.exe",
    "/programs/fd/fd.exe",
    "/apps/fd/fd.exe",
];

const BAT_BINARY_CANDIDATES: &[&str] = &[
    "/downloads/bat/bat.exe",
    "/downloads/bat.exe",
    "/programs/bat/bat.exe",
    "/apps/bat/bat.exe",
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

fn curated_tool_descriptor(
    app_id: u32,
    slug: &'static str,
    title: &'static str,
    package_id: &'static str,
) -> AppDescriptor {
    AppDescriptor::new(
        app_id,
        slug,
        title,
        LoaderDispatch::Pe,
        AbiPersonality::Win32,
        AppPresentation::ShellOwned,
        CapabilityProfile::file_worker(),
    )
    .with_package_id(package_id)
    .with_install_root(AppInstallRoot::UserApps)
    .with_trust(AppTrust::Installed)
    .with_state_contract(StateContract::WarmSuspend)
}

pub(crate) fn desktop_launch_registry() -> [PackageRecord; 19] {
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
        PackageRecord {
            aliases: &["helix", "hx", "helix editor", "code editor"],
            descriptor: curated_tool_descriptor(
                HELIX_BINARY_APP_ID,
                "helix-binary",
                "Helix",
                "org.helix.editor",
            ),
            external_candidates: HELIX_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["yazi", "file navigator", "yazi fm"],
            descriptor: curated_tool_descriptor(
                YAZI_BINARY_APP_ID,
                "yazi-binary",
                "Yazi",
                "dev.yazi.fm",
            ),
            external_candidates: YAZI_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["zellij", "terminal workspace", "multiplexer"],
            descriptor: curated_tool_descriptor(
                ZELLIJ_BINARY_APP_ID,
                "zellij-binary",
                "Zellij",
                "org.zellij.workspace",
            ),
            external_candidates: ZELLIJ_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["bottom", "btm", "system monitor"],
            descriptor: curated_tool_descriptor(
                BOTTOM_BINARY_APP_ID,
                "bottom-binary",
                "bottom",
                "org.bottom.monitor",
            ),
            external_candidates: BOTTOM_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["gitui", "git ui", "git client"],
            descriptor: curated_tool_descriptor(
                GITUI_BINARY_APP_ID,
                "gitui-binary",
                "GitUI",
                "org.gitui.client",
            ),
            external_candidates: GITUI_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["posting", "api client", "http client"],
            descriptor: curated_tool_descriptor(
                POSTING_BINARY_APP_ID,
                "posting-binary",
                "Posting",
                "dev.posting.client",
            ),
            external_candidates: POSTING_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["glow", "markdown viewer", "md viewer"],
            descriptor: curated_tool_descriptor(
                GLOW_BINARY_APP_ID,
                "glow-binary",
                "Glow",
                "org.glow.viewer",
            ),
            external_candidates: GLOW_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["ripgrep", "rg", "search code"],
            descriptor: curated_tool_descriptor(
                RIPGREP_BINARY_APP_ID,
                "ripgrep-binary",
                "ripgrep",
                "org.ripgrep.tool",
            ),
            external_candidates: RIPGREP_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["fd", "find files", "fd find"],
            descriptor: curated_tool_descriptor(FD_BINARY_APP_ID, "fd-binary", "fd", "org.fd.find"),
            external_candidates: FD_BINARY_CANDIDATES,
        },
        PackageRecord {
            aliases: &["bat", "cat viewer", "syntax cat"],
            descriptor: curated_tool_descriptor(
                BAT_BINARY_APP_ID,
                "bat-binary",
                "bat",
                "org.bat.viewer",
            ),
            external_candidates: BAT_BINARY_CANDIDATES,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::desktop_launch_registry;
    use crate::runtime_layer::package_registry_contract::RuntimePackageRegistry;

    #[test]
    fn curated_tool_registry_resolves_helix_candidate_path() {
        let registry = desktop_launch_registry();
        let resolution = RuntimePackageRegistry::new(&registry)
            .resolve_with_probe("helix", |path| path == "/apps/helix/hx.exe")
            .expect("helix resolution");
        assert_eq!(resolution.descriptor().title, "Helix");
        assert_eq!(resolution.path(), Some("/apps/helix/hx.exe"));
    }

    #[test]
    fn curated_tool_registry_reports_missing_bottom_candidates() {
        let registry = desktop_launch_registry();
        let resolution = RuntimePackageRegistry::new(&registry)
            .resolve_with_probe("bottom", |_| false)
            .expect("bottom resolution");
        let candidates = resolution
            .missing_candidates()
            .expect("missing candidate set");
        assert!(candidates.iter().any(|path| path.contains("btm.exe")));
    }
}
