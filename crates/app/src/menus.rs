//! Native macOS menu bar. Menu items map to [`Command`]s that the app runs
//! exactly like its buttons and shortcuts.

use std::collections::HashMap;
use std::sync::Mutex;

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::app::Command;

static CLICKED: Mutex<Vec<MenuId>> = Mutex::new(Vec::new());

pub struct NativeMenu {
    _menu: Menu,
    commands: HashMap<MenuId, Command>,
}

const CMD: Modifiers = Modifiers::META;
const CMD_SHIFT: Modifiers = Modifiers::META.union(Modifiers::SHIFT);
const CMD_ALT: Modifiers = Modifiers::META.union(Modifiers::ALT);

impl NativeMenu {
    pub fn install(ctx: &egui::Context) -> NativeMenu {
        let mut commands = HashMap::new();
        let mut item = |label: &str, cmd: Command, key: Option<(Modifiers, Code)>| {
            let it = MenuItem::new(label, true, key.map(|(m, c)| Accelerator::new(m, c)));
            commands.insert(it.id().clone(), cmd);
            it
        };
        let sep = PredefinedMenuItem::separator;

        let app = Submenu::with_items(
            "Revise",
            true,
            &[
                &PredefinedMenuItem::about(
                    Some("About Revise"),
                    Some(AboutMetadata { name: Some("Revise".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() }),
                ),
                &item("Check for Updates…", Command::CheckForUpdates, None),
                &sep(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &sep(),
                &item("Quit Revise", Command::Quit, Some((CMD, Code::KeyQ))),
            ],
        )
        .expect("app menu");
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &item("Open…", Command::Open, Some((CMD, Code::KeyO))),
                &sep(),
                &item("Save", Command::Save, Some((CMD, Code::KeyS))),
                &item("Save As…", Command::SaveAs, Some((CMD_SHIFT, Code::KeyS))),
                &item("Revert to Original…", Command::RevertToOriginal, None),
                &sep(),
                &item("Page Setup…", Command::PageSetup, Some((CMD_SHIFT, Code::KeyP))),
                &sep(),
                &PredefinedMenuItem::close_window(None),
            ],
        )
        .expect("file menu");
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &item("Undo", Command::Undo, Some((CMD, Code::KeyZ))),
                &item("Redo", Command::Redo, Some((CMD_SHIFT, Code::KeyZ))),
                &sep(),
                &item("Cut", Command::Cut, Some((CMD, Code::KeyX))),
                &item("Copy", Command::Copy, Some((CMD, Code::KeyC))),
                &item("Paste", Command::Paste, Some((CMD, Code::KeyV))),
                &item("Select All", Command::SelectAll, Some((CMD, Code::KeyA))),
                &sep(),
                &sep(),
                &item("Find…", Command::Find, Some((CMD, Code::KeyF))),
                &item("Find and Replace…", Command::Replace, Some((CMD_ALT, Code::KeyF))),
                &item("Find Next", Command::FindNext, Some((CMD, Code::KeyG))),
                &item("Find Previous", Command::FindPrevious, Some((CMD_SHIFT, Code::KeyG))),
                &sep(),
                &item("Add Link…", Command::Link, Some((CMD, Code::KeyK))),
                &sep(),
                &item("Check Spelling While Typing", Command::ToggleSpelling, None),
            ],
        )
        .expect("edit menu");
        let format = Submenu::with_items(
            "Format",
            true,
            &[
                &item("Bold", Command::Bold, Some((CMD, Code::KeyB))),
                &item("Italic", Command::Italic, Some((CMD, Code::KeyI))),
                &item("Underline", Command::Underline, Some((CMD, Code::KeyU))),
                &item("Strikethrough", Command::Strike, Some((CMD_SHIFT, Code::KeyX))),
                &sep(),
                &item("Normal Text", Command::Heading(0), Some((CMD_ALT, Code::Digit0))),
                &item("Heading 1", Command::Heading(1), Some((CMD_ALT, Code::Digit1))),
                &item("Heading 2", Command::Heading(2), Some((CMD_ALT, Code::Digit2))),
                &item("Heading 3", Command::Heading(3), Some((CMD_ALT, Code::Digit3))),
                &sep(),
                &item("Bulleted List", Command::Bullets, Some((CMD_SHIFT, Code::Digit8))),
                &item("Numbered List", Command::Numbers, Some((CMD_SHIFT, Code::Digit7))),
                &item("Increase Indent", Command::Indent, Some((CMD, Code::BracketRight))),
                &item("Decrease Indent", Command::Outdent, Some((CMD, Code::BracketLeft))),
                &sep(),
                &item("Align Left", Command::Align(document::Align::Left), Some((CMD_SHIFT, Code::KeyL))),
                &item("Center", Command::Align(document::Align::Center), Some((CMD_SHIFT, Code::KeyE))),
                &item("Align Right", Command::Align(document::Align::Right), Some((CMD_SHIFT, Code::KeyR))),
                &item("Justify", Command::Align(document::Align::Justify), Some((CMD_SHIFT, Code::KeyJ))),
                &sep(),
                &item("Insert Horizontal Line", Command::InsertRule, None),
                &item("Clear Formatting", Command::ClearFormatting, Some((CMD, Code::Backslash))),
            ],
        )
        .expect("format menu");
        let insert = Submenu::with_items(
            "Insert",
            true,
            &[
                &item("Image…", Command::InsertImage, None),
                &item("Table", Command::InsertTable, None),
                &item("Horizontal Line", Command::InsertRule, None),
                &item("Link…", Command::Link, None),
            ],
        )
        .expect("insert menu");
        let view = Submenu::with_items(
            "View",
            true,
            &[
                &item("Zoom In", Command::ZoomIn, Some((CMD, Code::Equal))),
                &item("Zoom Out", Command::ZoomOut, Some((CMD, Code::Minus))),
                &item("Actual Size", Command::ActualSize, Some((CMD, Code::Digit0))),
                &item("Fit Width", Command::FitWidth, Some((CMD, Code::Digit9))),
                &sep(),
                &item("Reconstruction Inspector", Command::ToggleInspector, Some((CMD_ALT, Code::KeyI))),
                &sep(),
                &Submenu::with_items(
                    "Appearance",
                    true,
                    &[
                        &item("Match System", Command::Appearance(egui::ThemePreference::System), None),
                        &item("Light", Command::Appearance(egui::ThemePreference::Light), None),
                        &item("Dark", Command::Appearance(egui::ThemePreference::Dark), None),
                    ],
                )
                .expect("appearance menu"),
                &sep(),
                &PredefinedMenuItem::fullscreen(None),
            ],
        )
        .expect("view menu");
        let window = Submenu::with_items(
            "Window",
            true,
            &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::maximize(None), &sep(), &PredefinedMenuItem::bring_all_to_front(None)],
        )
        .expect("window menu");

        let menu = Menu::with_items(&[&app, &file, &edit, &insert, &format, &view, &window]).expect("menu bar");
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();

        let repaint = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Ok(mut q) = CLICKED.lock() {
                q.push(e.id);
            }
            repaint.request_repaint();
        }));
        NativeMenu { _menu: menu, commands }
    }

    /// Commands chosen from the menu bar since the last call.
    pub fn take(&self) -> Vec<Command> {
        let ids = CLICKED.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        ids.iter().filter_map(|id| self.commands.get(id).copied()).collect()
    }
}
