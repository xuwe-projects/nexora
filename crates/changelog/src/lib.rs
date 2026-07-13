//! 应用更新日志的嵌入、解析与查询能力。
//!
//! 更新日志内容统一存放在 workspace 根目录的 `changelogs` 中，并采用
//! `<version>/<component>/<locale>.md` 路径约定。该 crate 会在编译时嵌入这些文件，
//! 让桌面程序可以在不访问文件系统和网络的情况下展示当前版本更新内容。

use std::{error::Error, fmt, path::Path};

use include_dir::{Dir, include_dir};
use semver::Version;

static CHANGELOGS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../changelogs");

/// 一条可供应用展示的版本更新日志。
///
/// 条目中的组件标识来自目录名，例如 `console` 或 `api`；语言标识来自 Markdown
/// 文件名，例如 `zh-CN.md`。Markdown 内容在编译时嵌入二进制，因此引用在应用
/// 生命周期内始终有效。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    version: Version,
    component: String,
    locale: String,
    markdown: &'static str,
    source_path: String,
}

impl ChangelogEntry {
    /// 返回该更新日志所属的语义化版本。
    ///
    /// 版本来自日志路径的第一段，并已经通过 `semver` 校验。
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// 返回该更新日志所属的应用或服务组件标识。
    ///
    /// 该值适合与应用启动时配置的组件 ID 比较，不应直接作为本地化展示名称。
    pub fn component(&self) -> &str {
        &self.component
    }

    /// 返回该更新日志使用的语言区域标识。
    ///
    /// 当前目录约定使用 `zh-CN`、`en-US` 这类 BCP 47 风格标识。
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// 返回可以交给 Markdown 组件渲染的日志正文。
    pub fn markdown(&self) -> &'static str {
        self.markdown
    }

    /// 返回该条目在 `changelogs` 目录下的相对源文件路径。
    ///
    /// 该路径主要用于校验错误、开发工具提示和测试，不表示运行时真实文件一定存在。
    pub fn source_path(&self) -> &str {
        &self.source_path
    }
}

/// 编译进应用的更新日志仓库。
///
/// 仓库初始化时会解析所有符合目录约定的 Markdown，并按语义化版本从新到旧排序。
/// 同一版本下使用组件标识和语言标识保证结果稳定，避免依赖文件系统遍历顺序。
#[derive(Debug)]
pub struct EmbeddedChangelogRepository {
    entries: Vec<ChangelogEntry>,
}

impl EmbeddedChangelogRepository {
    /// 解析所有编译进应用的更新日志并创建仓库。
    ///
    /// 根目录的 `README.md` 只用于说明维护方式，不会进入日志条目。
    ///
    /// # Errors
    ///
    /// 当 Markdown 路径不符合 `<version>/<component>/<locale>.md`、版本号不符合
    /// SemVer，或者文件内容不是 UTF-8 时返回 [`ChangelogError`]。
    pub fn load() -> Result<Self, ChangelogError> {
        let mut entries = Vec::new();
        collect_entries(&CHANGELOGS, &mut entries)?;
        entries.sort_by(|left, right| {
            right
                .version
                .cmp(&left.version)
                .then_with(|| left.component.cmp(&right.component))
                .then_with(|| left.locale.cmp(&right.locale))
        });

        Ok(Self { entries })
    }

    /// 返回仓库中全部日志条目。
    ///
    /// 条目首先按版本从新到旧排列，同版本下再按组件和语言标识稳定排序。
    pub fn entries(&self) -> &[ChangelogEntry] {
        &self.entries
    }

    /// 查找指定组件、版本和语言的更新日志。
    ///
    /// 没有对应 Markdown 文件时返回 `None`，调用方可以展示缺省提示或回退到在线日志。
    pub fn find(
        &self,
        component: &str,
        version: &Version,
        locale: &str,
    ) -> Option<&ChangelogEntry> {
        self.entries.iter().find(|entry| {
            entry.component == component && entry.version == *version && entry.locale == locale
        })
    }

    /// 返回指定组件和语言的全部更新日志。
    ///
    /// 返回顺序保持为版本从新到旧，可直接用于构建版本历史列表。
    pub fn releases<'a>(
        &'a self,
        component: &'a str,
        locale: &'a str,
    ) -> impl Iterator<Item = &'a ChangelogEntry> + 'a {
        self.entries
            .iter()
            .filter(move |entry| entry.component == component && entry.locale == locale)
    }
}

/// 更新日志目录或内容不符合约定时产生的错误。
#[derive(Debug)]
pub enum ChangelogError {
    /// Markdown 路径没有提供版本、组件和语言三段信息。
    InvalidPath {
        /// 无法解析的相对文件路径。
        path: String,
    },
    /// 路径中的版本目录不是合法的语义化版本号。
    InvalidVersion {
        /// 包含非法版本号的相对文件路径。
        path: String,
        /// `semver` 返回的具体解析错误。
        source: semver::Error,
    },
    /// Markdown 文件内容不是有效的 UTF-8 文本。
    InvalidUtf8 {
        /// 内容无法解码的相对文件路径。
        path: String,
    },
}

impl fmt::Display for ChangelogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { path } => write!(
                formatter,
                "更新日志路径 `{path}` 必须符合 <version>/<component>/<locale>.md"
            ),
            Self::InvalidVersion { path, source } => {
                write!(formatter, "更新日志路径 `{path}` 包含非法版本号: {source}")
            }
            Self::InvalidUtf8 { path } => {
                write!(formatter, "更新日志文件 `{path}` 不是有效的 UTF-8 文本")
            }
        }
    }
}

impl Error for ChangelogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidVersion { source, .. } => Some(source),
            Self::InvalidPath { .. } | Self::InvalidUtf8 { .. } => None,
        }
    }
}

fn collect_entries(
    directory: &'static Dir<'static>,
    entries: &mut Vec<ChangelogEntry>,
) -> Result<(), ChangelogError> {
    for file in directory.files() {
        let path = file.path();
        if path == Path::new("README.md")
            || path.extension().and_then(|value| value.to_str()) != Some("md")
        {
            continue;
        }

        entries.push(parse_entry(file.path(), file.contents_utf8())?);
    }

    for child in directory.dirs() {
        collect_entries(child, entries)?;
    }

    Ok(())
}

fn parse_entry(
    path: &Path,
    markdown: Option<&'static str>,
) -> Result<ChangelogEntry, ChangelogError> {
    let source_path = path.to_string_lossy().into_owned();
    let parts = path
        .iter()
        .map(|part| part.to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| ChangelogError::InvalidPath {
            path: source_path.clone(),
        })?;
    let [version, component, file_name] = parts.as_slice() else {
        return Err(ChangelogError::InvalidPath { path: source_path });
    };
    let locale = file_name
        .strip_suffix(".md")
        .filter(|locale| !locale.is_empty())
        .ok_or_else(|| ChangelogError::InvalidPath {
            path: source_path.clone(),
        })?;
    if component.is_empty() {
        return Err(ChangelogError::InvalidPath { path: source_path });
    }

    let version = Version::parse(version).map_err(|source| ChangelogError::InvalidVersion {
        path: source_path.clone(),
        source,
    })?;
    let markdown = markdown.ok_or_else(|| ChangelogError::InvalidUtf8 {
        path: source_path.clone(),
    })?;

    Ok(ChangelogEntry {
        version,
        component: (*component).to_owned(),
        locale: locale.to_owned(),
        markdown,
        source_path,
    })
}
