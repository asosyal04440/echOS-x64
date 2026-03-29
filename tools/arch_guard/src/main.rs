use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::{self, Visit};
use syn::{File, Item, ItemUse, TypePath};
use walkdir::WalkDir;

#[derive(Debug, Clone, Deserialize)]
struct GuardConfig {
    current_wave: u32,
    namespaces: Vec<NamespaceRule>,
    #[serde(default)]
    managed_files: Vec<ManagedFileRule>,
    #[serde(default)]
    forbidden_edges: Vec<EdgeRule>,
    #[serde(default)]
    surface_rules: Vec<SurfaceRule>,
    #[serde(default)]
    temporary_exceptions: Vec<ExceptionRule>,
    #[serde(default)]
    ratchet_facade_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct NamespaceRule {
    name: String,
    root: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManagedFileRule {
    namespace: String,
    path: String,
    #[serde(default)]
    forbid_legacy_prefixes: Vec<String>,
    #[serde(default = "default_true")]
    track_in_baseline: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct EdgeRule {
    from: String,
    to: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SurfaceRule {
    from: String,
    to: String,
    allow: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExceptionRule {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    file_suffix: Option<String>,
    path_prefix: String,
    #[serde(default)]
    expires_wave: Option<u32>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReferenceKind {
    DirectImport,
    GlobImport,
    ExprPath,
    TypePath,
    MacroEscape,
}

impl ReferenceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::DirectImport => "direct_import",
            Self::GlobImport => "glob_import",
            Self::ExprPath => "expr_path",
            Self::TypePath => "type_path",
            Self::MacroEscape => "macro_escape",
        }
    }
}

#[derive(Debug, Clone)]
struct Reference {
    kind: ReferenceKind,
    path: String,
}

#[derive(Debug, Clone)]
struct DebtTag {
    id: String,
    wave: u32,
    owner: String,
    reason: String,
}

#[derive(Debug, Clone)]
struct SourceFile {
    path: PathBuf,
    namespace: String,
    references: Vec<Reference>,
    debts: Vec<DebtTag>,
    is_facade: bool,
    forbidden_legacy_prefixes: Vec<String>,
    track_in_baseline: bool,
}

#[derive(Debug, Clone)]
struct Violation {
    file: PathBuf,
    namespace: String,
    kind: String,
    path: String,
    detail: String,
}

#[derive(Debug, Default)]
struct AnalysisResult {
    files: Vec<SourceFile>,
    violations: Vec<Violation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BaselineSnapshot {
    legacy_references: usize,
    legacy_references_by_namespace: BTreeMap<String, usize>,
    legacy_references_by_file: BTreeMap<String, usize>,
    facade_references_by_prefix: BTreeMap<String, usize>,
    facade_references_by_file: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default)]
struct MetricSummary {
    legacy_references: usize,
    legacy_references_by_namespace: BTreeMap<String, usize>,
    legacy_references_by_file: BTreeMap<String, usize>,
    facade_references_by_prefix: BTreeMap<String, usize>,
    facade_references_by_file: BTreeMap<String, usize>,
}

fn default_true() -> bool {
    true
}

fn main() {
    match run() {
        Ok(exit_code) => std::process::exit(exit_code),
        Err(err) => {
            eprintln!("arch_guard: {err}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<i32, String> {
    let args = parse_args(env::args().skip(1).collect())?;
    let repo_root = env::current_dir().map_err(|err| err.to_string())?;
    let config_path = repo_root
        .join("docs")
        .join("architecture")
        .join("arch_rules.toml");
    let baseline_path = repo_root
        .join("docs")
        .join("architecture")
        .join("arch_baseline.json");
    let config = load_config(&config_path)?;
    let mut analysis = analyze_repo(&repo_root, &config)?;

    if args.refresh_baseline {
        let summary = summarize_tracked_metrics(&analysis, &config);
        write_baseline(&baseline_path, &summary)?;
        if args.report {
            println!("arch_guard baseline refreshed: {}", baseline_path.display());
        }
    }

    if args.check {
        let baseline = load_baseline(&baseline_path)?;
        apply_baseline_ratchet(&mut analysis, &config, &baseline);
    }

    let report = render_report(&analysis, &config);

    if args.report {
        println!("{report}");
    }
    if args.check && !analysis.violations.is_empty() {
        if !args.report {
            println!("{report}");
        }
        return Ok(1);
    }
    Ok(0)
}

struct CliArgs {
    check: bool,
    report: bool,
    refresh_baseline: bool,
}

fn parse_args(args: Vec<String>) -> Result<CliArgs, String> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!(
            "Usage: cargo run -p arch_guard --target x86_64-pc-windows-msvc -- [--check] [--report] [--refresh-baseline]"
        );
        return Ok(CliArgs {
            check: false,
            report: false,
            refresh_baseline: false,
        });
    }

    let mut check = false;
    let mut report = false;
    let mut refresh_baseline = false;
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--report" => report = true,
            "--refresh-baseline" => refresh_baseline = true,
            other => return Err(format!("unsupported argument '{other}'")),
        }
    }
    if !check && !report && !refresh_baseline {
        report = true;
    }
    Ok(CliArgs {
        check,
        report,
        refresh_baseline,
    })
}

fn load_config(path: &Path) -> Result<GuardConfig, String> {
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("failed to read config '{}': {err}", path.display()))?;
    toml::from_str(&raw)
        .map_err(|err| format!("failed to parse config '{}': {err}", path.display()))
}

fn analyze_repo(repo_root: &Path, config: &GuardConfig) -> Result<AnalysisResult, String> {
    let mut result = AnalysisResult::default();
    let mut files_by_path = BTreeMap::<String, SourceFile>::new();
    for namespace in &config.namespaces {
        let root = repo_root.join(&namespace.root);
        if !root.exists() {
            result.violations.push(Violation {
                file: root.clone(),
                namespace: namespace.name.clone(),
                kind: String::from("missing_namespace_root"),
                path: namespace.root.clone(),
                detail: String::from("configured namespace root is missing"),
            });
            continue;
        }
        for entry in WalkDir::new(&root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("rs"))
        {
            let source = parse_source_file(entry.path(), namespace, config, Vec::new(), false)?;
            merge_source_file(&mut files_by_path, source);
        }
    }
    for managed in &config.managed_files {
        let path = repo_root.join(&managed.path);
        if !path.exists() {
            result.violations.push(Violation {
                file: path.clone(),
                namespace: managed.namespace.clone(),
                kind: String::from("missing_managed_file"),
                path: managed.path.clone(),
                detail: String::from("configured managed file is missing"),
            });
            continue;
        }
        let namespace = NamespaceRule {
            name: managed.namespace.clone(),
            root: managed.path.clone(),
        };
        let source = parse_source_file(
            &path,
            &namespace,
            config,
            managed.forbid_legacy_prefixes.clone(),
            managed.track_in_baseline,
        )?;
        merge_source_file(&mut files_by_path, source);
    }
    for source in files_by_path.into_values() {
        evaluate_source_file(&source, config, &mut result);
        result.files.push(source);
    }
    Ok(result)
}

fn parse_source_file(
    path: &Path,
    namespace: &NamespaceRule,
    config: &GuardConfig,
    forbidden_legacy_prefixes: Vec<String>,
    track_in_baseline: bool,
) -> Result<SourceFile, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read '{}': {err}", path.display()))?;
    let syntax = syn::parse_file(&text)
        .map_err(|err| format!("failed to parse '{}': {err}", path.display()))?;
    let mut collector = ReferenceCollector::default();
    collector.visit_file(&syntax);
    let debts = parse_debt_tags(&text)?;
    let is_facade = is_facade_file(&syntax);

    if is_facade && debts.is_empty() {
        return Ok(SourceFile {
            path: path.to_path_buf(),
            namespace: namespace.name.clone(),
            references: collector.references,
            debts,
            is_facade,
            forbidden_legacy_prefixes,
            track_in_baseline,
        });
    }

    let mut references = collector.references;
    references.sort_by(|a, b| a.path.cmp(&b.path).then(a.kind.cmp(&b.kind)));
    references.dedup_by(|a, b| a.kind == b.kind && a.path == b.path);

    validate_debts(&debts, namespace, config, path)?;

    Ok(SourceFile {
        path: path.to_path_buf(),
        namespace: namespace.name.clone(),
        references,
        debts,
        is_facade,
        forbidden_legacy_prefixes,
        track_in_baseline,
    })
}

fn merge_source_file(files_by_path: &mut BTreeMap<String, SourceFile>, source: SourceFile) {
    let key = normalize_path_string(source.path.to_string_lossy().as_ref());
    if let Some(existing) = files_by_path.get_mut(&key) {
        for reference in source.references {
            existing.references.push(reference);
        }
        existing
            .references
            .sort_by(|a, b| a.path.cmp(&b.path).then(a.kind.cmp(&b.kind)));
        existing
            .references
            .dedup_by(|a, b| a.kind == b.kind && a.path == b.path);

        for debt in source.debts {
            if !existing.debts.iter().any(|current| current.id == debt.id) {
                existing.debts.push(debt);
            }
        }
        existing.is_facade |= source.is_facade;
        existing.track_in_baseline |= source.track_in_baseline;
        for prefix in source.forbidden_legacy_prefixes {
            if !existing.forbidden_legacy_prefixes.contains(&prefix) {
                existing.forbidden_legacy_prefixes.push(prefix);
            }
        }
        return;
    }
    files_by_path.insert(key, source);
}

fn load_baseline(path: &Path) -> Result<BaselineSnapshot, String> {
    let raw = fs::read_to_string(path).map_err(|err| {
        format!(
            "failed to read baseline '{}': {}. run --refresh-baseline first",
            path.display(),
            err
        )
    })?;
    serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse baseline '{}': {err}", path.display()))
}

fn write_baseline(path: &Path, summary: &MetricSummary) -> Result<(), String> {
    let snapshot = BaselineSnapshot {
        legacy_references: summary.legacy_references,
        legacy_references_by_namespace: summary.legacy_references_by_namespace.clone(),
        legacy_references_by_file: summary.legacy_references_by_file.clone(),
        facade_references_by_prefix: summary.facade_references_by_prefix.clone(),
        facade_references_by_file: summary.facade_references_by_file.clone(),
    };
    let raw = serde_json::to_string_pretty(&snapshot).map_err(|err| err.to_string())?;
    fs::write(path, raw)
        .map_err(|err| format!("failed to write baseline '{}': {err}", path.display()))
}

fn summarize_metrics(result: &AnalysisResult, config: &GuardConfig) -> MetricSummary {
    summarize_metrics_filtered(result, config, |_| true)
}

fn summarize_tracked_metrics(result: &AnalysisResult, config: &GuardConfig) -> MetricSummary {
    summarize_metrics_filtered(result, config, |source| source.track_in_baseline)
}

fn summarize_metrics_filtered<F>(
    result: &AnalysisResult,
    config: &GuardConfig,
    include: F,
) -> MetricSummary
where
    F: Fn(&SourceFile) -> bool,
{
    let mut summary = MetricSummary::default();
    for source in &result.files {
        if !include(source) {
            continue;
        }
        let normalized_file = normalize_path_string(source.path.to_string_lossy().as_ref());
        let legacy_count = source
            .references
            .iter()
            .filter(|reference| is_legacy_reference(&reference.path, config))
            .count();
        if legacy_count != 0 {
            summary.legacy_references += legacy_count;
            *summary
                .legacy_references_by_namespace
                .entry(source.namespace.clone())
                .or_default() += legacy_count;
            *summary
                .legacy_references_by_file
                .entry(normalized_file.clone())
                .or_default() += legacy_count;
        }
        for prefix in &config.ratchet_facade_prefixes {
            let count = source
                .references
                .iter()
                .filter(|reference| {
                    reference.path == *prefix || reference.path.starts_with(&format!("{prefix}::"))
                })
                .count();
            if count == 0 {
                continue;
            }
            *summary
                .facade_references_by_prefix
                .entry(prefix.clone())
                .or_default() += count;
            *summary
                .facade_references_by_file
                .entry(format!("{normalized_file}::{prefix}"))
                .or_default() += count;
        }
    }
    summary
}

fn apply_baseline_ratchet(
    result: &mut AnalysisResult,
    config: &GuardConfig,
    baseline: &BaselineSnapshot,
) {
    let summary = summarize_tracked_metrics(result, config);

    push_baseline_violation(
        result,
        "baseline_legacy_total_regression",
        PathBuf::from("docs/architecture/arch_baseline.json"),
        String::from("baseline"),
        String::from("legacy_references"),
        baseline.legacy_references,
        summary.legacy_references,
    );

    compare_count_maps(
        result,
        "baseline_legacy_namespace_regression",
        "legacy namespace",
        &baseline.legacy_references_by_namespace,
        &summary.legacy_references_by_namespace,
    );
    compare_count_maps(
        result,
        "baseline_legacy_file_regression",
        "legacy file",
        &baseline.legacy_references_by_file,
        &summary.legacy_references_by_file,
    );
    compare_count_maps(
        result,
        "baseline_facade_prefix_regression",
        "facade prefix",
        &baseline.facade_references_by_prefix,
        &summary.facade_references_by_prefix,
    );
    compare_count_maps(
        result,
        "baseline_facade_file_regression",
        "facade file",
        &baseline.facade_references_by_file,
        &summary.facade_references_by_file,
    );
}

fn compare_count_maps(
    result: &mut AnalysisResult,
    kind: &str,
    subject: &str,
    baseline: &BTreeMap<String, usize>,
    current: &BTreeMap<String, usize>,
) {
    let mut keys = BTreeSet::new();
    keys.extend(baseline.keys().cloned());
    keys.extend(current.keys().cloned());
    for key in keys {
        let baseline_count = baseline.get(&key).copied().unwrap_or(0);
        let current_count = current.get(&key).copied().unwrap_or(0);
        if current_count > baseline_count {
            push_baseline_violation(
                result,
                kind,
                PathBuf::from("docs/architecture/arch_baseline.json"),
                String::from("baseline"),
                format!("{subject}::{key}"),
                baseline_count,
                current_count,
            );
        }
    }
}

fn push_baseline_violation(
    result: &mut AnalysisResult,
    kind: &str,
    file: PathBuf,
    namespace: String,
    path: String,
    baseline_count: usize,
    current_count: usize,
) {
    if current_count <= baseline_count {
        return;
    }
    result.violations.push(Violation {
        file,
        namespace,
        kind: kind.to_string(),
        path,
        detail: format!(
            "baseline ratchet forbids growth: baseline={} current={}",
            baseline_count, current_count
        ),
    });
}

fn validate_debts(
    debts: &[DebtTag],
    namespace: &NamespaceRule,
    config: &GuardConfig,
    path: &Path,
) -> Result<(), String> {
    for debt in debts {
        if debt.owner != namespace.name {
            return Err(format!(
                "invalid debt owner in '{}': expected owner '{}', got '{}'",
                path.display(),
                namespace.name,
                debt.owner
            ));
        }
        if debt.reason.trim().is_empty() {
            return Err(format!(
                "invalid debt tag in '{}': empty reason",
                path.display()
            ));
        }
        if debt.wave < config.current_wave
            && !config.temporary_exceptions.iter().any(|exception| {
                exception.kind.as_deref() == Some("debt_renewal")
                    && exception.from.as_deref() == Some(namespace.name.as_str())
                    && exception
                        .file_suffix
                        .as_deref()
                        .map(|suffix| path_suffix_matches(path, suffix))
                        .unwrap_or(false)
                    && exception.path_prefix == debt.id
                    && exception
                        .expires_wave
                        .map(|wave| wave >= config.current_wave)
                        .unwrap_or(false)
            })
        {
            return Err(format!(
                "expired debt '{}' in '{}': wave {} < current wave {}",
                debt.id,
                path.display(),
                debt.wave,
                config.current_wave
            ));
        }
    }
    Ok(())
}

fn evaluate_source_file(source: &SourceFile, config: &GuardConfig, result: &mut AnalysisResult) {
    if source.is_facade && source.debts.is_empty() {
        result.violations.push(Violation {
            file: source.path.clone(),
            namespace: source.namespace.clone(),
            kind: String::from("facade_missing_debt"),
            path: source.path.display().to_string(),
            detail: String::from("namespace facade requires at least one ARCH_DEBT tag"),
        });
    }

    for reference in &source.references {
        if source
            .forbidden_legacy_prefixes
            .iter()
            .any(|prefix| reference.path.starts_with(prefix))
        {
            result.violations.push(Violation {
                file: source.path.clone(),
                namespace: source.namespace.clone(),
                kind: String::from("legacy_path_violation"),
                path: reference.path.clone(),
                detail: format!(
                    "managed file forbids legacy path prefix [{}]",
                    source.forbidden_legacy_prefixes.join(", ")
                ),
            });
            continue;
        }
        let Some(target_namespace) = target_namespace(&reference.path, config) else {
            continue;
        };
        if target_namespace == source.namespace {
            continue;
        }
        if is_exception(source, reference, config) {
            continue;
        }
        if config
            .forbidden_edges
            .iter()
            .any(|rule| rule.from == source.namespace && rule.to == target_namespace)
        {
            result.violations.push(Violation {
                file: source.path.clone(),
                namespace: source.namespace.clone(),
                kind: String::from("forbidden_edge"),
                path: reference.path.clone(),
                detail: format!(
                    "forbidden edge {} -> {} via {}",
                    source.namespace,
                    target_namespace,
                    reference.kind.as_str()
                ),
            });
            continue;
        }
        if let Some(rule) = config
            .surface_rules
            .iter()
            .find(|rule| rule.from == source.namespace && rule.to == target_namespace)
        {
            let suffix = reference
                .path
                .strip_prefix(&format!("crate::{}::", rule.to))
                .unwrap_or("");
            let allowed = rule
                .allow
                .iter()
                .any(|prefix| suffix == *prefix || suffix.starts_with(&format!("{prefix}::")));
            if !allowed {
                result.violations.push(Violation {
                    file: source.path.clone(),
                    namespace: source.namespace.clone(),
                    kind: String::from("surface_violation"),
                    path: reference.path.clone(),
                    detail: format!(
                        "surface {} -> {} allows only [{}]",
                        source.namespace,
                        target_namespace,
                        rule.allow.join(", ")
                    ),
                });
            }
        }
    }
}

fn is_exception(source: &SourceFile, reference: &Reference, config: &GuardConfig) -> bool {
    config.temporary_exceptions.iter().any(|exception| {
        if let Some(kind) = &exception.kind {
            if kind != reference.kind.as_str() && kind != "any" {
                return false;
            }
        }
        if let Some(from) = &exception.from {
            if from != &source.namespace {
                return false;
            }
        }
        if let Some(suffix) = &exception.file_suffix {
            if !path_suffix_matches(&source.path, suffix) {
                return false;
            }
        }
        if !reference.path.starts_with(&exception.path_prefix) {
            return false;
        }
        exception
            .expires_wave
            .map(|wave| wave >= config.current_wave)
            .unwrap_or(true)
    })
}

fn path_suffix_matches(path: &Path, suffix: &str) -> bool {
    normalize_path_string(path.to_string_lossy().as_ref())
        .ends_with(normalize_path_string(suffix).as_str())
}

fn normalize_path_string(path: &str) -> String {
    path.replace('\\', "/")
}

fn target_namespace(path: &str, config: &GuardConfig) -> Option<String> {
    let mut segments = path.split("::");
    if segments.next()? != "crate" {
        return None;
    }
    let first = segments.next()?;
    config
        .namespaces
        .iter()
        .find(|namespace| namespace.name == first)
        .map(|namespace| namespace.name.clone())
}

fn is_legacy_reference(path: &str, config: &GuardConfig) -> bool {
    let mut segments = path.split("::");
    if segments.next() != Some("crate") {
        return false;
    }
    let Some(first) = segments.next() else {
        return false;
    };
    !config
        .namespaces
        .iter()
        .any(|namespace| namespace.name == first)
}

fn render_report(result: &AnalysisResult, config: &GuardConfig) -> String {
    let mut output = String::new();
    let summary = summarize_metrics(result, config);
    let mut by_namespace = BTreeMap::<String, usize>::new();
    let mut by_file = BTreeMap::<String, usize>::new();
    let mut by_kind = BTreeMap::<String, usize>::new();
    let mut debt_by_namespace = BTreeMap::<String, usize>::new();
    let mut active_debt = 0usize;
    let mut new_debt = 0usize;
    let mut renewed_debt = 0usize;
    let mut expired_debt = 0usize;
    let mut active_exceptions = 0usize;

    for source in &result.files {
        if !source.debts.is_empty() {
            active_debt += source.debts.len();
            *debt_by_namespace
                .entry(source.namespace.clone())
                .or_default() += source.debts.len();
            for debt in &source.debts {
                if debt.wave == config.current_wave {
                    new_debt += 1;
                }
                if debt.wave < config.current_wave {
                    if debt_has_active_renewal(debt, source, config) {
                        renewed_debt += 1;
                    } else {
                        expired_debt += 1;
                    }
                }
            }
        }
    }

    for violation in &result.violations {
        *by_namespace.entry(violation.namespace.clone()).or_default() += 1;
        *by_file
            .entry(violation.file.display().to_string())
            .or_default() += 1;
        *by_kind.entry(violation.kind.clone()).or_default() += 1;
    }
    for exception in &config.temporary_exceptions {
        let not_expired = exception
            .expires_wave
            .map(|wave| wave >= config.current_wave)
            .unwrap_or(true);
        if not_expired {
            active_exceptions += 1;
        }
    }

    writeln!(&mut output, "arch_guard report").unwrap();
    writeln!(&mut output, "current_wave = {}", config.current_wave).unwrap();
    writeln!(&mut output, "managed_files = {}", result.files.len()).unwrap();
    writeln!(&mut output, "violations = {}", result.violations.len()).unwrap();
    writeln!(&mut output, "active_debt = {}", active_debt).unwrap();
    writeln!(&mut output, "new_debt = {}", new_debt).unwrap();
    writeln!(&mut output, "renewed_debt = {}", renewed_debt).unwrap();
    writeln!(&mut output, "expired_debt = {}", expired_debt).unwrap();
    writeln!(
        &mut output,
        "legacy_references = {}",
        summary.legacy_references
    )
    .unwrap();
    writeln!(&mut output, "active_exceptions = {}", active_exceptions).unwrap();

    output.push_str("\nviolations_by_kind:\n");
    for (kind, count) in &by_kind {
        writeln!(&mut output, "  {kind}: {count}").unwrap();
    }
    output.push_str("\nviolations_by_namespace:\n");
    for (namespace, count) in &by_namespace {
        writeln!(&mut output, "  {namespace}: {count}").unwrap();
    }
    output.push_str("\ndebt_by_namespace:\n");
    for (namespace, count) in &debt_by_namespace {
        writeln!(&mut output, "  {namespace}: {count}").unwrap();
    }
    output.push_str("\nlegacy_references_by_namespace:\n");
    for (namespace, count) in &summary.legacy_references_by_namespace {
        writeln!(&mut output, "  {namespace}: {count}").unwrap();
    }
    output.push_str("\nlegacy_references_by_file:\n");
    for (file, count) in &summary.legacy_references_by_file {
        writeln!(&mut output, "  {file}: {count}").unwrap();
    }
    output.push_str("\nfacade_references_by_prefix:\n");
    for (prefix, count) in &summary.facade_references_by_prefix {
        writeln!(&mut output, "  {prefix}: {count}").unwrap();
    }
    output.push_str("\nfacade_references_by_file:\n");
    for (file, count) in &summary.facade_references_by_file {
        writeln!(&mut output, "  {file}: {count}").unwrap();
    }
    output.push_str("\nviolations_by_file:\n");
    for (file, count) in &by_file {
        writeln!(&mut output, "  {file}: {count}").unwrap();
    }
    output.push_str("\nactive_exceptions:\n");
    for exception in &config.temporary_exceptions {
        let not_expired = exception
            .expires_wave
            .map(|wave| wave >= config.current_wave)
            .unwrap_or(true);
        if !not_expired {
            continue;
        }
        writeln!(
            &mut output,
            "  kind={} from={} file_suffix={} path_prefix={} expires_wave={} reason={}",
            exception.kind.as_deref().unwrap_or("any"),
            exception.from.as_deref().unwrap_or("*"),
            exception.file_suffix.as_deref().unwrap_or("*"),
            exception.path_prefix,
            exception
                .expires_wave
                .map(|wave| wave.to_string())
                .unwrap_or_else(|| String::from("*")),
            exception.reason.as_deref().unwrap_or("")
        )
        .unwrap();
    }
    output.push_str("\nviolation_details:\n");
    for violation in &result.violations {
        writeln!(
            &mut output,
            "  [{}] {} :: {} :: {}",
            violation.kind,
            violation.file.display(),
            violation.path,
            violation.detail
        )
        .unwrap();
    }

    output
}

fn debt_has_active_renewal(debt: &DebtTag, source: &SourceFile, config: &GuardConfig) -> bool {
    config.temporary_exceptions.iter().any(|exception| {
        exception.kind.as_deref() == Some("debt_renewal")
            && exception.from.as_deref() == Some(source.namespace.as_str())
            && exception.path_prefix == debt.id
            && exception
                .file_suffix
                .as_deref()
                .map(|suffix| path_suffix_matches(&source.path, suffix))
                .unwrap_or(false)
            && exception
                .expires_wave
                .map(|wave| wave >= config.current_wave)
                .unwrap_or(false)
    })
}

#[derive(Default)]
struct ReferenceCollector {
    references: Vec<Reference>,
}

impl<'ast> Visit<'ast> for ReferenceCollector {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        flatten_use_tree(Vec::new(), &node.tree, &mut self.references);
        visit::visit_item_use(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if let Some(path) = normalize_syn_path(&node.path) {
            self.references.push(Reference {
                kind: ReferenceKind::ExprPath,
                path,
            });
        }
        visit::visit_expr_path(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast TypePath) {
        if let Some(path) = normalize_syn_path(&node.path) {
            self.references.push(Reference {
                kind: ReferenceKind::TypePath,
                path,
            });
        }
        visit::visit_type_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        for path in scan_macro_tokens(node.tokens.clone()) {
            self.references.push(Reference {
                kind: ReferenceKind::MacroEscape,
                path,
            });
        }
        visit::visit_macro(self, node);
    }
}

fn flatten_use_tree(prefix: Vec<String>, tree: &syn::UseTree, out: &mut Vec<Reference>) {
    match tree {
        syn::UseTree::Path(node) => {
            let mut next = prefix;
            next.push(node.ident.to_string());
            flatten_use_tree(next, &node.tree, out);
        }
        syn::UseTree::Name(node) => {
            let mut path = prefix;
            path.push(node.ident.to_string());
            push_use_reference(path, ReferenceKind::DirectImport, out);
        }
        syn::UseTree::Rename(node) => {
            let mut path = prefix;
            path.push(node.ident.to_string());
            push_use_reference(path, ReferenceKind::DirectImport, out);
        }
        syn::UseTree::Glob(_) => {
            push_use_reference(prefix, ReferenceKind::GlobImport, out);
        }
        syn::UseTree::Group(node) => {
            for item in &node.items {
                flatten_use_tree(prefix.clone(), item, out);
            }
        }
    }
}

fn push_use_reference(path: Vec<String>, kind: ReferenceKind, out: &mut Vec<Reference>) {
    if path.first().map(|segment| segment.as_str()) != Some("crate") {
        return;
    }
    out.push(Reference {
        kind,
        path: path.join("::"),
    });
}

fn normalize_syn_path(path: &syn::Path) -> Option<String> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    if segments.first().map(|segment| segment.as_str()) != Some("crate") {
        return None;
    }
    Some(segments.join("::"))
}

fn scan_macro_tokens(tokens: proc_macro2::TokenStream) -> Vec<String> {
    let rendered = tokens.to_string();
    let chars: Vec<char> = rendered.chars().collect();
    let mut found = BTreeSet::new();
    let mut idx = 0usize;
    while idx < chars.len() {
        if starts_with_word(&chars, idx, "crate") {
            let mut cursor = idx + 5;
            consume_ws(&chars, &mut cursor);
            if !consume_colons(&chars, &mut cursor) {
                idx += 1;
                continue;
            }
            let mut segments = vec![String::from("crate")];
            loop {
                consume_ws(&chars, &mut cursor);
                let Some(ident) = consume_ident(&chars, &mut cursor) else {
                    break;
                };
                segments.push(ident);
                let checkpoint = cursor;
                consume_ws(&chars, &mut cursor);
                if !consume_colons(&chars, &mut cursor) {
                    cursor = checkpoint;
                    break;
                }
            }
            if segments.len() > 1 {
                found.insert(segments.join("::"));
            }
            idx = cursor;
            continue;
        }
        idx += 1;
    }
    found.into_iter().collect()
}

fn starts_with_word(chars: &[char], idx: usize, word: &str) -> bool {
    let word_chars: Vec<char> = word.chars().collect();
    if idx + word_chars.len() > chars.len() {
        return false;
    }
    chars[idx..idx + word_chars.len()] == word_chars[..]
}

fn consume_ws(chars: &[char], cursor: &mut usize) {
    while *cursor < chars.len() && chars[*cursor].is_whitespace() {
        *cursor += 1;
    }
}

fn consume_colons(chars: &[char], cursor: &mut usize) -> bool {
    if *cursor + 1 >= chars.len() || chars[*cursor] != ':' || chars[*cursor + 1] != ':' {
        return false;
    }
    *cursor += 2;
    true
}

fn consume_ident(chars: &[char], cursor: &mut usize) -> Option<String> {
    if *cursor >= chars.len() {
        return None;
    }
    let start = *cursor;
    let first = chars[*cursor];
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    *cursor += 1;
    while *cursor < chars.len() {
        let ch = chars[*cursor];
        if ch == '_' || ch.is_ascii_alphanumeric() {
            *cursor += 1;
        } else {
            break;
        }
    }
    Some(chars[start..*cursor].iter().collect())
}

fn parse_debt_tags(text: &str) -> Result<Vec<DebtTag>, String> {
    let mut debts = Vec::new();
    for line in text.lines() {
        let Some(start) = line.find("ARCH_DEBT(") else {
            continue;
        };
        let rest = &line[start + "ARCH_DEBT(".len()..];
        let end = rest
            .find(')')
            .ok_or_else(|| String::from("unterminated ARCH_DEBT tag"))?;
        let body = &rest[..end];
        let mut id = None;
        let mut wave = None;
        let mut owner = None;
        let mut reason = None;
        for field in split_csv(body) {
            let mut parts = field.splitn(2, '=');
            let key = parts
                .next()
                .map(str::trim)
                .ok_or_else(|| String::from("missing ARCH_DEBT key"))?;
            let value = parts
                .next()
                .map(str::trim)
                .ok_or_else(|| String::from("missing ARCH_DEBT value"))?;
            match key {
                "id" => id = Some(strip_quotes(value).to_string()),
                "wave" => wave = Some(value.parse::<u32>().map_err(|err| err.to_string())?),
                "owner" => owner = Some(strip_quotes(value).to_string()),
                "reason" => reason = Some(strip_quotes(value).to_string()),
                other => return Err(format!("unknown ARCH_DEBT field '{other}'")),
            }
        }
        debts.push(DebtTag {
            id: id.ok_or_else(|| String::from("missing ARCH_DEBT id"))?,
            wave: wave.ok_or_else(|| String::from("missing ARCH_DEBT wave"))?,
            owner: owner.ok_or_else(|| String::from("missing ARCH_DEBT owner"))?,
            reason: reason.ok_or_else(|| String::from("missing ARCH_DEBT reason"))?,
        });
    }
    Ok(debts)
}

fn split_csv(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in body.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                if !current.trim().is_empty() {
                    items.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }
    items
}

fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

fn is_facade_file(syntax: &File) -> bool {
    if syntax.items.is_empty() {
        return false;
    }
    syntax.items.iter().all(|item| {
        matches!(
            item,
            Item::Use(_) | Item::Mod(_) | Item::ExternCrate(_) | Item::Macro(_)
        )
    })
}
