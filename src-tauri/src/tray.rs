//! Tray icon: Show / Hide, Workspaces ▸, Monitor ▸, Settings…, Quit.
//! Left-click toggles the deck. The menu is rebuilt whenever workspaces or monitors change.

use std::sync::Mutex;

use serde::Deserialize;
use serde_json::json;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, Wry};

use crate::window;

const TRAY_ID: &str = "deck-tray";
const ID_SHOW_HIDE: &str = "show-hide";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";
const WS_PREFIX: &str = "ws:";
const MON_PREFIX: &str = "mon:";
pub const EVENT_WORKSPACE_SWITCH: &str = "workspace://switch";

#[derive(Debug, Clone, Deserialize)]
pub struct TrayWorkspace {
    pub id: String,
    pub name: String,
}

#[derive(Default)]
pub struct TrayState(Mutex<(Vec<TrayWorkspace>, String)>);

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let (workspaces, active) = app
        .state::<TrayState>()
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    let ws_menu = Submenu::new(app, "Workspaces", true)?;
    if workspaces.is_empty() {
        ws_menu.append(&MenuItem::new(app, "(none)", false, None::<&str>)?)?;
    }
    for ws in &workspaces {
        ws_menu.append(&CheckMenuItem::with_id(
            app,
            format!("{WS_PREFIX}{}", ws.id),
            &ws.name,
            true,
            ws.id == active,
            None::<&str>,
        )?)?;
    }

    let mon_menu = Submenu::new(app, "Monitor", true)?;
    let current = app.state::<window::Placement>().current().map(|m| m.name);
    let monitors = window::deck(app)
        .and_then(|w| w.available_monitors().ok())
        .unwrap_or_default();
    for (i, m) in monitors.iter().enumerate() {
        let name = m.name().cloned().unwrap_or_else(|| format!("monitor-{i}"));
        let size = m.size();
        let label = format!(
            "{} ({}×{})",
            name.trim_start_matches(r"\\.\"),
            size.width,
            size.height
        );
        mon_menu.append(&CheckMenuItem::with_id(
            app,
            format!("{MON_PREFIX}{name}"),
            label,
            true,
            current.as_deref() == Some(name.as_str()),
            None::<&str>,
        )?)?;
    }

    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, ID_SHOW_HIDE, "Show / Hide", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &ws_menu,
            &mon_menu,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, ID_SETTINGS, "Settings…", true, None::<&str>)?,
            &MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?,
        ],
    )
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let id = event.id.as_ref();
    match id {
        ID_SHOW_HIDE => window::toggle(app),
        ID_SETTINGS => {
            let _ = crate::settings_window::open(app);
        }
        ID_QUIT => crate::startup::quit(app),
        _ => {
            if let Some(ws) = id.strip_prefix(WS_PREFIX) {
                let _ = app.emit(EVENT_WORKSPACE_SWITCH, json!({ "id": ws }));
                app.state::<TrayState>()
                    .0
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .1 = ws.to_string();
                refresh(app);
            } else if let Some(name) = id.strip_prefix(MON_PREFIX) {
                if let Err(e) = window::select(app, name) {
                    tracing::warn!("tray monitor select failed: {e}");
                }
                // Restore check marks even if nothing moved.
                refresh(app);
            }
        }
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Kotilo Deck")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::toggle(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Rebuilds the tray menu (workspace / monitor lists and check marks).
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                tracing::warn!("cannot update tray menu: {e}");
            }
        }
        Err(e) => tracing::warn!("cannot build tray menu: {e}"),
    }
}

#[tauri::command]
pub async fn tray_set_workspaces(
    app: AppHandle,
    state: State<'_, TrayState>,
    items: Vec<TrayWorkspace>,
    active: String,
) -> Result<(), String> {
    *state.0.lock().unwrap_or_else(|p| p.into_inner()) = (items, active);
    refresh(&app);
    Ok(())
}
