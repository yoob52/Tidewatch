//! 系统托盘：关闭按钮默认把窗口缩到这里，后台的 Agent 继续跑；从托盘菜单才真正退出。
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Window, Wry};
use tidewatch_core::db::lock;
use tidewatch_core::store::settings;

use crate::AppState;

pub struct Tray {
    show: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    icon: TrayIcon<Wry>,
}

pub fn setup(app: &App) -> tauri::Result<()> {
    // 界面起来后会按当前语言换掉这些文字
    let show = MenuItem::with_id(app, "show", "显示观澜", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出观澜", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("观澜")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder.build(app)?;
    app.manage(Tray { show, quit, icon });
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 主窗口点了关闭：设置里选了缩到托盘就藏起来，不退出。
pub fn on_close_requested(window: &Window, api: &tauri::CloseRequestApi) {
    if window.label() != "main" {
        return;
    }
    let to_tray = window
        .app_handle()
        .try_state::<AppState>()
        .map(|s| {
            settings::get(&lock(s.engine.db()))
                .map(|st| st.close_to_tray)
                .unwrap_or(true)
        })
        .unwrap_or(false);
    if to_tray && window.app_handle().try_state::<Tray>().is_some() {
        api.prevent_close();
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn tray_labels(
    tray: tauri::State<Tray>,
    show: String,
    quit: String,
    tooltip: String,
) -> Result<(), String> {
    tray.show.set_text(show).map_err(|e| e.to_string())?;
    tray.quit.set_text(quit).map_err(|e| e.to_string())?;
    tray.icon
        .set_tooltip(Some(tooltip))
        .map_err(|e| e.to_string())
}
