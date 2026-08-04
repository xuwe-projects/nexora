//! 桌面进程当前安装版本的只读发布信息。

use gpui::{App, Global};

use updater::{LoadedApplicationReleaseMetadata, UpdateChannel, load_current_release_metadata};

/// 当前桌面应用的只读发布信息快照。
///
/// 正式 `nexora build` 产物从安装包内经过校验的 `nexora-release.json` 读取版本、构建号、
/// 通道和 app ID。普通 `cargo run`、测试或其它没有发布元数据的开发进程只回退应用名称与
/// `ApplicationOptions::application_version`，其构建号、通道和 app ID 都为 `None`。
#[derive(Debug, Clone)]
pub struct ApplicationInfo {
    application_name: String,
    version: Option<String>,
    app_id: Option<String>,
    build_number: Option<u64>,
    channel: Option<UpdateChannel>,
    release: Option<LoadedApplicationReleaseMetadata>,
}

impl Global for ApplicationInfo {}

impl ApplicationInfo {
    pub(crate) fn load(
        application_name: String,
        application_version: Option<String>,
    ) -> Result<Self, String> {
        let release = load_current_release_metadata().map_err(|error| error.to_string())?;
        let Some(release) = release else {
            return Ok(Self {
                application_name,
                version: application_version,
                app_id: None,
                build_number: None,
                channel: None,
                release: None,
            });
        };
        let metadata = release.metadata();
        Ok(Self {
            application_name: metadata.display_name.clone(),
            version: Some(metadata.version.to_string()),
            app_id: Some(metadata.app_id.clone()),
            build_number: Some(metadata.build_number),
            channel: Some(metadata.channel),
            release: Some(release),
        })
    }

    /// 返回面向用户展示的应用名称。
    ///
    /// 正式安装包优先使用发布元数据中的 `display_name`；开发模式使用
    /// `ApplicationOptions::application_name`。
    pub fn application_name(&self) -> &str {
        &self.application_name
    }

    /// 返回正式发布的稳定 app ID。
    ///
    /// 开发运行没有可信发布身份时返回 `None`，不会从 package 名称或运行路径猜测标识。
    pub fn app_id(&self) -> Option<&str> {
        self.app_id.as_deref()
    }

    /// 返回当前语义化版本文本。
    ///
    /// 正式安装包使用发布元数据；开发模式回退应用选项。应用显式隐藏开发版本时可以为
    /// `None`。
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// 返回 release receipt 冻结的正式构建号。
    ///
    /// 普通 `cargo run`、测试运行或缺少发布元数据时返回 `None`；本接口永远不会用零、时间、
    /// 随机数或进程状态伪造构建号。
    pub const fn build_number(&self) -> Option<u64> {
        self.build_number
    }

    /// 返回正式安装包的固定发布通道。
    ///
    /// 开发运行没有可信发布元数据时返回 `None`。
    pub const fn channel(&self) -> Option<UpdateChannel> {
        self.channel
    }

    pub(crate) fn loaded_release(&self) -> Option<&LoadedApplicationReleaseMetadata> {
        self.release.as_ref()
    }
}

/// 读取当前进程已经初始化的应用发布信息。
///
/// Nexora 会在调用应用自身 `Application::initialize` 之前注册该只读 Global，因此自定义
/// LoginFeature、Feature、窗口和应用初始化代码都可以直接读取。该接口不执行文件 I/O。
///
/// # Panics
///
/// 在 Nexora 桌面运行器完成应用初始化之前调用时会 panic；正常应用生命周期中不会发生。
pub fn application_info(cx: &App) -> &ApplicationInfo {
    cx.global::<ApplicationInfo>()
}
