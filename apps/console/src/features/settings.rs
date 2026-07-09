//! 应用设置功能模块。
//!
//! 该模块展示桌面应用常见设置项的页面结构，用于承载后续偏好配置和运行时开关。

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _,
    description_list::{DescriptionItem, DescriptionList},
    group_box::{GroupBox, GroupBoxVariants as _},
    tag::Tag,
};

/// 应用设置功能视图。
///
/// 当前实现只渲染静态设置项，用于说明设置 feature 可以独立管理分组、说明和当前值。
pub struct SettingsFeature;

impl SettingsFeature {
    /// 渲染设置页面。
    ///
    /// 页面展示窗口、后台模式和打包配置等模板级设置，后续可以接入真实持久化配置。
    /// 顶部说明文字会跟随当前组件主题，分组和标签由 `gpui-component` 负责主题化。
    pub fn render<T>(cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let theme = cx.theme();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_bold()
                            .text_color(theme.foreground)
                            .child("应用设置"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("把窗口、后台模式、打包参数等运行配置集中在一个 feature。"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(setting_groups().iter().map(setting_group)),
            )
            .into_any_element()
    }
}

/// 设置页中的一个静态分组。
///
/// 真实应用可以把该类型替换为持久化配置、运行时状态或偏好设置模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingGroupData {
    title: &'static str,
    items: &'static [(&'static str, &'static str)],
}

impl SettingGroupData {
    /// 返回设置分组标题。
    ///
    /// 标题会显示在分组容器顶部，用于区分窗口、运行和发布等配置域。
    pub fn title(self) -> &'static str {
        self.title
    }

    /// 返回该分组中的设置项。
    ///
    /// 每个元组的第一项是设置名称，第二项是当前展示值。
    pub fn items(self) -> &'static [(&'static str, &'static str)] {
        self.items
    }
}

/// 返回设置页默认展示的模板配置分组。
///
/// 返回值顺序就是页面渲染顺序，用于稳定展示窗口、运行和发布三个配置区域。
pub fn setting_groups() -> &'static [SettingGroupData] {
    static WINDOW_ITEMS: [(&str, &str); 3] = [
        ("默认尺寸", "900 x 640"),
        ("最小尺寸", "900 x 640"),
        ("启动激活", "开启"),
    ];
    static RUNTIME_ITEMS: [(&str, &str); 3] = [
        ("守护模式", "关闭"),
        ("默认目标", "aarch64-apple-darwin"),
        ("打包输出", "dist/"),
    ];
    static RELEASE_ITEMS: [(&str, &str); 3] = [
        ("本地签名", "ad-hoc"),
        ("公证 profile", "xuwe"),
        ("校验文件", ".sha256"),
    ];
    static GROUPS: [SettingGroupData; 3] = [
        SettingGroupData {
            title: "窗口",
            items: &WINDOW_ITEMS,
        },
        SettingGroupData {
            title: "运行",
            items: &RUNTIME_ITEMS,
        },
        SettingGroupData {
            title: "发布",
            items: &RELEASE_ITEMS,
        },
    ];

    &GROUPS
}

fn setting_group(group: &SettingGroupData) -> AnyElement {
    GroupBox::new()
        .outline()
        .title(group.title)
        .child(
            DescriptionList::horizontal()
                .columns(1)
                .label_width(px(140.))
                .bordered(false)
                .children(group.items.iter().map(|(label, value)| {
                    DescriptionItem::new(*label).value(
                        Tag::secondary()
                            .small()
                            .outline()
                            .child(*value)
                            .into_any_element(),
                    )
                })),
        )
        .into_any_element()
}
