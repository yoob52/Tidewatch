//! 系统托盘：关闭按钮默认把窗口缩到这里，后台的 Agent 继续跑；从托盘菜单才真正退出。
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use std::sync::Mutex;
use tauri::{App, AppHandle, Manager, Window, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tidewatch_core::db::lock;
use tidewatch_core::store::{runs, settings};

use crate::AppState;

pub struct Tray {
    show: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    prompt: Mutex<QuitPrompt>,
}

/// 有运行在进行时退出前的确认框文字，跟着界面语言走。
struct QuitPrompt {
    message: String,
    ok: String,
    cancel: String,
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
            "quit" => self::quit(app),
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
    builder.build(app)?;
    let prompt = Mutex::new(QuitPrompt {
        message: "还有正在运行的 Agent，退出会中断它们。确定退出观澜吗？".into(),
        ok: "退出观澜".into(),
        cancel: "取消".into(),
    });
    app.manage(Tray { show, quit, prompt });
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 图标只在最后一个句柄释放时才从 Windows 托盘删掉；app.exit 不会释放它，
/// 不先删就会留下一个鼠标划过才消失的空图标。
fn remove(app: &AppHandle) {
    drop(app.remove_tray_by_id("main"));
}

fn exit(app: &AppHandle) {
    remove(app);
    app.exit(0);
}

fn running(app: &AppHandle) -> u32 {
    app.try_state::<AppState>()
        .and_then(|s| runs::count_running(&lock(s.engine.db())).ok())
        .unwrap_or(0)
}

/// 退出观澜；有运行在进行时先确认，取消就什么都不做。
pub fn quit(app: &AppHandle) {
    if running(app) == 0 {
        exit(app);
        return;
    }
    let (message, ok, cancel) = match app.try_state::<Tray>() {
        Some(t) => {
            let p = t.prompt.lock().unwrap_or_else(|e| e.into_inner());
            (p.message.clone(), p.ok.clone(), p.cancel.clone())
        }
        None => (
            "还有正在运行的 Agent，退出会中断它们。确定退出观澜吗？".into(),
            "退出观澜".into(),
            "取消".into(),
        ),
    };
    let handle = app.clone();
    app.dialog()
        .message(message)
        .title("Tidewatch")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(ok, cancel))
        .show(move |yes| {
            if yes {
                exit(&handle);
            }
        });
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
    } else {
        api.prevent_close();
        quit(window.app_handle());
    }
}

#[tauri::command]
pub fn tray_labels(
    app: AppHandle,
    tray: tauri::State<Tray>,
    show: String,
    quit: String,
    tooltip: String,
    quit_message: String,
    cancel: String,
) -> Result<(), String> {
    {
        let mut p = tray.prompt.lock().unwrap_or_else(|e| e.into_inner());
        p.message = quit_message;
        p.ok = quit.clone();
        p.cancel = cancel;
    }
    tray.show.set_text(show).map_err(|e| e.to_string())?;
    tray.quit.set_text(quit).map_err(|e| e.to_string())?;
    match app.tray_by_id("main") {
        Some(icon) => icon.set_tooltip(Some(tooltip)).map_err(|e| e.to_string()),
        None => Ok(()),
    }
}
