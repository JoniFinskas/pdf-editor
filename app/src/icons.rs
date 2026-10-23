//! Line icons painted with egui shapes on a 24-unit grid, so the interface
//! needs no icon font. Shapes follow common outline-icon conventions.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    File,
    FilePlus,
    Folder,
    Save,
    Undo,
    Redo,
    TextPlus,
    Search,
    Minus,
    Plus,
    Gear,
    ChevronLeft,
    ChevronDown,
    Dots,
    Home,
    Clock,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Close,
    Trash,
    RotateRight,
    Keyboard,
    Info,
    Edit,
    Sidebar,
    Bookmark,
    Crop,
    Image,
    Print,
    Pen,
    Signature,
    Form,
    Redact,
    Compress,
    Layers,
    Scan,
    Hash,
    Highlighter,
    Export,
    Square,
    Circle,
    Line,
    ArrowDiagonal,
    Lock,
    Replace,
}

/// Paints `icon` centered in `rect` with the given color.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let pen = Pen {
        painter,
        origin: rect.center() - eframe::egui::vec2(side, side) / 2.0,
        scale: side / 24.0,
        stroke: Stroke::new((side / 24.0 * 1.7).max(1.1), color),
        color,
    };
    pen.draw(icon);
}

struct Pen<'a> {
    painter: &'a Painter,
    origin: Pos2,
    scale: f32,
    stroke: Stroke,
    color: Color32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + eframe::egui::vec2(x, y) * self.scale
    }

    fn line(&self, points: &[(f32, f32)]) {
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::line(points, self.stroke));
    }

    fn closed(&self, points: &[(f32, f32)]) {
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::closed_line(points, self.stroke));
    }

    fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_stroke(self.p(x, y), r * self.scale, self.stroke);
    }

    fn fill(&self, points: &[(f32, f32)]) {
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter
            .add(Shape::convex_polygon(points, self.color, Stroke::NONE));
    }

    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_filled(self.p(x, y), r * self.scale, self.color);
    }

    /// Arc around (x, y); angles in turns, increasing clockwise on screen.
    fn arc(&self, x: f32, y: f32, r: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
        let steps = ((to - from).abs() * 48.0).ceil().max(4.0) as usize;
        (0..=steps)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / steps as f32) * TAU;
                (x + r * a.cos(), y + r * a.sin())
            })
            .collect()
    }

    fn file(&self) {
        self.closed(&[
            (5.0, 2.5),
            (14.0, 2.5),
            (19.0, 7.5),
            (19.0, 21.5),
            (5.0, 21.5),
        ]);
        self.line(&[(14.0, 2.5), (14.0, 7.5), (19.0, 7.5)]);
    }

    fn draw(&self, icon: Icon) {
        match icon {
            Icon::File => self.file(),
            Icon::FilePlus => {
                self.file();
                self.line(&[(12.0, 11.0), (12.0, 17.5)]);
                self.line(&[(8.75, 14.25), (15.25, 14.25)]);
            }
            Icon::Folder => self.closed(&[
                (2.5, 5.5),
                (9.0, 5.5),
                (11.0, 8.0),
                (21.5, 8.0),
                (21.5, 19.5),
                (2.5, 19.5),
            ]),
            Icon::Save => {
                self.closed(&[
                    (4.0, 3.5),
                    (16.5, 3.5),
                    (20.5, 7.5),
                    (20.5, 20.5),
                    (4.0, 20.5),
                ]);
                self.line(&[(8.0, 3.5), (8.0, 8.5), (15.0, 8.5), (15.0, 3.5)]);
                self.line(&[(7.0, 20.5), (7.0, 13.5), (17.5, 13.5), (17.5, 20.5)]);
            }
            Icon::Undo | Icon::Redo => {
                let m = |x: f32| if icon == Icon::Redo { 24.0 - x } else { x };
                self.line(&[(m(9.0), 14.0), (m(4.0), 9.0), (m(9.0), 4.0)]);
                let mut path = vec![(m(4.0), 9.0), (m(14.5), 9.0)];
                for (x, y) in self.arc(14.5, 14.5, 5.5, -0.25, 0.25) {
                    path.push((m(x), y));
                }
                path.push((m(11.0), 20.0));
                self.line(&path);
            }
            Icon::TextPlus => {
                self.line(&[(3.0, 7.0), (3.0, 4.0), (15.0, 4.0), (15.0, 7.0)]);
                self.line(&[(9.0, 4.0), (9.0, 20.0)]);
                self.line(&[(6.5, 20.0), (11.5, 20.0)]);
                self.line(&[(19.0, 11.5), (19.0, 19.5)]);
                self.line(&[(15.0, 15.5), (23.0, 15.5)]);
            }
            Icon::Search => {
                self.circle(11.0, 11.0, 7.0);
                self.line(&[(16.2, 16.2), (21.0, 21.0)]);
            }
            Icon::Minus => self.line(&[(5.0, 12.0), (19.0, 12.0)]),
            Icon::Plus => {
                self.line(&[(5.0, 12.0), (19.0, 12.0)]);
                self.line(&[(12.0, 5.0), (12.0, 19.0)]);
            }
            Icon::Gear => {
                let mut teeth = Vec::new();
                for i in 0..8 {
                    let base = i as f32 / 8.0;
                    for (r, da) in [(7.4, -0.047), (9.8, -0.026), (9.8, 0.026), (7.4, 0.047)] {
                        let a = (base + da) * TAU;
                        teeth.push((12.0 + r * a.cos(), 12.0 + r * a.sin()));
                    }
                }
                self.closed(&teeth);
                self.circle(12.0, 12.0, 3.0);
            }
            Icon::ChevronLeft => self.line(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]),
            Icon::ChevronDown => self.line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
            Icon::Dots => {
                for y in [5.0, 12.0, 19.0] {
                    self.dot(12.0, y, 1.6);
                }
            }
            Icon::Home => {
                self.line(&[(3.0, 10.5), (12.0, 3.0), (21.0, 10.5)]);
                self.line(&[(5.5, 8.5), (5.5, 21.0), (18.5, 21.0), (18.5, 8.5)]);
                self.line(&[(10.0, 21.0), (10.0, 14.5), (14.0, 14.5), (14.0, 21.0)]);
            }
            Icon::Clock => {
                self.circle(12.0, 12.0, 9.0);
                self.line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
            }
            Icon::ArrowLeft => {
                self.line(&[(19.0, 12.0), (5.0, 12.0)]);
                self.line(&[(11.0, 6.0), (5.0, 12.0), (11.0, 18.0)]);
            }
            Icon::ArrowRight => {
                self.line(&[(5.0, 12.0), (19.0, 12.0)]);
                self.line(&[(13.0, 6.0), (19.0, 12.0), (13.0, 18.0)]);
            }
            Icon::ArrowUp => {
                self.line(&[(12.0, 19.0), (12.0, 5.0)]);
                self.line(&[(6.0, 11.0), (12.0, 5.0), (18.0, 11.0)]);
            }
            Icon::ArrowDown => {
                self.line(&[(12.0, 5.0), (12.0, 19.0)]);
                self.line(&[(6.0, 13.0), (12.0, 19.0), (18.0, 13.0)]);
            }
            Icon::Close => {
                self.line(&[(6.0, 6.0), (18.0, 18.0)]);
                self.line(&[(18.0, 6.0), (6.0, 18.0)]);
            }
            Icon::Trash => {
                self.line(&[(3.5, 6.0), (20.5, 6.0)]);
                self.line(&[(9.0, 6.0), (9.0, 3.5), (15.0, 3.5), (15.0, 6.0)]);
                self.line(&[(5.5, 6.0), (6.5, 21.0), (17.5, 21.0), (18.5, 6.0)]);
                self.line(&[(10.0, 10.5), (10.0, 16.5)]);
                self.line(&[(14.0, 10.5), (14.0, 16.5)]);
            }
            Icon::RotateRight => {
                self.line(&self.arc(12.0, 12.0, 8.5, 0.0, 0.84));
                self.line(&[(20.5, 3.5), (20.5, 8.5), (15.5, 8.5)]);
            }
            Icon::Keyboard => {
                self.closed(&[(2.5, 6.0), (21.5, 6.0), (21.5, 18.0), (2.5, 18.0)]);
                for x in [6.5, 10.0, 13.5, 17.0] {
                    self.dot(x, 10.0, 0.9);
                }
                self.line(&[(7.5, 14.5), (16.5, 14.5)]);
            }
            Icon::Info => {
                self.circle(12.0, 12.0, 9.0);
                self.line(&[(12.0, 11.0), (12.0, 16.5)]);
                self.dot(12.0, 7.8, 1.1);
            }
            Icon::Edit => {
                self.line(&[
                    (12.0, 3.5),
                    (5.0, 3.5),
                    (3.5, 5.0),
                    (3.5, 19.0),
                    (5.0, 20.5),
                    (19.0, 20.5),
                    (20.5, 19.0),
                    (20.5, 12.0),
                ]);
                self.closed(&[
                    (18.0, 2.5),
                    (21.5, 6.0),
                    (12.5, 15.0),
                    (8.5, 15.5),
                    (9.0, 11.5),
                ]);
            }
            Icon::Sidebar => {
                self.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
                self.line(&[(12.0, 4.0), (12.0, 20.0)]);
            }
            Icon::Bookmark => {
                self.closed(&[
                    (6.0, 3.0),
                    (18.0, 3.0),
                    (18.0, 21.0),
                    (12.0, 16.5),
                    (6.0, 21.0),
                ]);
            }
            Icon::Crop => {
                self.line(&[(6.0, 2.5), (6.0, 18.0), (21.5, 18.0)]);
                self.line(&[(2.5, 6.0), (18.0, 6.0), (18.0, 21.5)]);
            }
            Icon::Image => {
                self.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
                self.circle(8.5, 9.5, 1.8);
                self.line(&[
                    (3.0, 17.0),
                    (9.0, 12.0),
                    (13.0, 15.5),
                    (16.0, 13.0),
                    (21.0, 17.0),
                ]);
            }
            Icon::Print => {
                self.line(&[(6.0, 9.0), (6.0, 3.0), (18.0, 3.0), (18.0, 9.0)]);
                self.line(&[
                    (6.0, 17.0),
                    (3.0, 17.0),
                    (3.0, 9.0),
                    (21.0, 9.0),
                    (21.0, 17.0),
                    (18.0, 17.0),
                ]);
                self.closed(&[(6.0, 14.0), (18.0, 14.0), (18.0, 21.0), (6.0, 21.0)]);
            }
            Icon::Pen => {
                self.closed(&[
                    (16.0, 3.5),
                    (20.5, 8.0),
                    (8.0, 20.5),
                    (3.5, 20.5),
                    (3.5, 16.0),
                ]);
                self.line(&[(13.5, 6.0), (18.0, 10.5)]);
            }
            Icon::Signature => {
                self.line(&[
                    (3.0, 15.0),
                    (6.0, 7.0),
                    (8.0, 15.0),
                    (10.5, 9.5),
                    (12.5, 14.0),
                    (14.5, 11.5),
                    (16.5, 14.0),
                    (21.0, 12.0),
                ]);
                self.line(&[(3.0, 20.0), (21.0, 20.0)]);
            }
            Icon::Form => {
                self.closed(&[(3.5, 4.5), (9.5, 4.5), (9.5, 10.5), (3.5, 10.5)]);
                self.line(&[(5.0, 7.5), (6.5, 9.0), (8.5, 6.0)]);
                self.line(&[(13.0, 7.5), (21.0, 7.5)]);
                self.closed(&[(3.5, 13.5), (9.5, 13.5), (9.5, 19.5), (3.5, 19.5)]);
                self.line(&[(13.0, 16.5), (21.0, 16.5)]);
            }
            Icon::Redact => {
                self.closed(&[(3.0, 5.0), (21.0, 5.0), (21.0, 19.0), (3.0, 19.0)]);
                self.fill(&[(6.5, 10.0), (17.5, 10.0), (17.5, 14.0), (6.5, 14.0)]);
            }
            Icon::Compress => {
                self.line(&[(3.5, 3.5), (10.0, 10.0)]);
                self.line(&[(10.0, 4.5), (10.0, 10.0), (4.5, 10.0)]);
                self.line(&[(20.5, 20.5), (14.0, 14.0)]);
                self.line(&[(14.0, 19.5), (14.0, 14.0), (19.5, 14.0)]);
            }
            Icon::Layers => {
                self.closed(&[(12.0, 3.0), (21.0, 7.5), (12.0, 12.0), (3.0, 7.5)]);
                self.line(&[(3.0, 12.0), (12.0, 16.5), (21.0, 12.0)]);
                self.line(&[(3.0, 16.5), (12.0, 21.0), (21.0, 16.5)]);
            }
            Icon::Scan => {
                self.line(&[(3.0, 8.0), (3.0, 3.0), (8.0, 3.0)]);
                self.line(&[(16.0, 3.0), (21.0, 3.0), (21.0, 8.0)]);
                self.line(&[(21.0, 16.0), (21.0, 21.0), (16.0, 21.0)]);
                self.line(&[(8.0, 21.0), (3.0, 21.0), (3.0, 16.0)]);
                self.line(&[(7.5, 10.0), (16.5, 10.0)]);
                self.line(&[(7.5, 14.0), (13.5, 14.0)]);
            }
            Icon::Hash => {
                self.line(&[(10.0, 3.5), (8.0, 20.5)]);
                self.line(&[(16.0, 3.5), (14.0, 20.5)]);
                self.line(&[(4.5, 9.0), (20.0, 9.0)]);
                self.line(&[(4.0, 15.0), (19.5, 15.0)]);
            }
            Icon::Highlighter => {
                self.closed(&[(15.0, 3.0), (21.0, 9.0), (12.0, 18.0), (6.0, 12.0)]);
                self.line(&[(6.0, 12.0), (4.0, 17.0), (7.0, 20.0), (12.0, 18.0)]);
                self.line(&[(14.0, 21.0), (21.0, 21.0)]);
            }
            Icon::Export => {
                self.line(&[(12.0, 15.0), (12.0, 3.0)]);
                self.line(&[(7.0, 8.0), (12.0, 3.0), (17.0, 8.0)]);
                self.line(&[(4.0, 13.0), (4.0, 21.0), (20.0, 21.0), (20.0, 13.0)]);
            }
            Icon::Square => self.closed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]),
            Icon::Circle => self.circle(12.0, 12.0, 8.5),
            Icon::Line => self.line(&[(5.0, 19.0), (19.0, 5.0)]),
            Icon::ArrowDiagonal => {
                self.line(&[(5.0, 19.0), (19.0, 5.0)]);
                self.line(&[(10.0, 5.0), (19.0, 5.0), (19.0, 14.0)]);
            }
            Icon::Lock => {
                self.closed(&[(5.0, 11.0), (19.0, 11.0), (19.0, 21.0), (5.0, 21.0)]);
                let mut shackle = vec![(8.0, 11.0)];
                shackle.extend(self.arc(12.0, 7.5, 4.0, 0.5, 1.0));
                shackle.push((16.0, 11.0));
                self.line(&shackle);
            }
            Icon::Replace => {
                self.line(&[(4.0, 8.0), (19.0, 8.0)]);
                self.line(&[(15.0, 4.0), (19.0, 8.0), (15.0, 12.0)]);
                self.line(&[(20.0, 16.0), (5.0, 16.0)]);
                self.line(&[(9.0, 12.0), (5.0, 16.0), (9.0, 20.0)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, LayerId, pos2, vec2};

    const ALL: [Icon; 47] = [
        Icon::File,
        Icon::FilePlus,
        Icon::Folder,
        Icon::Save,
        Icon::Undo,
        Icon::Redo,
        Icon::TextPlus,
        Icon::Search,
        Icon::Minus,
        Icon::Plus,
        Icon::Gear,
        Icon::ChevronLeft,
        Icon::ChevronDown,
        Icon::Dots,
        Icon::Home,
        Icon::Clock,
        Icon::ArrowLeft,
        Icon::ArrowRight,
        Icon::ArrowUp,
        Icon::ArrowDown,
        Icon::Close,
        Icon::Trash,
        Icon::RotateRight,
        Icon::Keyboard,
        Icon::Info,
        Icon::Edit,
        Icon::Sidebar,
        Icon::Bookmark,
        Icon::Crop,
        Icon::Image,
        Icon::Print,
        Icon::Pen,
        Icon::Signature,
        Icon::Form,
        Icon::Redact,
        Icon::Compress,
        Icon::Layers,
        Icon::Scan,
        Icon::Hash,
        Icon::Highlighter,
        Icon::Export,
        Icon::Square,
        Icon::Circle,
        Icon::Line,
        Icon::ArrowDiagonal,
        Icon::Lock,
        Icon::Replace,
    ];

    #[test]
    fn every_icon_paints_inside_its_box() {
        let ctx = Context::default();
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(24.0, 24.0));
        for icon in ALL {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                paint(
                    &ui.ctx().layer_painter(LayerId::background()),
                    rect,
                    icon,
                    Color32::WHITE,
                );
            });
            assert!(!output.shapes.is_empty(), "{icon:?}");
            for shape in &output.shapes {
                let bounds = shape.shape.visual_bounding_rect();
                assert!(
                    rect.expand(2.0).contains_rect(bounds),
                    "{icon:?}: {bounds:?}"
                );
            }
            output.textures_delta.clear();
        }
    }
}
