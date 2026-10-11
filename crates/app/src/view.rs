//! The document canvas: paints laid-out pages (virtualised to what is on
//! screen), debug overlays, caret and selection, and turns pointer and
//! keyboard input into editor commands.

use std::collections::HashMap;
use std::sync::Arc;

use editor::{Editor, Move, Pos};
use egui::{Color32, Event, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use layout::{DocLayout, Item, Layouter};
use reconstruction::PageAnalysis;
use render::GlyphCache;
use render::gpu::{self, Atlas, View};

use crate::worker::{Request, Worker};

/// Gap between pages, in document points.
pub const PAGE_GAP: f32 = 24.0;
pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 6.0;

#[derive(Clone, Copy, PartialEq)]
pub struct Overlays {
    pub reconstructed: bool,
    pub original: bool,
    pub original_opacity: f32,
    pub diff: bool,
    pub glyphs: bool,
    pub words: bool,
    pub lines: bool,
    pub blocks: bool,
    pub zones: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Self {
            reconstructed: true,
            original: false,
            original_opacity: 0.5,
            diff: false,
            glyphs: false,
            words: false,
            lines: false,
            blocks: false,
            zones: false,
        }
    }
}

/// An in-progress mouse drag on an object (preview only; applied on release).
enum Drag {
    /// Resizing the selected image from its bottom-right corner.
    Image { section: usize, path: Vec<u32>, page: usize, rect: document::Rect, width: f32 },
    /// Moving table column boundary `boundary` (index into `cols`).
    Column { section: usize, path: Vec<u32>, page: usize, cols: Vec<f32>, boundary: usize, x: f32, y0: f32, y1: f32 },
}

/// Grab distance for column borders and resize handles, in screen points.
const GRAB: f32 = 4.0;

struct Bitmap {
    scale: f32,
    texture: TextureHandle,
    pixels: image::RgbaImage,
}

pub struct DocView {
    pub editor: Editor,
    pub layouter: Layouter,
    pub layout: DocLayout,
    layout_rev: u64,
    pub analyses: Vec<Option<PageAnalysis>>,
    pub worker: Option<Worker>,
    pub zoom: f32,
    pub overlays: Overlays,
    pub diff_scores: HashMap<usize, f32>,
    glyphs: GlyphCache,
    atlas: Atlas,
    atlas_tex: Option<(u64, TextureHandle)>,
    images: HashMap<u64, TextureHandle>,
    path_layers: HashMap<usize, (usize, u32, TextureHandle)>,
    originals: HashMap<u32, Bitmap>,
    requested: HashMap<u32, f32>,
    diffs: HashMap<usize, (u64, u32, TextureHandle)>,
    pending_scroll: Option<Vec2>,
    scroll_to_caret: bool,
    /// Set by ⌘K; the toolbar opens its link editor.
    pub link_requested: bool,
    /// Find results to highlight, and which one is current.
    pub find_highlights: Vec<(usize, std::ops::Range<usize>)>,
    pub find_current: Option<usize>,
    /// Message for the title bar, picked up by the app.
    pub notice: Option<String>,
    pub spellcheck: bool,
    /// Misspelled ranges per paragraph text (keyed by a hash of the text).
    spell_cache: HashMap<u64, Vec<std::ops::Range<usize>>>,
    /// Caret position under the last right-click (for the context menu).
    context_pos: Option<Pos>,
    /// Image selected by clicking it: (section, block path).
    pub selected_object: Option<(usize, Vec<u32>)>,
    /// Set by the toolbar; the app shows the file picker.
    pub insert_image_requested: bool,
    drag: Option<Drag>,
    /// The current mouse press began on an object: text selection ignores it.
    press_on_object: bool,
    /// Page nearest the middle of the viewport (for the status bar).
    pub current_page: usize,
    canvas_width: f32,
    words: Option<(u64, usize)>,
    caret_epoch: f64,
}

impl DocView {
    pub fn new(editor: Editor, worker: Option<Worker>) -> Self {
        let mut layouter = Layouter::new();
        let layout = layouter.layout(&editor.doc);
        let n = editor.doc.sections.len();
        Self {
            layout_rev: editor.revision,
            editor,
            layouter,
            layout,
            analyses: vec![None; n],
            worker,
            zoom: 1.0,
            overlays: Overlays::default(),
            diff_scores: HashMap::new(),
            glyphs: GlyphCache::default(),
            atlas: Atlas::default(),
            atlas_tex: None,
            images: HashMap::new(),
            path_layers: HashMap::new(),
            originals: HashMap::new(),
            requested: HashMap::new(),
            diffs: HashMap::new(),
            pending_scroll: None,
            scroll_to_caret: false,
            link_requested: false,
            find_highlights: Vec::new(),
            find_current: None,
            notice: None,
            spellcheck: true,
            spell_cache: HashMap::new(),
            context_pos: None,
            selected_object: None,
            insert_image_requested: false,
            drag: None,
            press_on_object: false,
            current_page: 0,
            canvas_width: 0.0,
            words: None,
            caret_epoch: 0.0,
        }
    }

    pub fn relayout(&mut self) {
        if self.editor.revision != self.layout_rev {
            self.layout = self.layouter.layout(&self.editor.doc);
            self.layout_rev = self.editor.revision;
        }
    }

    pub fn set_original(&mut self, ctx: &egui::Context, page: u32, scale: f32, pixels: image::RgbaImage) {
        let size = [pixels.width() as usize, pixels.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
        let texture = ctx.load_texture(format!("orig-{page}"), color, TextureOptions::LINEAR);
        self.originals.insert(page, Bitmap { scale, texture, pixels });
    }

    /// Development scripting (`REVISE_SCRIPT`): `click:page:x:y` (page
    /// points), `shiftclick:page:x:y`, `type:text`, `key:Name[+shift|+alt|+cmd]`,
    /// `overlay:original|diff|lines|blocks|zones`, `zoom:factor`.
    pub fn script_step(&mut self, ctx: &egui::Context, step: &str) {
        self.relayout();
        let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
        match cmd {
            "click" | "shiftclick" => {
                let v: Vec<f32> = arg.split(':').filter_map(|s| s.parse().ok()).collect();
                if let [page, x, y] = v[..]
                    && let Some(h) = self.layout.hit(page as usize, x, y)
                {
                    let p = Pos::new(h.para, h.offset);
                    if cmd == "click" { self.editor.set_caret(p) } else { self.editor.extend_to(p) }
                }
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("canvas")));
            }
            "type" => self.editor.insert_text(&arg.replace("\\n", "\n")),
            "key" => {
                let mut parts = arg.split('+');
                let name = parts.next().unwrap_or_default();
                let mut m = Modifiers::NONE;
                for p in parts {
                    match p {
                        "shift" => m.shift = true,
                        "alt" => m.alt = true,
                        "cmd" => m.command = true,
                        _ => {}
                    }
                }
                if let Some(k) = Key::from_name(name) {
                    self.key(k, m);
                }
            }
            "overlay" => match arg {
                "original" => self.overlays.original = true,
                "diff" => self.overlays.diff = true,
                "lines" => self.overlays.lines = true,
                "blocks" => self.overlays.blocks = true,
                "zones" => self.overlays.zones = true,
                _ => {}
            },
            "zoom" => self.zoom = arg.parse().unwrap_or(self.zoom),
            "theme" => ctx.set_theme(if arg == "light" { egui::ThemePreference::Light } else { egui::ThemePreference::Dark }),
            "action" => {
                let a: Vec<&str> = arg.split(':').collect();
                let e = &mut self.editor;
                match a.as_slice() {
                    ["table", r, c] => {
                        let width = self.layout.para(e.sel.focus.para).width;
                        e.insert_table(r.parse().unwrap_or(2), c.parse().unwrap_or(2), width);
                    }
                    ["shade_row"] => e.table_set_shading(Some(document::Rgba::rgb(217, 234, 211)), true),
                    ["col_right"] => e.table_insert_col(true),
                    ["row_below"] => e.table_insert_row(true),
                    ["heading", l] => e.set_block_type(editor::BlockType::Heading(l.parse().unwrap_or(1))),
                    ["rule"] => e.insert_rule(),
                    ["image", path] => {
                        crate::app::insert_image_for_script(self, std::path::Path::new(path));
                        return;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if std::env::var_os("REVISE_TRACE").is_some() {
            eprintln!("step {step:?}: sel {:?} paras {}", self.editor.sel, self.editor.para_count());
        }
        self.relayout();
    }

    /// Scrolls the caret into view on the next frame.
    pub fn reveal_caret(&mut self) {
        self.scroll_to_caret = true;
    }

    /// Returns keyboard focus to the page canvas (after toolbar actions).
    pub fn focus_canvas(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("canvas")));
        self.scroll_to_caret = true;
    }

    /// Zoom so the widest page fills the canvas width.
    pub fn fit_width(&mut self) {
        let w = (0..self.layout.page_count()).map(|i| self.layout.page(i).size.w).fold(0.0, f32::max);
        if w > 0.0 && self.canvas_width > 0.0 {
            self.zoom = ((self.canvas_width - 48.0) / (w + 2.0 * PAGE_GAP)).clamp(MIN_ZOOM, MAX_ZOOM);
        }
    }

    pub fn word_count(&mut self) -> usize {
        let rev = self.editor.revision;
        if let Some((r, n)) = self.words
            && r == rev
        {
            return n;
        }
        let n = (0..self.editor.para_count()).map(|i| self.editor.paragraph(i).text().split_whitespace().count()).sum();
        self.words = Some((rev, n));
        n
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    fn page_rects(&self, width: f32) -> Vec<Rect> {
        let mut y = PAGE_GAP;
        (0..self.layout.page_count())
            .map(|i| {
                let size = self.layout.page(i).size;
                let x = ((width / self.zoom - size.w) * 0.5).max(PAGE_GAP);
                let r = Rect::from_min_size(Pos2::new(x, y) * self.zoom, Vec2::new(size.w, size.h) * self.zoom);
                y += size.h + PAGE_GAP;
                r
            })
            .collect()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.relayout();
        let doc_w = (0..self.layout.page_count()).map(|i| self.layout.page(i).size.w).fold(0.0, f32::max);
        let doc_h: f32 = (0..self.layout.page_count()).map(|i| self.layout.page(i).size.h + PAGE_GAP).sum::<f32>() + PAGE_GAP;
        let content = Vec2::new(doc_w + 2.0 * PAGE_GAP, doc_h) * self.zoom;

        let mut area = egui::ScrollArea::both().auto_shrink(false);
        if let Some(offset) = self.pending_scroll.take() {
            area = area.scroll_offset(offset);
        }
        self.canvas_width = ui.available_width();
        let out = area.show_viewport(ui, |ui, viewport| {
            let size = content.max(ui.available_size());
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let response = ui.interact(rect, egui::Id::new("canvas"), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let origin = response.rect.min;
            let rects: Vec<Rect> = self.page_rects(size.x).into_iter().map(|r| r.translate(origin.to_vec2())).collect();
            if !self.object_pointer(ui, &response, &rects) {
                self.handle_pointer(ui, &response, &rects);
            }
            if response.secondary_clicked()
                && let Some(p) = response.interact_pointer_pos().and_then(|pos| self.hit(&rects, pos))
            {
                let (a, b) = self.editor.sel.ordered();
                if p < a || p > b {
                    self.editor.set_caret(p);
                }
                self.context_pos = Some(p);
                response.request_focus();
            }
            response.context_menu(|ui| self.context_menu(ui));
            self.handle_keys(ui, &response);
            // Input may have changed the page count: paint from fresh geometry.
            self.relayout();
            let rects: Vec<Rect> = self.page_rects(size.x).into_iter().map(|r| r.translate(origin.to_vec2())).collect();
            let clip = viewport.translate(origin.to_vec2()).expand(100.0);
            let mid = viewport.center().y + origin.y;
            self.current_page = rects.iter().position(|r| r.bottom() + PAGE_GAP * self.zoom >= mid).unwrap_or(0);
            for (i, rect) in rects.iter().enumerate() {
                if rect.intersects(clip) {
                    self.paint_page(ui, &painter, i, *rect, clip);
                }
            }
            if std::mem::take(&mut self.scroll_to_caret)
                && let Some((page, r)) = self.layout.caret(self.editor.sel.focus.para, self.editor.sel.focus.offset)
                && let Some(pr) = rects.get(page)
            {
                let view = View { origin: pr.min, zoom: self.zoom, pixels_per_point: 1.0 };
                ui.scroll_to_rect(view.rect(&r).expand(24.0), None);
            }
            self.upload_atlas(ui.ctx());
            response
        });

        // Cmd+scroll / pinch zoom, anchored at the pointer.
        let zoom_delta = ui.input(|i| i.zoom_delta());
        if zoom_delta != 1.0 && out.inner.hovered() {
            let old = self.zoom;
            self.zoom_by(zoom_delta);
            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                let local = pointer - out.inner_rect.min;
                self.pending_scroll = Some((out.state.offset + local) / old * self.zoom - local);
            }
        }
    }

    fn doc_point(&self, rects: &[Rect], p: Pos2) -> Option<(usize, f32, f32)> {
        let i = rects
            .iter()
            .enumerate()
            .min_by(|a, b| dist(a.1, p).total_cmp(&dist(b.1, p)))
            .map(|(i, _)| i)?;
        let local = (p - rects[i].min) / self.zoom;
        Some((i, local.x, local.y))
    }

    fn hit(&self, rects: &[Rect], p: Pos2) -> Option<Pos> {
        let (page, x, y) = self.doc_point(rects, p)?;
        self.layout.hit(page, x, y).map(|h| Pos::new(h.para, h.offset))
    }

    fn handle_pointer(&mut self, ui: &egui::Ui, response: &egui::Response, rects: &[Rect]) {
        let Some(pointer) = response.interact_pointer_pos() else { return };
        let shift = ui.input(|i| i.modifiers.shift);
        if response.triple_clicked() {
            if let Some(p) = self.hit(rects, pointer) {
                self.editor.select_paragraph(p.para);
            }
        } else if response.double_clicked() {
            if let Some(p) = self.hit(rects, pointer) {
                self.editor.select_word_at(p);
            }
        } else if response.drag_started() || response.clicked() || (response.is_pointer_button_down_on() && !response.dragged()) {
            if response.drag_started() || ui.input(|i| i.pointer.any_pressed()) {
                if let Some(p) = self.hit(rects, pointer) {
                    if shift { self.editor.extend_to(p) } else { self.editor.set_caret(p) }
                }
                response.request_focus();
            }
        } else if response.dragged()
            && let Some(p) = self.hit(rects, pointer)
        {
            self.editor.extend_to(p);
        }
        self.caret_epoch = ui.input(|i| i.time);
    }

    /// Images and table borders under the pointer. Returns true when the
    /// pointer event was consumed (so text selection doesn't also react).
    fn object_pointer(&mut self, ui: &egui::Ui, response: &egui::Response, rects: &[Rect]) -> bool {
        let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
        let Some(pointer) = pointer else { return false };
        let Some((page, x, y)) = self.doc_point(rects, pointer) else { return false };
        let grab = GRAB / self.zoom;

        // Continue / finish an active drag.
        if let Some(drag) = &mut self.drag {
            match drag {
                Drag::Image { rect, width, .. } => *width = (x - rect.x0).max(24.0),
                Drag::Column { cols, boundary, x: bx, .. } => {
                    let b = *boundary;
                    let lo = cols[b - 1] + 12.0;
                    let hi = cols.get(b + 1).map_or(f32::MAX, |n| n - 12.0);
                    *bx = x.clamp(lo, hi);
                }
            }
            ui.ctx().set_cursor_icon(match drag {
                Drag::Image { .. } => egui::CursorIcon::ResizeNwSe,
                Drag::Column { .. } => egui::CursorIcon::ResizeColumn,
            });
            if !ui.input(|i| i.pointer.primary_down()) {
                match self.drag.take().expect("drag") {
                    Drag::Image { section, path, width, .. } => self.editor.resize_image(section, &path, width),
                    Drag::Column { section, path, cols, boundary, x, .. } => {
                        let mut xs = cols.clone();
                        xs[boundary] = x;
                        let widths = xs.windows(2).map(|w| w[1] - w[0]).collect();
                        self.editor.table_set_col_widths(section, &path, widths);
                    }
                }
            }
            return true;
        }

        // The selected image's bottom-right handle.
        let handle = self.selected_object.as_ref().and_then(|(s, p)| self.layout.object(*s, p)).filter(|(pg, _)| *pg == page);
        let on_handle = handle.is_some_and(|(_, r)| (x - r.x1).abs() < grab * 2.0 && (y - r.y1).abs() < grab * 2.0);
        // A table column border (not the left edge).
        let border = self.layout.tables_on(page).find_map(|(si, t, lp)| {
            let rows: Vec<_> = t.rows.iter().filter(|r| r.0 == lp).collect();
            let (y0, y1) = (rows.first()?.1, rows.last()?.2);
            if y < y0 || y > y1 {
                return None;
            }
            let b = (1..t.cols.len()).find(|&b| (x - t.cols[b]).abs() < grab)?;
            Some(Drag::Column { section: si, path: t.path.clone(), page, cols: t.cols.clone(), boundary: b, x: t.cols[b], y0, y1 })
        });
        if on_handle {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        } else if border.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }

        let pressed = ui.input(|i| i.pointer.primary_pressed()) && (response.is_pointer_button_down_on() || response.hovered());
        if !pressed {
            if self.press_on_object && ui.input(|i| i.pointer.primary_down()) {
                return true;
            }
            self.press_on_object = false;
            return false;
        }
        self.press_on_object = true;
        if let (true, Some((_, r)), Some((s, p))) = (on_handle, handle, self.selected_object.clone()) {
            self.drag = Some(Drag::Image { section: s, path: p, page, rect: r, width: r.width() });
            return true;
        }
        if let Some(d) = border {
            self.selected_object = None;
            self.drag = Some(d);
            return true;
        }
        if let Some((si, obj)) = self.layout.object_at(page, x, y) {
            self.selected_object = Some((si, obj.path.clone()));
            response.request_focus();
            return true;
        }
        self.selected_object = None;
        self.press_on_object = false;
        false
    }

    fn handle_keys(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if !response.has_focus() {
            return;
        }
        // With an image selected, Delete removes it and Escape deselects.
        if let Some((section, path)) = self.selected_object.clone() {
            let (del, esc) = ui.input(|i| (i.key_pressed(Key::Backspace) || i.key_pressed(Key::Delete), i.key_pressed(Key::Escape)));
            if del {
                self.editor.delete_block(section, &path);
                self.selected_object = None;
                return;
            }
            if esc || ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Text(_)))) {
                self.selected_object = None;
            }
        }
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false },
            )
        });
        let events = ui.input(|i| i.events.clone());
        for ev in events {
            let before = (self.editor.revision, self.editor.sel);
            match ev {
                Event::Text(t) => self.editor.insert_text(&t),
                Event::Ime(egui::ImeEvent::Commit(t)) => self.editor.insert_text(&t),
                Event::Paste(t) => self.editor.paste(&t),
                Event::Copy => {
                    let t = self.editor.copy();
                    ui.ctx().copy_text(t);
                }
                Event::Cut => {
                    let t = self.editor.cut();
                    ui.ctx().copy_text(t);
                }
                Event::Key { key, pressed: true, modifiers, .. } => self.key(key, modifiers),
                _ => {}
            }
            if (self.editor.revision, self.editor.sel) != before {
                self.relayout();
                self.scroll_to_caret = true;
                self.caret_epoch = ui.input(|i| i.time);
            }
        }
    }

    fn key(&mut self, key: Key, m: Modifiers) {
        let e = &mut self.editor;
        let layout = &self.layout;
        let extend = m.shift;
        let mv = |e: &mut Editor, mv: Move| e.move_caret(mv, extend, layout);
        match key {
            Key::ArrowLeft if m.command => mv(e, Move::LineStart),
            Key::ArrowRight if m.command => mv(e, Move::LineEnd),
            Key::ArrowUp if m.command => mv(e, Move::DocStart),
            Key::ArrowDown if m.command => mv(e, Move::DocEnd),
            Key::ArrowLeft if m.alt => mv(e, Move::WordLeft),
            Key::ArrowRight if m.alt => mv(e, Move::WordRight),
            Key::ArrowUp if m.alt => mv(e, Move::ParaStart),
            Key::ArrowDown if m.alt => mv(e, Move::ParaEnd),
            Key::ArrowLeft => mv(e, Move::Left),
            Key::ArrowRight => mv(e, Move::Right),
            Key::ArrowUp => mv(e, Move::Up),
            Key::ArrowDown => mv(e, Move::Down),
            Key::Home => mv(e, if m.command { Move::DocStart } else { Move::LineStart }),
            Key::End => mv(e, if m.command { Move::DocEnd } else { Move::LineEnd }),
            Key::Backspace if m.command => {
                // Delete back to the start of the visual line.
                if e.sel.is_collapsed() {
                    e.move_caret(Move::LineStart, true, layout);
                }
                e.backspace(false);
            }
            Key::Backspace => e.backspace(m.alt),
            Key::Delete => e.delete_forward(m.alt),
            Key::Enter => e.enter(),
            Key::Tab if !m.command => {
                // Tab moves between table cells and nests list items at
                // their start; otherwise it types a tab.
                let at_list_start = e.sel.is_collapsed() && e.sel.focus.offset == 0 && e.current_paragraph_style().list.is_some();
                if !e.table_next_cell(m.shift) {
                    if at_list_start || (m.shift && e.current_paragraph_style().list.is_some()) {
                        e.change_indent(!m.shift);
                    } else if !m.shift {
                        e.insert_text("\t");
                    }
                }
            }
            Key::A if m.command => e.select_all(),
            Key::Z if m.command && m.shift => e.redo(),
            Key::Z if m.command => e.undo(),
            Key::Y if m.command => e.redo(),
            Key::B if m.command => e.toggle_style(|s| s.font.is_bold(), |s, on| s.font.weight = if on { 700 } else { 400 }),
            Key::I if m.command => e.toggle_style(|s| s.font.italic, |s, on| s.font.italic = on),
            Key::U if m.command => e.toggle_style(|s| s.underline, |s, on| s.underline = on),
            Key::X if m.command && m.shift => e.toggle_style(|s| s.strike, |s, on| s.strike = on),
            Key::K if m.command => self.link_requested = true,
            Key::Num7 if m.command && m.shift => e.toggle_list(true),
            Key::Num8 if m.command && m.shift => e.toggle_list(false),
            Key::Num0 if m.command && m.alt => e.set_block_type(editor::BlockType::Normal),
            Key::Num1 if m.command && m.alt => e.set_block_type(editor::BlockType::Heading(1)),
            Key::Num2 if m.command && m.alt => e.set_block_type(editor::BlockType::Heading(2)),
            Key::Num3 if m.command && m.alt => e.set_block_type(editor::BlockType::Heading(3)),
            Key::OpenBracket if m.command => e.change_indent(false),
            Key::CloseBracket if m.command => e.change_indent(true),
            Key::L if m.command && m.shift => e.set_align(document::Align::Left),
            Key::E if m.command && m.shift => e.set_align(document::Align::Center),
            Key::R if m.command && m.shift => e.set_align(document::Align::Right),
            Key::J if m.command && m.shift => e.set_align(document::Align::Justify),
            Key::Backslash if m.command => e.clear_formatting(),
            _ => {}
        }
    }

    fn upload_atlas(&mut self, ctx: &egui::Context) {
        let generation = self.atlas.generation;
        let stale = self.atlas_tex.as_ref().is_none_or(|(g, _)| *g != generation);
        if stale {
            let size = gpu::ATLAS_SIZE as usize;
            let img = egui::ColorImage::new([size, size], vec![Color32::TRANSPARENT; size * size]);
            let tex = ctx.load_texture("glyph-atlas", img, TextureOptions::NEAREST);
            self.atlas_tex = Some((generation, tex));
            self.atlas.dirty = Some((0, 0, gpu::ATLAS_SIZE, gpu::ATLAS_SIZE));
        }
        if let Some((pos, size, pixels)) = self.atlas.take_dirty()
            && let Some((_, tex)) = &mut self.atlas_tex
        {
            let img = egui::ColorImage::new(size, pixels);
            tex.set_partial(pos, img, TextureOptions::NEAREST);
        }
    }

    fn paint_page(&mut self, ui: &egui::Ui, painter: &egui::Painter, i: usize, rect: Rect, clip: Rect) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let view = View { origin: rect.min, zoom: self.zoom, pixels_per_point: ppp };
        // Two soft shadows (ambient + contact) read as paper above the canvas.
        let dark = ui.visuals().dark_mode;
        let ambient = egui::epaint::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 26 }) };
        let contact = egui::epaint::Shadow { offset: [0, 1], blur: 3, spread: 0, color: Color32::from_black_alpha(if dark { 140 } else { 30 }) };
        painter.add(ambient.as_shape(rect, 1.0));
        painter.add(contact.as_shape(rect, 1.0));
        painter.rect_filled(rect, 1.0, Color32::WHITE);

        let page = self.layout.page(i);
        let section = page.section;
        let pending = self.editor.doc.sections[section].pending;
        let source_page = (!page.continuation).then_some(section as u32);
        let want_original = pending || self.overlays.original || self.overlays.diff;
        if let (true, Some(sp)) = (want_original, source_page) {
            self.request_original(sp, (self.zoom * ppp).min(4.0));
        }
        let full_uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

        if let Some(bmp) = source_page.and_then(|sp| self.originals.get(&sp))
            && (pending || (self.overlays.original && !self.overlays.diff))
        {
            let alpha = if pending || !self.overlays.reconstructed { 1.0 } else { self.overlays.original_opacity };
            painter.image(bmp.texture.id(), rect, full_uv, Color32::WHITE.gamma_multiply(alpha));
        }

        if self.overlays.diff && !pending {
            if let Some(tex) = self.diff_texture(&ctx, i) {
                painter.image(tex, rect, full_uv, Color32::WHITE);
            }
        } else if self.overlays.reconstructed && !pending {
            self.paint_content(&ctx, painter, i, view, clip);
        }

        if self.spellcheck && !pending && self.overlays.reconstructed {
            self.paint_spelling(painter, i, view);
        }
        self.paint_selection_and_caret(ui, painter, i, view);
        self.paint_object_ui(painter, i, view);
        self.paint_debug(painter, i, view);
    }

    fn paint_content(&mut self, ctx: &egui::Context, painter: &egui::Painter, i: usize, view: View, clip: Rect) {
        let page = self.layout.page(i);
        let full_uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        // Vector decorations: rasterized once per zoom level.
        if page.items.iter().any(|it| matches!(it, Item::Path(_))) {
            let scale = (view.zoom * view.pixels_per_point).min(4.0);
            let scale_q = (scale * 8.0).round() as u32;
            let key = Arc::as_ptr(&self.layout.sections[page.section]) as usize;
            let fresh = self.path_layers.get(&i).is_some_and(|(k, s, _)| *k == key && *s == scale_q);
            if !fresh {
                let mut only_paths = page.clone();
                only_paths.items.retain(|it| matches!(it, Item::Path(_)));
                let layers = render::raster::Layers { text: false, graphics: true, background: false };
                let img = render::raster::render_page(&only_paths, scale_q as f32 / 8.0, &mut self.glyphs, &layers);
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                let tex = ctx.load_texture(format!("paths-{i}"), color, TextureOptions::LINEAR);
                self.path_layers.insert(i, (key, scale_q, tex));
            }
            if let Some((_, _, tex)) = self.path_layers.get(&i) {
                painter.image(tex.id(), view.rect(&document::Rect::new(0.0, 0.0, page.size.w, page.size.h)), full_uv, Color32::WHITE);
            }
        }
        for item in &page.items {
            if let Item::Image { rect, image } = item {
                let tex = self.images.entry(image.id).or_insert_with(|| {
                    let color = render::raster::decode(image).map_or_else(
                        || egui::ColorImage::new([1, 1], vec![Color32::LIGHT_GRAY]),
                        |pm| {
                            let px = pm.pixels().iter().map(|p| Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha())).collect();
                            egui::ColorImage::new([pm.width() as usize, pm.height() as usize], px)
                        },
                    );
                    ctx.load_texture(format!("img-{}", image.id), color, TextureOptions::LINEAR)
                });
                painter.image(tex.id(), view.rect(rect), full_uv, Color32::WHITE);
            }
        }
        painter.add(gpu::rect_mesh(page, view));
        let tex = self.atlas_tex.as_ref().map(|(_, t)| t.id()).unwrap_or_default();
        match gpu::text_mesh(page, view, &mut self.atlas, &mut self.glyphs, tex, clip) {
            Some(mesh) => {
                painter.add(mesh);
            }
            None => ctx.request_repaint(),
        }
    }

    fn paint_selection_and_caret(&self, ui: &egui::Ui, painter: &egui::Painter, i: usize, view: View) {
        for (k, (para, range)) in self.find_highlights.iter().enumerate() {
            if *para >= self.layout.para_count() {
                continue;
            }
            let fill = if self.find_current == Some(k) {
                Color32::from_rgba_unmultiplied(255, 150, 0, 120)
            } else {
                Color32::from_rgba_unmultiplied(255, 214, 10, 90)
            };
            for (page, r) in self.layout.selection_rects(*para, range.clone()) {
                if page == i {
                    painter.rect_filled(view.rect(&r), 2.0, fill);
                }
            }
        }
        let sel = self.editor.sel;
        let (a, b) = sel.ordered();
        if !sel.is_collapsed() {
            let fill = Color32::from_rgba_unmultiplied(66, 133, 244, 64);
            for para in a.para..=b.para.min(self.layout.para_count().saturating_sub(1)) {
                let pl = self.layout.para(para);
                let on_page = pl.lines.iter().any(|l| self.layout.line_page(para, l) == i);
                if !on_page {
                    continue;
                }
                let len = self.editor.paragraph(para).len();
                let from = if para == a.para { a.offset } else { 0 };
                let to = if para == b.para { b.offset } else { len + 1 };
                for (page, r) in self.layout.selection_rects(para, from..to) {
                    if page == i {
                        painter.rect_filled(view.rect(&r), 0.0, fill);
                    }
                }
            }
        }
        let focused = ui.memory(|m| m.focused().is_some());
        if focused && sel.focus.para < self.layout.para_count()
            && let Some((page, r)) = self.layout.caret(sel.focus.para, sel.focus.offset)
            && page == i
        {
            let t = ui.input(|i| i.time) - self.caret_epoch;
            if (t % 1.06) < 0.62 {
                let r = view.rect(&r);
                painter.line_segment([r.min, r.max], Stroke::new(2.0, Color32::from_rgb(24, 24, 28)));
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn paint_object_ui(&self, painter: &egui::Painter, i: usize, view: View) {
        let accent = Color32::from_rgb(38, 110, 235);
        if let Some((s, p)) = &self.selected_object
            && let Some((page, r)) = self.layout.object(*s, p)
            && page == i
        {
            let r = view.rect(&r);
            painter.rect_stroke(r, 0.0, Stroke::new(2.0, accent), egui::StrokeKind::Outside);
            for c in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
                let h = Rect::from_center_size(c, Vec2::splat(9.0));
                painter.rect_filled(h, 2.0, Color32::WHITE);
                painter.rect_stroke(h, 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Inside);
            }
        }
        match &self.drag {
            Some(Drag::Image { page, rect, width, .. }) if *page == i => {
                let h = rect.height() * width / rect.width().max(1.0);
                let r = view.rect(&document::Rect::from_xywh(rect.x0, rect.y0, *width, h));
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent.gamma_multiply(0.8)), egui::StrokeKind::Middle);
            }
            Some(Drag::Column { page, x, y0, y1, .. }) if *page == i => {
                painter.line_segment([view.to_screen(*x, *y0), view.to_screen(*x, *y1)], Stroke::new(2.0, accent));
            }
            _ => {}
        }
    }

    fn misspelled(&mut self, para: usize) -> Vec<std::ops::Range<usize>> {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let p = self.editor.paragraph(para);
        let text = p.text();
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        let found = self.spell_cache.entry(h.finish()).or_insert_with(|| crate::spell::misspellings(&text)).clone();
        // Links, email addresses and web addresses aren't words.
        found
            .into_iter()
            .filter(|r| {
                let word = &text[r.clone()];
                let around = text[..r.start].rsplit(char::is_whitespace).next().unwrap_or("").to_string()
                    + text[r.end..].split(char::is_whitespace).next().unwrap_or("");
                let domain = around.as_bytes().windows(2).any(|w| w[0] == b'.' && w[1].is_ascii_alphabetic());
                p.style_at(r.start + 1).link.is_none() && !word.contains(['@', '/']) && !around.contains(['@', '/']) && !domain
            })
            .collect()
    }

    /// Red wavy underlines under misspelled words (not the word being typed).
    fn paint_spelling(&mut self, painter: &egui::Painter, i: usize, view: View) {
        let caret = self.editor.sel.is_collapsed().then_some(self.editor.sel.focus);
        let paras: Vec<usize> = (0..self.layout.para_count().min(self.editor.para_count()))
            .filter(|&p| self.layout.para(p).lines.iter().any(|l| self.layout.line_page(p, l) == i))
            .collect();
        let red = Color32::from_rgb(230, 60, 60);
        for para in paras {
            for range in self.misspelled(para) {
                if caret.is_some_and(|c| c.para == para && c.offset == range.end) {
                    continue;
                }
                for (page, r) in self.layout.selection_rects(para, range.clone()) {
                    if page != i {
                        continue;
                    }
                    let r = view.rect(&r);
                    let y = r.bottom() - 1.5 * view.zoom.max(0.5);
                    let step = 2.0;
                    let pts: Vec<Pos2> = (0..=((r.width() / step) as usize))
                        .map(|k| Pos2::new(r.left() + k as f32 * step, y + if k % 2 == 0 { 0.0 } else { 1.6 }))
                        .collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(1.1, red)));
                }
            }
        }
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        if let Some(p) = self.context_pos.filter(|p| self.spellcheck && p.para < self.editor.para_count()) {
            let text = self.editor.paragraph(p.para).text();
            let bad = self.misspelled(p.para).into_iter().find(|r| r.start <= p.offset && p.offset <= r.end);
            let e = &mut self.editor;
            if let Some(range) = bad {
                let word = text[range.clone()].to_string();
                let guesses = crate::spell::suggestions(&word);
                if guesses.is_empty() {
                    ui.add_enabled(false, egui::Button::new("No suggestions"));
                }
                for g in guesses {
                    if ui.button(RichText::new(&g).strong()).clicked() {
                        e.select_match(&editor::Match { para: p.para, range: range.clone() });
                        e.insert_text(&g);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Learn Spelling").clicked() {
                    crate::spell::learn(&word);
                    self.spell_cache.clear();
                    ui.close();
                }
                if ui.button("Ignore Spelling").clicked() {
                    crate::spell::ignore(&word);
                    self.spell_cache.clear();
                    ui.close();
                }
                ui.separator();
            }
        }
        let e = &mut self.editor;
        let has_sel = !e.sel.is_collapsed();
        if ui.add_enabled(has_sel, egui::Button::new("Cut")).clicked() {
            let t = e.cut();
            ui.ctx().copy_text(t);
            ui.close();
        }
        if ui.add_enabled(has_sel, egui::Button::new("Copy")).clicked() {
            let t = e.copy();
            ui.ctx().copy_text(t);
            ui.close();
        }
        if ui.button("Paste").clicked() {
            if let Some(t) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()) {
                e.paste(&t);
            }
            ui.close();
        }
        if e.table_at_caret().is_some() {
            ui.separator();
            if e.can_merge_cells() && ui.button("Merge Cells").clicked() {
                e.table_merge_cells();
                ui.close();
            }
            if e.can_split_cell() && ui.button("Unmerge Cells").clicked() {
                e.table_split_cell();
                ui.close();
            }
            #[allow(clippy::type_complexity)]
            let items: [(&str, fn(&mut editor::Editor)); 7] = [
                ("Insert Row Above", |e| e.table_insert_row(false)),
                ("Insert Row Below", |e| e.table_insert_row(true)),
                ("Insert Column Left", |e| e.table_insert_col(false)),
                ("Insert Column Right", |e| e.table_insert_col(true)),
                ("Delete Row", |e| e.table_delete_row()),
                ("Delete Column", |e| e.table_delete_col()),
                ("Delete Table", |e| e.table_delete()),
            ];
            for (label, f) in items {
                if ui.button(label).clicked() {
                    f(e);
                    ui.close();
                }
            }
        }
    }

    fn paint_debug(&self, painter: &egui::Painter, i: usize, view: View) {
        let o = self.overlays;
        if !(o.glyphs || o.words || o.lines || o.blocks || o.zones) {
            return;
        }
        let page = self.layout.page(i);
        if page.continuation {
            return;
        }
        let Some(Some(a)) = self.analyses.get(page.section) else { return };
        let draw = |rects: &mut dyn Iterator<Item = &document::Rect>, color: Color32| {
            for r in rects {
                painter.rect_stroke(view.rect(r), 0.0, Stroke::new(1.0, color), egui::StrokeKind::Middle);
            }
        };
        if o.glyphs {
            draw(&mut a.glyphs.iter(), Color32::from_rgba_unmultiplied(0, 160, 0, 120));
        }
        if o.words {
            draw(&mut a.words.iter(), Color32::from_rgba_unmultiplied(0, 90, 255, 160));
        }
        if o.lines {
            draw(&mut a.lines.iter(), Color32::from_rgba_unmultiplied(255, 140, 0, 200));
        }
        if o.blocks {
            for (r, kind) in &a.blocks {
                let c = match *kind {
                    "rule" => Color32::from_rgb(150, 0, 150),
                    "image" => Color32::from_rgb(0, 150, 150),
                    _ => Color32::from_rgb(220, 0, 60),
                };
                painter.rect_stroke(view.rect(&r.inflate(1.5)), 0.0, Stroke::new(1.2, c), egui::StrokeKind::Middle);
            }
        }
        if o.zones {
            for (r, kind) in &a.zones {
                let c = match *kind {
                    "columns" | "column" => Color32::from_rgb(0, 170, 90),
                    "table" => Color32::from_rgb(170, 90, 0),
                    _ => Color32::from_rgba_unmultiplied(120, 120, 120, 160),
                };
                let sr = view.rect(&r.inflate(4.0));
                painter.rect_stroke(sr, 2.0, Stroke::new(1.5, c), egui::StrokeKind::Middle);
                painter.text(sr.left_top(), egui::Align2::LEFT_BOTTOM, *kind, egui::FontId::monospace(10.0), c);
            }
        }
    }

    fn request_original(&mut self, page: u32, scale: f32) {
        let have = self.originals.get(&page).map(|b| b.scale).or(self.requested.get(&page).copied());
        if have.is_none_or(|s| (s / scale - 1.0).abs() > 0.15)
            && let Some(w) = &self.worker
        {
            self.requested.insert(page, scale);
            w.request(Request::Render { page, scale });
        }
    }

    /// Original-vs-reconstruction difference overlay for a page.
    fn diff_texture(&mut self, ctx: &egui::Context, i: usize) -> Option<egui::TextureId> {
        let page = self.layout.page(i);
        let orig = self.originals.get(&(page.section as u32))?;
        let scale_q = (orig.scale * 8.0).round() as u32;
        if let Some((rev, s, tex)) = self.diffs.get(&i)
            && *rev == self.layout_rev
            && *s == scale_q
        {
            return Some(tex.id());
        }
        let recon = render::raster::render_page(page, orig.scale, &mut self.glyphs, &render::raster::ALL);
        let (img, score) = render::raster::diff(&orig.pixels, &recon);
        self.diff_scores.insert(i, score);
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let tex = ctx.load_texture(format!("diff-{i}"), color, TextureOptions::LINEAR);
        let id = tex.id();
        self.diffs.insert(i, (self.layout_rev, scale_q, tex));
        Some(id)
    }
}

fn dist(r: &Rect, p: Pos2) -> f32 {
    let dx = (r.min.x - p.x).max(p.x - r.max.x).max(0.0);
    let dy = (r.min.y - p.y).max(p.y - r.max.y).max(0.0);
    dx + dy
}
