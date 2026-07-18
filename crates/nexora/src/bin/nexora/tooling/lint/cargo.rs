//! Cargo workspace、依赖边界和技术选型检查。

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use toml_edit::{DocumentMut, Item, Value};

use super::{
    CliError, CliResult,
    diagnostic::{Diagnostic, Report},
    line_column, relative_path,
};

const DEPENDENCY_SECTIONS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
const BROAD_FEATURES: [&str; 3] = ["all", "everything", "full"];
const FORBIDDEN_DATABASE_CRATES: [&str; 4] = ["diesel", "rusqlite", "sea-orm", "sea_orm"];
const FORBIDDEN_HTTP_CRATES: [&str; 7] = [
    "actix-web",
    "actix_web",
    "hyper",
    "poem",
    "rocket",
    "salvo",
    "warp",
];
const FORBIDDEN_RUNTIMES: [&str; 3] = ["async-std", "async_std", "smol"];
const GPUI_COMPONENT_GIT: &str = "https://github.com/longbridge/gpui-component";
const GPUI_COMPONENT_REV: &str = "031555662e99a1b5a549990b47f246d475b8288a";
const ZED_GIT: &str = "https://github.com/zed-industries/zed";
const ZED_REV: &str = "1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba";
const GPUI_REPLACE: &str = "https://github.com/zed-industries/zed#gpui@0.2.2";
const GPUI_MACROS_REPLACE: &str = "https://github.com/zed-industries/zed#gpui_macros@0.1.0";

/// 已解析的 Cargo manifest。
#[derive(Debug)]
struct Manifest {
    path: PathBuf,
    source: String,
    document: DocumentMut,
}

impl Manifest {
    fn load(path: PathBuf) -> CliResult<Self> {
        let source = fs::read_to_string(&path).map_err(|error| {
            CliError::new(format!(
                "无法读取 Cargo manifest {}：{error}",
                path.display()
            ))
        })?;
        let document = source.parse::<DocumentMut>().map_err(|error| {
            CliError::new(format!(
                "无法解析 Cargo manifest {}：{error}",
                path.display()
            ))
        })?;

        Ok(Self {
            path,
            source,
            document,
        })
    }

    fn position(&self, item: &Item) -> (usize, usize) {
        item.span()
            .map_or((1, 1), |span| line_column(&self.source, span.start))
    }

    fn dependency_position(&self, name: &str, item: &Item) -> (usize, usize) {
        let position = self.position(item);
        if position != (1, 1) {
            return position;
        }

        self.source
            .lines()
            .enumerate()
            .find_map(|(index, line)| {
                let (key, _) = line.split_once('=')?;
                let key = key.trim().trim_matches(['\'', '"']);
                (key == name).then_some((index + 1, line.len() - line.trim_start().len() + 1))
            })
            .unwrap_or(position)
    }
}

/// Workspace 中的一个成员 package。
#[derive(Debug)]
pub(super) struct Member {
    name: String,
    directory: PathBuf,
    manifest: Manifest,
    dependencies: Vec<Dependency>,
}

impl Member {
    /// 返回 package 名称。
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// 返回 package 所在目录。
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    /// 返回该 package 是否直接使用 GPUI 或 gpui-component。
    pub(super) fn uses_gpui(&self) -> bool {
        self.uses_dependency("gpui")
            || self.uses_dependency("gpui-component")
            || self.uses_dependency("gpui_component")
    }

    /// 返回该 package 是否直接声明指定依赖。
    pub(super) fn uses_dependency(&self, name: &str) -> bool {
        self.dependencies
            .iter()
            .any(|dependency| dependency.actual_name == name)
    }

    /// 返回该 package 是否承担跨边界契约或共享模型职责。
    pub(super) fn is_contract(&self) -> bool {
        is_contract_crate(&self.name)
    }
}

/// 当前命令检查的 Cargo workspace。
#[derive(Debug)]
pub(super) struct Workspace {
    root: PathBuf,
    root_manifest: Manifest,
    members: Vec<Member>,
}

impl Workspace {
    /// 从指定目录加载 workspace 和全部成员 manifest。
    pub(super) fn load(input: &Path) -> CliResult<Self> {
        let root = workspace_root(input)?;
        let root_manifest = Manifest::load(root.join("Cargo.toml"))?;
        let member_directories = member_directories(&root, &root_manifest.document)?;
        let mut members = Vec::with_capacity(member_directories.len());

        for directory in member_directories {
            let manifest = Manifest::load(directory.join("Cargo.toml"))?;
            let name_item = manifest
                .document
                .get("package")
                .and_then(Item::as_table_like)
                .and_then(|package| package.get("name"))
                .ok_or_else(|| {
                    CliError::new(format!(
                        "workspace 成员 {} 缺少 package.name",
                        manifest.path.display()
                    ))
                })?;
            let name = name_item.as_str().ok_or_else(|| {
                CliError::new(format!(
                    "workspace 成员 {} 的 package.name 必须是字符串",
                    manifest.path.display()
                ))
            })?;
            let dependencies = collect_dependencies(&manifest);

            members.push(Member {
                name: name.to_owned(),
                directory,
                manifest,
                dependencies,
            });
        }

        members.sort_by(|left, right| left.directory.cmp(&right.directory));
        Ok(Self {
            root,
            root_manifest,
            members,
        })
    }

    /// 返回 workspace 根目录。
    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    /// 返回全部成员 package。
    pub(super) fn members(&self) -> &[Member] {
        &self.members
    }
}

#[derive(Debug, Clone)]
struct Dependency {
    declared_name: String,
    actual_name: String,
    uses_workspace: bool,
    features: Vec<String>,
    line: usize,
    column: usize,
}

/// 检查 workspace manifests、crate 目标组织、依赖边界和迁移目录。
pub(super) fn check(workspace: &Workspace, report: &mut Report) -> CliResult<()> {
    check_root_features(workspace, report);
    check_gpui_revision_matrix(workspace, report);
    check_members(workspace, report);
    check_dependency_edges(workspace, report);
    check_migration_locations(workspace, report)?;
    check_modified_migrations(workspace, report)?;
    Ok(())
}

fn check_gpui_revision_matrix(workspace: &Workspace, report: &mut Report) {
    let Some(dependencies) = workspace
        .root_manifest
        .document
        .get("workspace")
        .and_then(Item::as_table_like)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Item::as_table_like)
    else {
        return;
    };
    let Some(component) = dependencies.get("gpui-component") else {
        return;
    };

    check_pinned_dependency(
        workspace,
        "gpui-component",
        component,
        GPUI_COMPONENT_GIT,
        GPUI_COMPONENT_REV,
        report,
    );
    if let Some(assets) = dependencies.get("gpui-component-assets") {
        check_pinned_dependency(
            workspace,
            "gpui-component-assets",
            assets,
            GPUI_COMPONENT_GIT,
            GPUI_COMPONENT_REV,
            report,
        );
    }
    for name in ["gpui", "gpui_platform", "reqwest_client"] {
        if let Some(item) = dependencies.get(name) {
            check_pinned_dependency(workspace, name, item, ZED_GIT, ZED_REV, report);
        }
    }

    let replacements = workspace
        .root_manifest
        .document
        .get("replace")
        .and_then(Item::as_table_like);
    for key in [GPUI_REPLACE, GPUI_MACROS_REPLACE] {
        let valid = replacements
            .and_then(|table| table.get(key))
            .is_some_and(|item| {
                dependency_string_property(item, "git") == Some(ZED_GIT)
                    && dependency_string_property(item, "rev") == Some(ZED_REV)
            });
        if !valid {
            report.push(
                Diagnostic::error(
                    "nexora::gpui_revision_mismatch",
                    relative_path(workspace.root(), &workspace.root_manifest.path),
                    1,
                    1,
                    format!("gpui-component 依赖缺少兼容的 `[replace]` 条目 `{key}`"),
                )
                .with_help(format!(
                    "把 `{key}` 固定到 `{ZED_GIT}` revision `{ZED_REV}`"
                )),
            );
        }
    }
}

fn check_pinned_dependency(
    workspace: &Workspace,
    name: &str,
    item: &Item,
    expected_git: &str,
    expected_rev: &str,
    report: &mut Report,
) {
    if dependency_string_property(item, "git") == Some(expected_git)
        && dependency_string_property(item, "rev") == Some(expected_rev)
    {
        return;
    }
    let (line, column) = workspace.root_manifest.dependency_position(name, item);
    report.push(
        Diagnostic::error(
            "nexora::gpui_revision_mismatch",
            relative_path(workspace.root(), &workspace.root_manifest.path),
            line,
            column,
            format!("依赖 `{name}` 没有使用 Nexora 已验证的 GPUI 兼容 revision"),
        )
        .with_help(format!(
            "设置 git = `{expected_git}`、rev = `{expected_rev}`；升级 gpui-component 时同步更新整组 revision"
        )),
    );
}

fn check_root_features(workspace: &Workspace, report: &mut Report) {
    let Some(dependencies) = workspace
        .root_manifest
        .document
        .get("workspace")
        .and_then(Item::as_table_like)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Item::as_table_like)
    else {
        return;
    };

    for (name, item) in dependencies.iter() {
        let features = dependency_features(item);
        push_broad_feature_diagnostics(
            workspace,
            &workspace.root_manifest,
            name,
            &features,
            item,
            report,
        );
    }
}

fn check_members(workspace: &Workspace, report: &mut Report) {
    let workspace_dependencies = workspace_dependency_names(&workspace.root_manifest.document);

    for member in &workspace.members {
        check_member_name(workspace, member, report);
        check_member_targets(workspace, member, report);

        for dependency in &member.dependencies {
            if !dependency.uses_workspace {
                report.push(
                    Diagnostic::error(
                        "nexora::dependency_not_in_workspace",
                        relative_path(workspace.root(), &member.manifest.path),
                        dependency.line,
                        dependency.column,
                        format!(
                            "依赖 `{}` 没有通过 workspace 继承",
                            dependency.declared_name
                        ),
                    )
                    .with_help(format!(
                        "先在根 Cargo.toml 的 [workspace.dependencies] 声明 `{}`，再使用 `{} = {{ workspace = true }}`",
                        dependency.declared_name, dependency.declared_name
                    )),
                );
            } else if !workspace_dependencies.contains(dependency.declared_name.as_str()) {
                report.push(
                    Diagnostic::error(
                        "nexora::dependency_not_in_workspace",
                        relative_path(workspace.root(), &member.manifest.path),
                        dependency.line,
                        dependency.column,
                        format!(
                            "依赖 `{}` 使用 workspace 继承，但根 manifest 没有对应声明",
                            dependency.declared_name
                        ),
                    )
                    .with_help("在根 Cargo.toml 的 [workspace.dependencies] 中补充该依赖"),
                );
            }

            push_broad_feature_diagnostics_from_dependency(workspace, member, dependency, report);
            check_technology(workspace, member, dependency, report);
        }
    }
}

fn check_member_name(workspace: &Workspace, member: &Member, report: &mut Report) {
    let normalized = member.name.replace('_', "-");
    let invalid = ["-core", "-handle", "-handler"]
        .into_iter()
        .any(|suffix| normalized.ends_with(suffix) && normalized.len() > suffix.len());
    if !invalid {
        return;
    }

    let package_item = member
        .manifest
        .document
        .get("package")
        .and_then(Item::as_table_like)
        .and_then(|package| package.get("name"));
    let (line, column) = package_item.map_or((1, 1), |item| member.manifest.position(item));
    report.push(
        Diagnostic::error(
            "nexora::invalid_crate_name",
            relative_path(workspace.root(), &member.manifest.path),
            line,
            column,
            format!("crate `{}` 使用了被禁止的通用后缀命名", member.name),
        )
        .with_help("直接使用明确的业务职责名称，例如 domain、accounts、projects 或 desktop"),
    );
}

fn check_member_targets(workspace: &Workspace, member: &Member, report: &mut Report) {
    let has_binary = member.directory.join("src/main.rs").is_file()
        || member.manifest.document.get("bin").is_some();
    let has_library = member.directory.join("src/lib.rs").is_file()
        || member.manifest.document.get("lib").is_some();
    if !(has_binary && has_library) || allows_mixed_targets(member) {
        return;
    }

    report.push(
        Diagnostic::error(
            "nexora::mixed_binary_library",
            relative_path(workspace.root(), &member.manifest.path),
            1,
            1,
            format!(
                "bin package `{}` 同时声明了 binary 和 library target",
                member.name
            ),
        )
        .with_help("保留 src/main.rs；需要复用的业务能力应拆到职责明确的 workspace library crate"),
    );
}

fn allows_mixed_targets(member: &Member) -> bool {
    let Some(configuration) = member
        .manifest
        .document
        .get("package")
        .and_then(Item::as_table_like)
        .and_then(|package| package.get("metadata"))
        .and_then(Item::as_table_like)
        .and_then(|metadata| metadata.get("nexora"))
        .and_then(Item::as_table_like)
    else {
        return false;
    };
    let allowed = configuration
        .get("allow-mixed-targets")
        .and_then(Item::as_bool)
        .unwrap_or(false);
    let has_reason = configuration
        .get("reason")
        .and_then(Item::as_str)
        .is_some_and(|reason| !reason.trim().is_empty());

    allowed && has_reason
}

fn check_technology(
    workspace: &Workspace,
    member: &Member,
    dependency: &Dependency,
    report: &mut Report,
) {
    let name = dependency.actual_name.as_str();
    let replacement = if FORBIDDEN_DATABASE_CRATES.contains(&name) {
        Some("数据库访问统一使用 sqlx")
    } else if FORBIDDEN_HTTP_CRATES.contains(&name) {
        Some("HTTP 服务统一使用 axum；业务 handler 不直接使用 hyper")
    } else if FORBIDDEN_RUNTIMES.contains(&name) {
        Some("异步运行时统一使用 tokio")
    } else {
        None
    };

    if let Some(replacement) = replacement {
        report.push(
            Diagnostic::error(
                "nexora::forbidden_technology",
                relative_path(workspace.root(), &member.manifest.path),
                dependency.line,
                dependency.column,
                format!(
                    "package `{}` 直接引入了不符合技术选型的依赖 `{name}`",
                    member.name
                ),
            )
            .with_help(replacement),
        );
    }
}

fn check_dependency_edges(workspace: &Workspace, report: &mut Report) {
    let members_by_name = workspace
        .members
        .iter()
        .map(|member| (member.name.as_str(), member))
        .collect::<HashMap<_, _>>();

    for member in &workspace.members {
        for dependency in &member.dependencies {
            if is_contract_crate(&member.name)
                && matches!(
                    dependency.actual_name.as_str(),
                    "axum" | "hyper" | "sqlx" | "tokio"
                )
            {
                report.push(
                    Diagnostic::warning(
                        "nexora::forbidden_dependency_edge",
                        relative_path(workspace.root(), &member.manifest.path),
                        dependency.line,
                        dependency.column,
                        format!(
                            "轻量契约 crate `{}` 直接依赖了基础设施 `{}`",
                            member.name, dependency.actual_name
                        ),
                    )
                    .with_help(
                        "契约 crate 只保留序列化模型和必要校验，不引入 HTTP、数据库或运行时实现",
                    ),
                );
            }

            let Some(target) = members_by_name.get(dependency.actual_name.as_str()) else {
                continue;
            };

            let member_area = first_component(relative_path(workspace.root(), &member.directory));
            let target_area = first_component(relative_path(workspace.root(), &target.directory));
            let library_depends_on_app = member_area.as_deref() == Some("crates")
                && matches!(target_area.as_deref(), Some("apps" | "examples"));
            let console_depends_on_server =
                member.name == "console" && matches!(target.name.as_str(), "api" | "server");
            let lightweight_contract_has_heavy_dependency = is_contract_crate(&member.name)
                && matches!(
                    target.name.as_str(),
                    "api" | "server" | "application" | "desktop"
                );

            if library_depends_on_app
                || console_depends_on_server
                || lightweight_contract_has_heavy_dependency
            {
                report.push(
                    Diagnostic::warning(
                        "nexora::forbidden_dependency_edge",
                        relative_path(workspace.root(), &member.manifest.path),
                        dependency.line,
                        dependency.column,
                        format!(
                            "依赖方向 `{}` -> `{}` 可能把应用实现或基础设施泄漏到下层 crate",
                            member.name, target.name
                        ),
                    )
                    .with_help("抽取轻量 contracts、models 或具体业务 crate，并保持依赖单向"),
                );
            }
        }
    }
}

fn check_migration_locations(workspace: &Workspace, report: &mut Report) -> CliResult<()> {
    let expected = Path::new("crates/migrate/migrations");
    let mut directories = Vec::new();
    collect_directories_named(workspace.root(), "migrations", &mut directories)?;

    for directory in directories {
        let relative = relative_path(workspace.root(), &directory);
        if relative == expected {
            continue;
        }

        report.push(
            Diagnostic::error(
                "nexora::invalid_migration_location",
                relative.join("."),
                1,
                1,
                format!("数据库迁移目录 `{}` 不在统一位置", relative.display()),
            )
            .with_help("将迁移文件放到 crates/migrate/migrations，并使用 sqlx migrate 管理"),
        );
    }

    for member in &workspace.members {
        if member.name == "migrate"
            && relative_path(workspace.root(), &member.directory) != Path::new("crates/migrate")
        {
            report.push(
                Diagnostic::error(
                    "nexora::invalid_migration_location",
                    relative_path(workspace.root(), &member.manifest.path),
                    1,
                    1,
                    "迁移 crate 必须位于 crates/migrate",
                )
                .with_help("移动 package，并把迁移 SQL 集中到 crates/migrate/migrations"),
            );
        }
    }

    Ok(())
}

fn check_modified_migrations(workspace: &Workspace, report: &mut Report) -> CliResult<()> {
    let mut modified = BTreeSet::new();
    for cached in [false, true] {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(workspace.root())
            .args(["diff", "--name-only", "--diff-filter=M"]);
        if cached {
            command.arg("--cached");
        }
        let output = match command.output() {
            Ok(output) if output.status.success() => output,
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(CliError::new(format!(
                    "无法检查已修改的数据库迁移：{error}"
                )));
            }
        };
        modified.extend(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(PathBuf::from),
        );
    }

    let migration_root = Path::new("crates/migrate/migrations");
    for path in modified {
        if !path.starts_with(migration_root) {
            continue;
        }
        report.push(
            Diagnostic::error(
                "nexora::modified_migration",
                path,
                1,
                1,
                "已经提交的数据库迁移文件被修改",
            )
            .with_help("恢复原迁移，并使用 sqlx-cli 创建新的后续迁移完成修正"),
        );
    }

    Ok(())
}

fn push_broad_feature_diagnostics(
    workspace: &Workspace,
    manifest: &Manifest,
    dependency_name: &str,
    features: &[String],
    item: &Item,
    report: &mut Report,
) {
    let Some(feature) = features
        .iter()
        .find(|feature| BROAD_FEATURES.contains(&feature.as_str()))
    else {
        return;
    };
    let (line, column) = manifest.dependency_position(dependency_name, item);
    if manifest_is_suppressed(manifest, "nexora::broad_dependency_feature", line) {
        return;
    }

    report.push(
        Diagnostic::warning(
            "nexora::broad_dependency_feature",
            relative_path(workspace.root(), &manifest.path),
            line,
            column,
            format!("依赖 `{dependency_name}` 启用了聚合 feature `{feature}`"),
        )
        .with_help("只启用当前 crate 实际使用的最小 feature 集合"),
    );
}

fn push_broad_feature_diagnostics_from_dependency(
    workspace: &Workspace,
    member: &Member,
    dependency: &Dependency,
    report: &mut Report,
) {
    let Some(feature) = dependency
        .features
        .iter()
        .find(|feature| BROAD_FEATURES.contains(&feature.as_str()))
    else {
        return;
    };
    if manifest_is_suppressed(
        &member.manifest,
        "nexora::broad_dependency_feature",
        dependency.line,
    ) {
        return;
    }

    report.push(
        Diagnostic::warning(
            "nexora::broad_dependency_feature",
            relative_path(workspace.root(), &member.manifest.path),
            dependency.line,
            dependency.column,
            format!(
                "依赖 `{}` 启用了聚合 feature `{feature}`",
                dependency.declared_name
            ),
        )
        .with_help("只启用当前 crate 实际使用的最小 feature 集合"),
    );
}

fn workspace_root(input: &Path) -> CliResult<PathBuf> {
    let path = if input.file_name().is_some_and(|name| name == "Cargo.toml") {
        input.parent().unwrap_or(Path::new("."))
    } else {
        input
    };
    let root = path.canonicalize().map_err(|error| {
        CliError::new(format!(
            "无法访问 workspace 路径 {}：{error}",
            path.display()
        ))
    })?;
    if !root.join("Cargo.toml").is_file() {
        return Err(CliError::new(format!(
            "{} 不是 Cargo workspace：缺少 Cargo.toml",
            root.display()
        )));
    }
    Ok(root)
}

fn member_directories(root: &Path, document: &DocumentMut) -> CliResult<Vec<PathBuf>> {
    let workspace = document
        .get("workspace")
        .and_then(Item::as_table_like)
        .ok_or_else(|| CliError::new("根 Cargo.toml 缺少 [workspace]"))?;
    let mut directories = BTreeSet::new();

    if document.get("package").is_some() {
        directories.insert(root.to_path_buf());
    }

    let members = workspace
        .get("members")
        .and_then(Item::as_array)
        .ok_or_else(|| CliError::new("根 Cargo.toml 缺少 workspace.members"))?;
    for member in members.iter() {
        let pattern = member
            .as_str()
            .ok_or_else(|| CliError::new("workspace.members 只能包含字符串路径"))?;
        expand_member_pattern(root, pattern, &mut directories)?;
    }

    if let Some(excludes) = workspace.get("exclude").and_then(Item::as_array) {
        let mut excluded = BTreeSet::new();
        for exclude in excludes.iter() {
            let pattern = exclude
                .as_str()
                .ok_or_else(|| CliError::new("workspace.exclude 只能包含字符串路径"))?;
            expand_member_pattern(root, pattern, &mut excluded)?;
        }
        directories.retain(|directory| !excluded.contains(directory));
    }

    Ok(directories.into_iter().collect())
}

fn expand_member_pattern(
    root: &Path,
    pattern: &str,
    directories: &mut BTreeSet<PathBuf>,
) -> CliResult<()> {
    let segments = pattern
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    expand_segments(root, &segments, directories)
}

fn expand_segments(
    current: &Path,
    segments: &[&str],
    directories: &mut BTreeSet<PathBuf>,
) -> CliResult<()> {
    let Some((segment, remaining)) = segments.split_first() else {
        if current.join("Cargo.toml").is_file() {
            directories.insert(current.canonicalize().map_err(|error| {
                CliError::new(format!(
                    "无法访问 workspace 成员 {}：{error}",
                    current.display()
                ))
            })?);
        }
        return Ok(());
    };

    if segment.contains(['*', '?']) {
        let entries = fs::read_dir(current).map_err(|error| {
            CliError::new(format!(
                "无法读取 workspace 目录 {}：{error}",
                current.display()
            ))
        })?;
        for entry in entries {
            let entry = entry
                .map_err(|error| CliError::new(format!("无法读取 workspace 成员目录：{error}")))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if entry.path().is_dir() && wildcard_matches(segment, &file_name) {
                expand_segments(&entry.path(), remaining, directories)?;
            }
        }
    } else {
        expand_segments(&current.join(segment), remaining, directories)?;
    }

    Ok(())
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut pattern_ix, mut value_ix, mut star_ix, mut star_value_ix) = (0, 0, None, 0);

    while value_ix < value.len() {
        if pattern_ix < pattern.len()
            && (pattern[pattern_ix] == b'?' || pattern[pattern_ix] == value[value_ix])
        {
            pattern_ix += 1;
            value_ix += 1;
        } else if pattern_ix < pattern.len() && pattern[pattern_ix] == b'*' {
            star_ix = Some(pattern_ix);
            pattern_ix += 1;
            star_value_ix = value_ix;
        } else if let Some(star) = star_ix {
            pattern_ix = star + 1;
            star_value_ix += 1;
            value_ix = star_value_ix;
        } else {
            return false;
        }
    }

    pattern[pattern_ix..].iter().all(|byte| *byte == b'*')
}

fn collect_dependencies(manifest: &Manifest) -> Vec<Dependency> {
    let mut dependencies = Vec::new();

    for section in DEPENDENCY_SECTIONS {
        if let Some(table) = manifest.document.get(section).and_then(Item::as_table_like) {
            collect_dependency_table(table, manifest, &mut dependencies);
        }
    }

    if let Some(targets) = manifest
        .document
        .get("target")
        .and_then(Item::as_table_like)
    {
        for (_, target) in targets.iter() {
            let Some(target) = target.as_table_like() else {
                continue;
            };
            for section in DEPENDENCY_SECTIONS {
                if let Some(table) = target.get(section).and_then(Item::as_table_like) {
                    collect_dependency_table(table, manifest, &mut dependencies);
                }
            }
        }
    }

    dependencies
}

fn collect_dependency_table(
    table: &dyn toml_edit::TableLike,
    manifest: &Manifest,
    dependencies: &mut Vec<Dependency>,
) {
    for (name, item) in table.iter() {
        let (line, column) = manifest.dependency_position(name, item);
        dependencies.push(Dependency {
            declared_name: name.to_owned(),
            actual_name: dependency_string_property(item, "package")
                .unwrap_or(name)
                .to_owned(),
            uses_workspace: dependency_bool_property(item, "workspace") == Some(true),
            features: dependency_features(item),
            line,
            column,
        });
    }
}

fn dependency_features(item: &Item) -> Vec<String> {
    dependency_property(item, "features")
        .and_then(Value::as_array)
        .map(|features| {
            features
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn dependency_string_property<'a>(item: &'a Item, name: &str) -> Option<&'a str> {
    dependency_property(item, name).and_then(Value::as_str)
}

fn dependency_bool_property(item: &Item, name: &str) -> Option<bool> {
    dependency_property(item, name).and_then(Value::as_bool)
}

fn dependency_property<'a>(item: &'a Item, name: &str) -> Option<&'a Value> {
    match item {
        Item::Value(Value::InlineTable(table)) => table.get(name),
        Item::Table(table) => table.get(name).and_then(Item::as_value),
        _ => None,
    }
}

fn workspace_dependency_names(document: &DocumentMut) -> HashSet<&str> {
    document
        .get("workspace")
        .and_then(Item::as_table_like)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Item::as_table_like)
        .map(|dependencies| dependencies.iter().map(|(name, _)| name).collect())
        .unwrap_or_default()
}

fn manifest_is_suppressed(manifest: &Manifest, rule: &str, line: usize) -> bool {
    let lines = manifest.source.lines().collect::<Vec<_>>();
    let start = line.saturating_sub(4);
    let end = line.saturating_sub(1).min(lines.len());

    lines[start..end].iter().any(|candidate| {
        candidate.contains("nexora-lint: allow(")
            && candidate.contains(rule)
            && candidate.contains("reason=")
    })
}

fn first_component(path: PathBuf) -> Option<String> {
    path.components().find_map(|component| match component {
        Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
        _ => None,
    })
}

fn is_contract_crate(name: &str) -> bool {
    matches!(name, "contract" | "contracts" | "model" | "models")
        || name.ends_with("-contracts")
        || name.ends_with("-models")
}

fn collect_directories_named(
    directory: &Path,
    target_name: &str,
    output: &mut Vec<PathBuf>,
) -> CliResult<()> {
    for entry in fs::read_dir(directory).map_err(|error| {
        CliError::new(format!(
            "无法扫描 workspace 目录 {}：{error}",
            directory.display()
        ))
    })? {
        let entry =
            entry.map_err(|error| CliError::new(format!("无法读取 workspace 目录项：{error}")))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), ".git" | "dist" | "target") {
            continue;
        }
        if name == target_name {
            output.push(path);
            continue;
        }
        collect_directories_named(&path, target_name, output)?;
    }

    Ok(())
}
