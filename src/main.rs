// PGN Tree Viewer: a phone-style opening-tree viewer for an analysed PGN.
//
// Layout: dark title bar, wooden board with a last-move arrow, a list of the
// continuations at the current position (move / number of sub-moves /
// evaluation), and a dark navigation bar. The PGN file is watched and
// re-read automatically whenever it changes on disk, keeping the current
// position.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pgn;

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

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PGN Tree Viewer")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([820.0, 520.0])
            .with_active(true),
        ..Default::default()
    };
    eframe::run_native(
        "PGN Tree Viewer",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    path: Option<PathBuf>,
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

        // The viewer always starts empty; the user picks the PGN via "Open".
        let mut app = Self {
            path: None,
            tree: None,
            current: 0,
            selected: 0,
            flip: stored_flip,
            status: String::new(),
            pieces,
            scroll_to_selected: false,
            screenshot: std::env::var_os("PGNVIEWER_SCREENSHOT").map(PathBuf::from),
            frames: 0,
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
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("Cannot read {}: {e}", path.display());
                return;
            }
        };
        match pgn::parse(&text) {
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
                self.path = Some(path.to_path_buf());
            }
            Err(e) => {
                self.status = format!("PGN error in {}: {e}", path.display());
            }
        }
    }

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

    fn copy_fen(&self, ctx: &egui::Context) {
        if let Some(t) = &self.tree {
            ctx.copy_text(Fen::from_position(&t.nodes[self.current].pos, EnPassantMode::Legal).to_string());
        }
    }

    fn title(&self) -> String {
        if let Some(t) = &self.tree {
            for key in ["ChapterName", "Event"] {
                if let Some(v) = t.header(key).filter(|v| !v.is_empty() && *v != "?") {
                    return v.to_string();
                }
            }
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "PGN Tree Viewer".into())
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
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_screenshot(&ctx);

        ctx.input(|i| {
            if i.key_pressed(Key::ArrowLeft) {
                self.back();
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
                    ui.label(egui::RichText::new(self.title()).color(BAR_TEXT).size(22.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let menu = icon_button(ui, Icon::Menu, "Menu");
                        egui::Popup::menu(&menu).show(|ui| {
                            ui.set_min_width(180.0);
                            if ui.button("Open PGN…").clicked() {
                                self.open_dialog();
                            }
                            if ui.button("Reload").clicked() {
                                if let Some(p) = self.path.clone() {
                                    self.load(&p, true);
                                }
                            }
                            if ui.button("Copy FEN").clicked() {
                                self.copy_fen(&ctx);
                            }
                            if ui.button("Flip board (F)").clicked() {
                                self.flip = !self.flip;
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
                let has_tree = self.tree.is_some();
                let (can_back, can_fwd) = self
                    .tree
                    .as_ref()
                    .map(|t| (t.nodes[self.current].parent.is_some(), !t.nodes[self.current].children.is_empty()))
                    .unwrap_or((false, false));
                ui.horizontal(|ui| {
                    let n = 5.0;
                    let w = 52.0;
                    let gap = 22.0;
                    ui.add_space(((ui.available_width() - n * w - (n - 1.0) * gap) / 2.0).max(0.0));
                    ui.spacing_mut().item_spacing.x = gap;
                    if icon_button_enabled(ui, Icon::Start, "Start (Home)", can_back).clicked() {
                        self.to_start();
                    }
                    if icon_button_enabled(ui, Icon::Prev, "Back (←)", can_back).clicked() {
                        self.back();
                    }
                    if icon_button_enabled(ui, Icon::Next, "Play selected move (→)", can_fwd).clicked() {
                        self.forward();
                    }
                    if icon_button_enabled(ui, Icon::End, "End of line (End)", can_fwd).clicked() {
                        self.to_end();
                    }
                    if icon_button_enabled(ui, Icon::Flip, "Flip board (F)", has_tree).clicked() {
                        self.flip = !self.flip;
                    }
                });
            });

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
                        if !self.status.is_empty() {
                            ui.add_space(12.0);
                            ui.colored_label(Color32::from_rgb(0xc0, 0x30, 0x30), &self.status);
                        }
                    });
                    return;
                };

                // Board on the left, continuations on the right.
                let full = ui.available_rect_before_wrap();
                let pad = 16.0;
                let board = (full.height() - 2.0 * pad).min(full.width() * 0.6 - 2.0 * pad).max(160.0).floor();
                let board_rect = Rect::from_min_size(full.min + Vec2::splat(pad), Vec2::splat(board));
                let node = &tree.nodes[self.current];
                let painter = ui.painter_at(full);
                draw_board(&painter, board_rect, &node.pos, self.flip, &self.pieces);
                if let Some((from, to)) = node.from_to {
                    draw_arrow(&painter, board_rect, from, to, self.flip);
                }
                let right = Rect::from_min_max(Pos2::new(board_rect.right() + pad, full.top() + pad), full.max);
                let mut clicked = None;
                let scroll_to = std::mem::take(&mut self.scroll_to_selected);
                ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {

                // Current line + evaluation of the current position.
                let line_text = current_line(tree, self.current);
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
    Next,
    End,
    Flip,
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
    icon_button_enabled(ui, icon, tip, true)
}

/// Hand-painted white line icons on the dark bars.
fn icon_button_enabled(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool) -> egui::Response {
    let size = Vec2::new(52.0, 48.0);
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
        Icon::Prev | Icon::Next => {
            let left = matches!(icon, Icon::Prev);
            p.circle_stroke(c, 20.0, Stroke::new(2.5, col));
            chevron(&p, c.x + if left { -1.0 } else { 1.0 }, left);
        }
        Icon::Start | Icon::End => {
            let left = matches!(icon, Icon::Start);
            p.circle_stroke(c, 20.0, Stroke::new(2.5, col));
            let bar_x = if left { c.x - 8.0 } else { c.x + 8.0 };
            p.line_segment([Pos2::new(bar_x, c.y - 9.0), Pos2::new(bar_x, c.y + 9.0)], st);
            chevron(&p, c.x + if left { 3.0 } else { -3.0 }, left);
        }
        Icon::Flip => {
            // Two curved arrows (rotate).
            let r = 12.0;
            for (a0, a1) in [(0.3f32, 2.7f32), (3.44, 5.84)] {
                let pts: Vec<Pos2> = (0..=12)
                    .map(|k| {
                        let t = a0 + (a1 - a0) * k as f32 / 12.0;
                        c + Vec2::new(t.cos(), t.sin()) * r
                    })
                    .collect();
                let end = *pts.last().unwrap();
                p.add(egui::Shape::line(pts, Stroke::new(2.5, col)));
                let tang = Vec2::new(-a1.sin(), a1.cos());
                let back = -tang * 7.0;
                let side = Vec2::new(a1.cos(), a1.sin()) * 5.0;
                p.add(egui::Shape::convex_polygon(
                    vec![end + tang * 4.0, end + back + side, end + back - side],
                    col,
                    Stroke::NONE,
                ));
            }
        }
    }
    resp.on_hover_text(tip)
}
