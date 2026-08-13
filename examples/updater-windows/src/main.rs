#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gpui::{App, Context, IntoElement, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use nexora::{
    Application as _, ApplicationLogo, ApplicationOptions, FeatureElement,
    FeatureReloadAvailability, NavigationContextExt as _, desktop,
};

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "更新程序",
    path = "/",
    section = "Updater",
    icon = "rotate-ccw",
    order = 0
)]
struct UpdaterFeature;

impl FeatureElement for UpdaterFeature {
    fn reload_availability(&self) -> FeatureReloadAvailability {
        FeatureReloadAvailability::Available
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Task<()> {
        _ = desktop::check_for_updates(window, cx);
        gpui::Task::ready(())
    }

    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = theme::component_size(cx);
        let update_button = desktop::check_for_updates_button("updater-windows-check", cx)
            .map(|button| button.primary().with_size(size));
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .child("Windows 更新程序示例"),
            )
            .child(format!("Cargo 版本: {}", env!("CARGO_PKG_VERSION")))
            .child(format!(
                "健康失败注入: {}",
                option_env!("NEXORA_EXAMPLE_HEALTH_FAILURE").unwrap_or("off")
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("updater-windows-open-window")
                            .icon(IconName::PanelRightOpen)
                            .label("第二窗口")
                            .with_size(size)
                            .on_click(|_, _, cx| {
                                _ = cx.navigate("/second-window");
                            }),
                    )
                    .children(update_button),
            )
            .child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_4()
                    .max_w(px(680.0))
                    .child("这个 example 使用本地 RustFS/S3 发布源验证 Windows setup EXE、update ZIP、sidecar 替换、健康确认和失败回滚。"),
            )
    }
}

#[derive(Default, nexora::Window)]
#[nexora(title = "第二窗口", path = "/second-window")]
struct SecondWindow;

impl nexora::WindowElement for SecondWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_5()
            .gap_3()
            .child(div().text_lg().font_semibold().child("第二原生窗口"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("强制更新门禁测试会确认这里不能绕过主窗口持有的公共更新 Dialog。"),
            )
    }
}

struct ExampleApplication {
    updater: Option<desktop::UpdateConfig>,
}

impl nexora::Application for ExampleApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("Windows 更新程序示例")
            .application_logo(ApplicationLogo::png(include_bytes!(
                "../assets/logos/updater-windows/logo-icon-128.png"
            )))
            .initial_path("/")
            .window_size(960.0, 640.0)
    }

    fn initialize(&mut self, cx: &mut App) {
        if let Some(updater) = self.updater.clone() {
            desktop::install_updater(updater, cx).expect("example 只能安装一份 updater 配置");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let updater = desktop::UpdateConfig::from_current_bundle_if_present()?.map(|updater| {
        updater.with_health_report_on_launch(
            option_env!("NEXORA_EXAMPLE_HEALTH_FAILURE") != Some("before-health"),
        )
    });
    ExampleApplication { updater }.run()?;
    Ok(())
}
