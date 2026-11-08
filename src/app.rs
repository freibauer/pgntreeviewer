// PGN Tree Viewer app: board with last-move arrow plus the list of
// continuations at the current position (move / number of next-level
// continuations / evaluation). Shared by the desktop binary (main.rs) and the
// Android library (lib.rs).

use crate::pgn;
use eframe::egui;
use egui::{Color32, ColorImage, FontId, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use shakmaty::fen::Fen;
use shakmaty::{Color, EnPassantMode, File, Position, Rank, Role, Square};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PIECES: &[(&str, &[u8])] = &[
    ("wK", include_bytes!("../assets/pieces/wK.png")),
    ("wQ", include_bytes!("../assets/pieces/wQ.png")),
    ("wR", include_bytes!("../assets/pieces/wR.png")),
    ("wB", include_bytes!("../assets/pieces/wB.png")),
    ("wN", include_bytes!("../assets/pieces/wN.png")),
    ("wP", include_bytes!("../assets/pieces/wP.png")),
    ("bK", include_bytes!("../assets/pieces/bK.png")),
    ("bQ", include_bytes!("../assets/pieces/bQ.png")),
    ("bR", include_bytes!("../assets/pieces/bR.png")),
    ("bB", include_bytes!("../assets/pieces/bB.png")),
    ("bN", include_bytes!("../assets/pieces/bN.png")),
    ("bP", include_bytes!("../assets/pieces/bP.png")),
];

const BAR: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
const BAR_TEXT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const LIST_BG: Color32 = Color32::WHITE;
const LIST_TEXT: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
const LIST_LINE: Color32 = Color32::from_rgb(0xdd, 0xdd, 0xdd);
const LIST_SELECTED: Color32 = Color32::from_rgb(0xe4, 0xee, 0xfb);
const ARROW: Color32 = Color32::from_rgba_premultiplied(0xe6, 0xdc, 0x28, 0xe6);
const LIGHT_SQ: Color32 = Color32::from_rgb(0xf0, 0xd9, 0xb5);
const DARK_SQ: Color32 = Color32::from_rgb(0xd9, 0x84, 0x48);

/// Window/viewport options shared by desktop and Android.
pub fn native_options() -> eframe::NativeOptions {
    // Test aid: PGNVIEWER_PORTRAIT=1 opens a phone-shaped window.
    let size = if std::env::var_os("PGNVIEWER_PORTRAIT").is_some() { [420.0, 900.0] } else { [1200.0, 780.0] };
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PGN Tree Viewer")
            .with_inner_size(size)
            .with_min_inner_size([360.0, 480.0])
            .with_active(true),
        ..Default::default()
    }
}

pub fn run(options: eframe::NativeOptions) -> eframe::Result<()> {
    eframe::run_native(
        "PGN Tree Viewer",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    /// Path of the opened file (desktop only; used for Reload and the dialog's start folder).
    path: Option<PathBuf>,
    /// Display name of the opened file (window title fallback).
    file_name: Option<String>,
    tree: Option<pgn::Tree>,
    current: usize,
    selected: usize,
    flip: bool,
    status: String,
    pieces: HashMap<&'static str, TextureHandle>,
    scroll_to_selected: bool,
    /// Test aids: `PGNVIEWER_SCREENSHOT=out.png` saves a screenshot and exits;
    /// `PGNVIEWER_FILE=x.pgn` preloads a file; `PGNVIEWER_LINE="d4 d5 e4"` opens at that line.
    screenshot: Option<PathBuf>,
    frames: u32,
    /// Android: areas covered by status / navigation bar [left, top, right, bottom] in points.
    insets: [f32; 4],
    show_info: bool,
    /// Short confirmation text (e.g. "FEN copied") and the time until it is shown.
    notice: Option<(String, f64)>,
    /// Name of the last opened PGN (for "Open last PGN").
    last_name: Option<String>,
    /// Desktop: path of the last opened PGN (remembered across starts, never opened automatically).
    last_path: Option<PathBuf>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let mut pieces = HashMap::new();
        for (name, bytes) in PIECES {
            if let Ok(img) = image::load_from_memory(bytes) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let ci = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                pieces.insert(*name, cc.egui_ctx.load_texture(*name, ci, TextureOptions::LINEAR));
            }
        }
        let stored_flip = cc.storage.and_then(|s| s.get_string("flip")).as_deref() == Some("1");
        // Desktop: last opened file, offered via "Open last PGN" (not opened automatically).
        let last_path = cc
            .storage
            .and_then(|s| s.get_string("last_file"))
            .map(PathBuf::from)
            .filter(|p| p.exists());

        // The viewer always starts empty; the user picks the PGN via "Open".
        let mut app = Self {
            path: None,
            file_name: None,
            tree: None,
            current: 0,
            selected: 0,
            flip: stored_flip,
            status: String::new(),
            pieces,
            scroll_to_selected: false,
            screenshot: std::env::var_os("PGNVIEWER_SCREENSHOT").map(PathBuf::from),
            frames: 0,
            insets: [0.0; 4],
            show_info: std::env::var_os("PGNVIEWER_INFO").is_some(),
            notice: None,
            last_name: last_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            last_path,
        };
        // Test aid only (used for screenshots): PGNVIEWER_FILE preloads a file.
        if let Some(p) = std::env::var_os("PGNVIEWER_FILE") {
            app.load(Path::new(&p), false);
        }
        if let (Ok(line), Some(t)) = (std::env::var("PGNVIEWER_LINE"), &app.tree) {
            let path: Vec<String> = line.split_whitespace().map(String::from).collect();
            app.current = t.follow(&path);
        }
        app
    }

    /// (Re)load the PGN. With `keep_position`, the current line is kept as
    /// far as it still exists in the new tree.
    fn load(&mut self, path: &Path, keep_position: bool) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if self.load_text(&text, &name, keep_position) {
                    self.path = Some(path.to_path_buf());
                    self.last_path = Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
                    self.last_name = Some(name);
                }
            }
            Err(e) => self.status = format!("Cannot read {}: {e}", path.display()),
        }
    }

    /// Parse PGN text and show it. Returns false (and keeps the old tree) on a parse error.
    fn load_text(&mut self, text: &str, name: &str, keep_position: bool) -> bool {
        match pgn::parse(text) {
            Ok(tree) => {
                let old_path = match (&self.tree, keep_position) {
                    (Some(t), true) => Some((t.path_of(self.current), self.selected_san())),
                    _ => None,
                };
                self.tree = Some(tree);
                self.current = 0;
                self.selected = 0;
                if let Some((p, sel)) = old_path {
                    let t = self.tree.as_ref().unwrap();
                    self.current = t.follow(&p);
                    if let Some(sel) = sel {
                        self.selected = t.nodes[self.current]
                            .children
                            .iter()
                            .position(|&c| t.nodes[c].san == sel)
                            .unwrap_or(0);
                    }
                }
                self.status.clear();
                self.file_name = Some(name.to_string());
                true
            }
            Err(e) => {
                self.status = format!("PGN error in {name}: {e}");
                false
            }
        }
    }

    #[cfg(target_os = "android")]
    fn open_last(&mut self) {
        if let Err(e) = crate::android::open_last() {
            self.status = format!("Cannot open last PGN: {e}");
        }
    }

    #[cfg(not(target_os = "android"))]
    fn open_last(&mut self) {
        if let Some(p) = self.last_path.clone() {
            self.load(&p, false);
        }
    }

    #[cfg(target_os = "android")]
    fn open_dialog(&mut self) {
        match crate::android::open_picker() {
            Ok(()) => self.status.clear(),
            Err(e) => self.status = format!("Cannot open file picker: {e}"),
        }
    }

    /// Android: pick up a file chosen in the picker, opened with / shared to the app.
    #[cfg(target_os = "android")]
    fn poll_android(&mut self, ctx: &egui::Context) {
        match crate::android::take_picked() {
            Some(Ok((name, text))) => {
                self.path = None;
                self.load_text(&text, &name, false);
            }
            Some(Err(e)) => self.status = e,
            None => {}
        }
        self.last_name = crate::android::last_name();
        match crate::android::insets_px() {
            Ok(px) => {
                let ppp = ctx.pixels_per_point();
                self.insets = px.map(|v| v.max(0) as f32 / ppp);
            }
            Err(e) => self.status = format!("Insets: {e}"),
        }
        // The picker result arrives asynchronously; keep checking while idle.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    #[cfg(not(target_os = "android"))]
    fn open_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .set_title("Open PGN")
            .add_filter("PGN files", &["pgn"])
            .add_filter("All files", &["*"]);
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()).filter(|d| !d.as_os_str().is_empty()) {
            dlg = dlg.set_directory(dir);
        }
        if let Some(p) = dlg.pick_file() {
            self.load(&p, false);
        }
    }

    fn selected_san(&self) -> Option<String> {
        let t = self.tree.as_ref()?;
        let c = *t.nodes[self.current].children.get(self.selected)?;
        Some(t.nodes[c].san.clone())
    }

    fn go_to(&mut self, node: usize) {
        let Some(t) = &self.tree else { return };
        let prev = self.current;
        self.current = node;
        // Going back: pre-select the move we came from.
        self.selected = t.nodes[node].children.iter().position(|&c| c == prev).unwrap_or(0);
        self.scroll_to_selected = true;
    }

    fn back(&mut self) {
        if let Some(p) = self.tree.as_ref().and_then(|t| t.nodes[self.current].parent) {
            self.go_to(p);
        }
    }

    fn forward(&mut self) {
        if let Some(&c) = self.tree.as_ref().and_then(|t| t.nodes[self.current].children.get(self.selected)) {
            self.go_to(c);
        }
    }

    fn to_start(&mut self) {
        if self.tree.is_some() {
            self.go_to(0);
            self.selected = 0;
        }
    }

    fn to_end(&mut self) {
        let Some(t) = &self.tree else { return };
        let mut n = self.current;
        if let Some(&c) = t.nodes[n].children.get(self.selected) {
            n = c;
            while let Some(&c) = t.nodes[n].children.first() {
                n = c;
            }
        }
        self.go_to(n);
    }

    fn select_delta(&mut self, d: i32) {
        let Some(t) = &self.tree else { return };
        let n = t.nodes[self.current].children.len() as i32;
        if n > 0 {
            self.selected = (self.selected as i32 + d).clamp(0, n - 1) as usize;
            self.scroll_to_selected = true;
        }
    }

    fn current_fen(&self) -> Option<String> {
        let t = self.tree.as_ref()?;
        Some(Fen::from_position(&t.nodes[self.current].pos, EnPassantMode::Legal).to_string())
    }

    /// Short confirmation shown for 2 s in the grey info strip.
    fn notify(&mut self, ctx: &egui::Context, text: &str) {
        self.notice = Some((text.to_string(), ctx.input(|i| i.time) + 2.0));
        ctx.request_repaint_after(std::time::Duration::from_millis(2100));
    }

    fn copy_fen(&mut self, ctx: &egui::Context) {
        let Some(fen) = self.current_fen() else { return };
        #[cfg(target_os = "android")]
        if let Err(e) = crate::android::copy_text(&fen) {
            self.status = format!("Cannot copy FEN: {e}");
            return;
        }
        #[cfg(not(target_os = "android"))]
        ctx.copy_text(fen);
        self.notify(ctx, "FEN copied to clipboard");
    }

    fn title(&self) -> String {
        if let Some(t) = &self.tree {
            for key in ["ChapterName", "Event"] {
                if let Some(v) = t.header(key).filter(|v| !v.is_empty() && *v != "?") {
                    return v.to_string();
                }
            }
        }
        self.file_name.clone().unwrap_or_else(|| "PGN Tree Viewer".into())
    }
}

impl App {
    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(out) = self.screenshot.clone() else { return };
        self.frames += 1;
        if self.frames == 8 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                let _ = buf.save(&out);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("flip", if self.flip { "1" } else { "0" }.into());
        if let Some(p) = &self.last_path {
            storage.set_string("last_file", p.to_string_lossy().into_owned());
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_screenshot(&ctx);
        #[cfg(target_os = "android")]
        self.poll_android(&ctx);

        ctx.input(|i| {
            if i.key_pressed(Key::ArrowLeft) {
                self.back();
            }
            if i.key_pressed(Key::BrowserBack) {
                // Android back button/gesture: one move back; at the start position leave the app.
                if self.tree.as_ref().is_some_and(|t| t.nodes[self.current].parent.is_some()) {
                    self.back();
                } else {
                    #[cfg(target_os = "android")]
                    crate::android::move_to_background();
                }
            }
            if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::Enter) {
                self.forward();
            }
            if i.key_pressed(Key::ArrowUp) {
                self.select_delta(-1);
            }
            if i.key_pressed(Key::ArrowDown) {
                self.select_delta(1);
            }
            if i.key_pressed(Key::Home) {
                self.to_start();
            }
            if i.key_pressed(Key::End) {
                self.to_end();
            }
            if i.key_pressed(Key::F) {
                self.flip = !self.flip;
            }
        });

        // ---------------- system bar areas (Android) ----------------
        // The native surface covers the whole screen; keep the parts under the
        // status bar, navigation bar and display cutouts free (dark, like the bars).
        let [il, it, ir, ib] = self.insets;
        let pad = |p: egui::Panel| p.frame(egui::Frame::new().fill(BAR)).resizable(false).show_separator_line(false);
        if it > 0.0 {
            pad(egui::Panel::top("inset_top")).exact_size(it).show(ui, |_| {});
        }
        if ib > 0.0 {
            pad(egui::Panel::bottom("inset_bottom")).exact_size(ib).show(ui, |_| {});
        }
        if il > 0.0 {
            pad(egui::Panel::left("inset_left")).exact_size(il).show(ui, |_| {});
        }
        if ir > 0.0 {
            pad(egui::Panel::right("inset_right")).exact_size(ir).show(ui, |_| {});
        }

        // ---------------- title bar ----------------
        egui::Panel::top("title_bar")
            .frame(egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(8, 6)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_height(48.0);
                    if icon_button(ui, Icon::BackArrow, "Start position (Home)").clicked() {
                        self.to_start();
                    }
                    ui.add_space(10.0);
                    // Leave room for the two icons on the right; shorten long titles with "…".
                    let title_w = (ui.available_width() - 2.0 * 52.0 - 30.0).max(40.0);
                    ui.allocate_ui(Vec2::new(title_w, 48.0), |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(self.title()).color(BAR_TEXT).size(22.0))
                                .truncate(),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let menu = icon_button(ui, Icon::Menu, "Menu");
                        egui::Popup::menu(&menu).show(|ui| {
                            ui.set_min_width(180.0);
                            if ui.button("Open PGN…").clicked() {
                                self.open_dialog();
                            }
                            if let Some(p) = self.path.clone()
                                && ui.button("Reload").clicked()
                            {
                                self.load(&p, true);
                            }
                            ui.separator();
                            if ui.button("Info…").clicked() {
                                self.show_info = true;
                            }
                        });
                        ui.add_space(14.0);
                        if icon_button(ui, Icon::Open, "Open PGN…").clicked() {
                            self.open_dialog();
                        }
                    });
                });
            });

        // ---------------- navigation bar ----------------
        egui::Panel::bottom("nav_bar")
            .frame(egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(8, 10)))
            .show(ui, |ui| {
                // Only backwards navigation: going forward = tapping a move in the list.
                // The two buttons share the full width so they are easy to reach.
                // [Flip] [FEN] [start position] [one move back], sharing the full width.
                let has_tree = self.tree.is_some();
                let can_back = self.tree.as_ref().is_some_and(|t| t.nodes[self.current].parent.is_some());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let size = Vec2::new(ui.available_width() / 4.0, 52.0);
                    if text_button_sized(ui, "Flip", "Flip board (F)", has_tree, size).clicked() {
                        self.flip = !self.flip;
                    }
                    if text_button_sized(ui, "FEN", "Copy the FEN of this position", has_tree, size).clicked() {
                        self.copy_fen(&ctx);
                    }
                    if icon_button_sized(ui, Icon::Start, "Start position (Home)", can_back, size).clicked() {
                        self.to_start();
                    }
                    if icon_button_sized(ui, Icon::Prev, "One move back (←)", can_back, size).clicked() {
                        self.back();
                    }
                });
            });

        // ---------------- info dialog ----------------
        if self.show_info {
            let modal = egui::Modal::new(egui::Id::new("info")).show(&ctx, |ui| {
                ui.set_width(280.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("PGN Tree Viewer").size(22.0).strong());
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(16.0));
                    ui.label(egui::RichText::new(format!("Build {}", env!("BUILD_DATE"))).size(16.0));
                    ui.add_space(12.0);
                    ui.button(egui::RichText::new("OK").size(18.0)).clicked()
                })
                .inner
            });
            if modal.inner || modal.should_close() {
                self.show_info = false;
            }
        }

        // ---------------- board + move list ----------------
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(LIST_BG))
            .show(ui, |ui| {
                let Some(tree) = &self.tree else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(120.0);
                        ui.label(egui::RichText::new("PGN Tree Viewer").size(28.0).color(LIST_TEXT));
                        ui.add_space(20.0);
                        let open = egui::Button::new(egui::RichText::new("Open PGN…").size(20.0))
                            .min_size(Vec2::new(200.0, 44.0));
                        if ui.add(open).clicked() {
                            self.open_dialog();
                        }
                        if let Some(name) = self.last_name.clone() {
                            ui.add_space(12.0);
                            let last = egui::Button::new(egui::RichText::new("Open last PGN").size(20.0))
                                .min_size(Vec2::new(200.0, 44.0));
                            if ui.add(last).on_hover_text(&name).clicked() {
                                self.open_last();
                            }
                            ui.label(egui::RichText::new(name).size(14.0).color(Color32::GRAY));
                        }
                        if !self.status.is_empty() {
                            ui.add_space(12.0);
                            ui.colored_label(Color32::from_rgb(0xc0, 0x30, 0x30), &self.status);
                        }
                    });
                    return;
                };

                // Landscape (PC): board left, continuations right.
                // Portrait (phone): board on top, continuations below.
                let full = ui.available_rect_before_wrap();
                let portrait = full.height() > full.width();
                let (board_rect, right) = if portrait {
                    let pad = 4.0;
                    let board = (full.width() - 2.0 * pad).min(full.height() * 0.6).max(120.0).floor();
                    let board_rect = Rect::from_min_size(
                        Pos2::new(full.center().x - board / 2.0, full.top() + pad),
                        Vec2::splat(board),
                    );
                    let list = Rect::from_min_max(Pos2::new(full.left(), board_rect.bottom() + pad), full.max);
                    (board_rect, list)
                } else {
                    let pad = 16.0;
                    let board = (full.height() - 2.0 * pad).min(full.width() * 0.6 - 2.0 * pad).max(160.0).floor();
                    let board_rect = Rect::from_min_size(full.min + Vec2::splat(pad), Vec2::splat(board));
                    let list = Rect::from_min_max(Pos2::new(board_rect.right() + pad, full.top() + pad), full.max);
                    (board_rect, list)
                };
                let node = &tree.nodes[self.current];
                let painter = ui.painter_at(full);
                draw_board(&painter, board_rect, &node.pos, self.flip, &self.pieces);
                if let Some((from, to)) = node.from_to {
                    draw_arrow(&painter, board_rect, from, to, self.flip);
                }
                let mut clicked = None;
                let scroll_to = std::mem::take(&mut self.scroll_to_selected);
                ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {

                // Current line + evaluation of the current position.
                let now = ui.input(|i| i.time);
                let notice = self.notice.as_ref().filter(|(_, until)| now < *until).map(|(t, _)| t.clone());
                let line_text = notice.unwrap_or_else(|| current_line(tree, self.current));
                let eval_text = node.eval.map(|e| e.text()).unwrap_or_else(|| "–".into());
                egui::Frame::new()
                    .fill(Color32::from_rgb(0xf3, 0xf3, 0xf3))
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // Keep the end of the line visible: drop leading moves until it fits.
                            let max_w = ui.available_width() - 80.0;
                            let font = FontId::proportional(14.0);
                            let mut tokens: Vec<&str> = line_text.split(' ').collect();
                            let mut shown = line_text.clone();
                            while tokens.len() > 1
                                && ui.painter().layout_no_wrap(shown.clone(), font.clone(), LIST_TEXT).size().x > max_w
                            {
                                tokens.remove(0);
                                shown = format!("… {}", tokens.join(" "));
                            }
                            ui.label(egui::RichText::new(shown).font(font).color(LIST_TEXT));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(eval_text)
                                        .size(16.0)
                                        .strong()
                                        .color(eval_color(node.eval)),
                                );
                            });
                        });
                    });

                if !self.status.is_empty() {
                    ui.colored_label(Color32::from_rgb(0xc0, 0x30, 0x30), &self.status);
                }

                // Continuations.
                let children = node.children.clone();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if children.is_empty() {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("End of line").size(18.0).color(Color32::GRAY));
                        });
                    }
                    for (i, &c) in children.iter().enumerate() {
                        let resp = move_row(ui, tree, c, i == self.selected, &self.pieces);
                        if resp.clicked() {
                            clicked = Some(c);
                        }
                        if scroll_to && i == self.selected {
                            resp.scroll_to_me(None);
                        }
                    }
                });
                });
                if let Some(c) = clicked {
                    self.go_to(c);
                }
            });
    }
}

/// The moves leading to `node`, e.g. "1.d4 d5 2.e4 dxe4 3.Nc3".
fn current_line(tree: &pgn::Tree, node: usize) -> String {
    let path = tree.path_of(node);
    if path.is_empty() {
        return "Start position".into();
    }
    let mut out = String::new();
    for k in 0..path.len() {
        let n = tree.follow(&path[..=k]);
        let prefix = tree.move_prefix(n);
        if !(prefix.ends_with("...") && k > 0) {
            out.push_str(&prefix);
        }
        out.push_str(&path[k]);
        out.push(' ');
    }
    out.trim_end().to_string()
}

fn eval_color(e: Option<pgn::Eval>) -> Color32 {
    match e {
        Some(pgn::Eval::Cp(v)) if v >= 0.3 => Color32::from_rgb(0x1e, 0x7d, 0x32),
        Some(pgn::Eval::Cp(v)) if v <= -0.3 => Color32::from_rgb(0xb7, 0x1c, 0x1c),
        Some(pgn::Eval::Mate(n)) if n > 0 => Color32::from_rgb(0x1e, 0x7d, 0x32),
        Some(pgn::Eval::Mate(_)) => Color32::from_rgb(0xb7, 0x1c, 0x1c),
        _ => LIST_TEXT,
    }
}

/// One row of the continuation list: "4.Qh5     12     -0.81", with the
/// piece letter drawn as a figurine.
fn move_row(
    ui: &mut egui::Ui,
    tree: &pgn::Tree,
    child: usize,
    selected: bool,
    pieces: &HashMap<&'static str, TextureHandle>,
) -> egui::Response {
    let h = 54.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter_at(rect);
    let bg = if selected {
        LIST_SELECTED
    } else if resp.hovered() {
        Color32::from_gray(0xf6)
    } else {
        LIST_BG
    };
    p.rect_filled(rect, 0.0, bg);
    p.line_segment(
        [Pos2::new(rect.left(), rect.bottom() - 0.5), Pos2::new(rect.right(), rect.bottom() - 0.5)],
        Stroke::new(1.0, LIST_LINE),
    );

    let node = &tree.nodes[child];
    let font = FontId::proportional(26.0);
    let cy = rect.center().y;
    let mut x = rect.left() + 14.0;

    // Move number.
    let g = p.layout_no_wrap(tree.move_prefix(child), font.clone(), LIST_TEXT);
    let gw = g.size().x;
    p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, LIST_TEXT);
    x += gw + 1.0;

    // Figurine (always drawn as a white piece).
    let san = node.san.as_str();
    let mut rest = san;
    if let Some(first) = san.chars().next()
        && "KQRBN".contains(first)
        && let Some(tex) = pieces.get(format!("w{first}").as_str())
    {
        let s = 30.0;
        p.image(
            tex.id(),
            Rect::from_center_size(Pos2::new(x + s / 2.0, cy), Vec2::splat(s)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        x += s;
        rest = &san[1..];
    }
    let g = p.layout_no_wrap(rest.to_string(), font.clone(), LIST_TEXT);
    p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, LIST_TEXT);

    // Number of continuations on the next level (right-aligned).
    let count_x = rect.right() - 150.0;
    let g = p.layout_no_wrap(node.children.len().to_string(), font.clone(), LIST_TEXT);
    p.galley(Pos2::new(count_x - g.size().x, cy - g.size().y / 2.0), g, LIST_TEXT);

    // Evaluation column (right-aligned).
    let eval = node.eval.map(|e| e.text()).unwrap_or_else(|| "–".into());
    let col = eval_color(node.eval);
    let g = p.layout_no_wrap(eval, font, col);
    p.galley(Pos2::new(rect.right() - 16.0 - g.size().x, cy - g.size().y / 2.0), g, col);

    let n = node.children.len();
    let next = format!("{n} continuation{}", if n == 1 { "" } else { "s" });
    resp.on_hover_text(if node.comment.is_empty() { next } else { format!("{next}\n{{ {} }}", node.comment) })
}

fn square_rect(board: Rect, sq: Square, flip: bool) -> Rect {
    let s = board.width() / 8.0;
    let (f, r) = (u32::from(sq.file()) as f32, u32::from(sq.rank()) as f32);
    let (col, row) = if flip { (7.0 - f, r) } else { (f, 7.0 - r) };
    Rect::from_min_size(Pos2::new(board.left() + col * s, board.top() + row * s), Vec2::splat(s))
}

fn draw_board(
    p: &egui::Painter,
    board: Rect,
    pos: &shakmaty::Chess,
    flip: bool,
    pieces: &HashMap<&'static str, TextureHandle>,
) {
    for r in 0..8u32 {
        for f in 0..8u32 {
            let sq = Square::from_coords(File::new(f), Rank::new(r));
            let rect = square_rect(board, sq, flip);
            let is_light = (f + r) % 2 == 1;
            p.rect_filled(rect, 0.0, if is_light { LIGHT_SQ } else { DARK_SQ });
            if let Some(pc) = pos.board().piece_at(sq) {
                let key = format!(
                    "{}{}",
                    if pc.color == Color::White { 'w' } else { 'b' },
                    match pc.role {
                        Role::King => 'K',
                        Role::Queen => 'Q',
                        Role::Rook => 'R',
                        Role::Bishop => 'B',
                        Role::Knight => 'N',
                        Role::Pawn => 'P',
                    }
                );
                if let Some(tex) = pieces.get(key.as_str()) {
                    p.image(
                        tex.id(),
                        rect.shrink(rect.width() * 0.03),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            }
        }
    }
    p.rect_stroke(board, 0.0, Stroke::new(1.5, Color32::from_gray(0x60)), egui::StrokeKind::Outside);
}

fn draw_arrow(p: &egui::Painter, board: Rect, from: Square, to: Square, flip: bool) {
    let a = square_rect(board, from, flip).center();
    let b = square_rect(board, to, flip).center();
    let s = board.width() / 8.0;
    let dir = (b - a).normalized();
    let normal = Vec2::new(-dir.y, dir.x);
    let shaft_w = s * 0.11;
    let head_len = s * 0.42;
    let head_w = s * 0.42;
    let start = a + dir * s * 0.18;
    let tip = b + dir * s * 0.05;
    let neck = tip - dir * head_len;
    let pts = vec![
        start + normal * shaft_w / 2.0,
        neck + normal * shaft_w / 2.0,
        neck + normal * head_w / 2.0,
        tip,
        neck - normal * head_w / 2.0,
        neck - normal * shaft_w / 2.0,
        start - normal * shaft_w / 2.0,
    ];
    let outline = Stroke::new(1.0, Color32::from_rgba_premultiplied(0x6a, 0x60, 0x00, 0xb0));
    // Non-convex outline: fill shaft and head separately.
    p.add(egui::Shape::convex_polygon(vec![pts[0], pts[1], pts[5], pts[6]], ARROW, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(vec![pts[2], pts[3], pts[4]], ARROW, Stroke::NONE));
    p.add(egui::Shape::closed_line(pts, outline));
}

#[derive(Clone, Copy)]
enum Icon {
    BackArrow,
    Menu,
    Open,
    Start,
    Prev,
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
    icon_button_enabled(ui, icon, tip, true)
}

/// Hand-painted white line icons on the dark bars.
fn icon_button_enabled(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool) -> egui::Response {
    icon_button_sized(ui, icon, tip, enabled, Vec2::new(52.0, 48.0))
}

/// Text button on the dark bars (e.g. "FEN", "Eng") with a custom hit area.
fn text_button_sized(ui: &mut egui::Ui, label: &str, tip: &str, enabled: bool, size: Vec2) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let col = if !enabled {
        Color32::from_gray(90)
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        Color32::from_gray(225)
    };
    let p = ui.painter_at(rect);
    let pill = Rect::from_center_size(rect.center(), Vec2::new(64.0, 40.0));
    p.rect_stroke(pill, 20.0, Stroke::new(2.5, col), egui::StrokeKind::Middle);
    p.text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(18.0), col);
    resp.on_hover_text(tip)
}

/// Icon button with a custom hit area (the icon stays centred).
fn icon_button_sized(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool, size: Vec2) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let p = ui.painter_at(rect);
    let col = if !enabled {
        Color32::from_gray(90)
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        Color32::from_gray(225)
    };
    let st = Stroke::new(3.0, col);
    let c = rect.center();
    let chevron = |p: &egui::Painter, cx: f32, left: bool| {
        let d = if left { 1.0 } else { -1.0 };
        p.line_segment([Pos2::new(cx + 4.0 * d, c.y - 9.0), Pos2::new(cx - 5.0 * d, c.y)], st);
        p.line_segment([Pos2::new(cx - 5.0 * d, c.y), Pos2::new(cx + 4.0 * d, c.y + 9.0)], st);
    };
    match icon {
        Icon::BackArrow => {
            p.line_segment([Pos2::new(c.x - 13.0, c.y), Pos2::new(c.x + 13.0, c.y)], st);
            p.line_segment([Pos2::new(c.x - 13.0, c.y), Pos2::new(c.x - 3.0, c.y - 10.0)], st);
            p.line_segment([Pos2::new(c.x - 13.0, c.y), Pos2::new(c.x - 3.0, c.y + 10.0)], st);
        }
        Icon::Menu => {
            for dy in [-9.0, 0.0, 9.0] {
                p.circle_filled(Pos2::new(c.x, c.y + dy), 3.2, col);
            }
        }
        Icon::Open => {
            let r = Rect::from_center_size(c + Vec2::new(0.0, 2.0), Vec2::new(28.0, 20.0));
            let s = Stroke::new(2.5, col);
            p.rect_stroke(r, 2.0, s, egui::StrokeKind::Middle);
            p.line_segment([Pos2::new(r.left(), r.top() - 4.0), Pos2::new(r.left() + 10.0, r.top() - 4.0)], s);
            p.line_segment([Pos2::new(r.left() + 10.0, r.top() - 4.0), Pos2::new(r.left() + 13.0, r.top())], s);
        }
        Icon::Prev => {
            p.circle_stroke(c, 20.0, Stroke::new(2.5, col));
            chevron(&p, c.x - 1.0, true);
        }
        Icon::Start => {
            p.circle_stroke(c, 20.0, Stroke::new(2.5, col));
            p.line_segment([Pos2::new(c.x - 8.0, c.y - 9.0), Pos2::new(c.x - 8.0, c.y + 9.0)], st);
            chevron(&p, c.x + 3.0, true);
        }
    }
    resp.on_hover_text(tip)
}
