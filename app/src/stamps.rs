use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::{BatesNumber, RangeStamp, StampPlacement, TextStamp, stamp_date_today};
pub struct Stamps {
    pub open: bool,
    stamp: TextStamp,
    pages: String,
    date: String,
    placement: StampPlacement,
    bates: BatesNumber,
    bates_mode: bool,
}
impl Default for Stamps {
    fn default() -> Self {
        Self {
            open: false,
            bates: BatesNumber {
                prefix: "DOC-".into(),
                start: 1,
                digits: 6,
            },
            bates_mode: false,
            pages: String::new(),
            date: stamp_date_today(),
            placement: StampPlacement::Absolute,
            stamp: TextStamp {
                template: "{page} / {pages}".into(),
                font: "Arial".into(),
                size: 12.0,
                color: [80, 80, 80],
                opacity: 1.0,
                x: 72.0,
                y: 24.0,
            },
        }
    }
}
impl Stamps {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Headers, footers and Bates", Icon::Hash)
            .width(420.0)
            .show(ctx, p, &mut self.open, |ui| {
                widgets::field(ui, p, "Start from", |ui| {
                    ui.horizontal(|ui| {
                        if Btn::new("Page numbers")
                            .with_icon(Icon::Hash)
                            .look(Look::Outline)
                            .show(ui, p)
                            .clicked()
                        {
                            self.bates_mode = false;
                            self.placement = StampPlacement::Footer;
                            self.stamp.template = "{page} / {pages}".into();
                            self.stamp.size = 12.0;
                            self.stamp.opacity = 1.0;
                            self.stamp.y = 24.0;
                        }
                        if Btn::new("Watermark")
                            .with_icon(Icon::TextPlus)
                            .look(Look::Outline)
                            .show(ui, p)
                            .clicked()
                        {
                            self.bates_mode = false;
                            self.placement = StampPlacement::Absolute;
                            self.stamp.template = "DRAFT".into();
                            self.stamp.size = 48.0;
                            self.stamp.opacity = 0.25;
                            self.stamp.y = 300.0;
                        }
                    });
                });
                ui.checkbox(&mut self.bates_mode, "Bates numbering");
                if self.bates_mode {
                    widgets::field(ui, p, "Prefix", |ui| {
                        ui.text_edit_singleline(&mut self.bates.prefix);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Start");
                        ui.add(egui::DragValue::new(&mut self.bates.start));
                        ui.label("Digits");
                        ui.add(egui::DragValue::new(&mut self.bates.digits).range(1..=20));
                    });
                    ui.label(self.bates.label(0).unwrap_or_else(|e| e.to_string()));
                }
                let width = ui.available_width();
                if !self.bates_mode {
                    widgets::field(ui, p, "Text", |ui| {
                        ui.add(widgets::text_field(&mut self.stamp.template, width));
                    });
                    widgets::hint(ui, p, "Tokens: {page}, {pages}, {date}. Date stays fixed for this operation.");
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let font_width = 2.0 * widgets::column_width(ui) + 10.0;
                    widgets::field(ui, p, "Font", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.stamp.font)
                                .margin(egui::Margin::symmetric(10, 7))
                                .desired_width(font_width),
                        );
                    });
                    widgets::number(
                        ui,
                        p,
                        "Size",
                        true,
                        egui::DragValue::new(&mut self.stamp.size).range(1.0..=1000.0),
                    );
                    widgets::field(ui, p, "Color", |ui| {
                        ui.color_edit_button_srgb(&mut self.stamp.color)
                    });
                });
                widgets::field(ui, p, "Page ranges (blank = all)", |ui| {
                    ui.text_edit_singleline(&mut self.pages);
                });
                if !self.bates_mode {
                    widgets::field(ui, p, "Date", |ui| {
                        ui.text_edit_singleline(&mut self.date);
                    });
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.placement, StampPlacement::Header, "Header");
                    ui.selectable_value(&mut self.placement, StampPlacement::Footer, "Footer");
                    ui.selectable_value(&mut self.placement, StampPlacement::Absolute, "Watermark");
                });
                widgets::slider(
                    ui,
                    p,
                    "Opacity",
                    egui::Slider::new(&mut self.stamp.opacity, 0.0..=1.0),
                );
                widgets::caption(ui, p, "PDF points · left offset and edge margin; watermark uses bottom baseline");
                widgets::numbers(
                    ui,
                    p,
                    vec![
                        ("Left", egui::DragValue::new(&mut self.stamp.x)),
                        ("Baseline", egui::DragValue::new(&mut self.stamp.y)),
                    ],
                );
                widgets::actions(ui, |ui| {
                    if Btn::new("Apply to selected pages")
                        .look(Look::Primary)
                        .enabled(idle && (self.bates_mode || !self.stamp.template.is_empty()))
                        .show(ui, p)
                        .clicked()
                    {
                        let options = RangeStamp {
                            stamp: self.stamp.clone(),
                            pages: self.pages.clone(),
                            date: self.date.clone(),
                            placement: self.placement,
                        };
                        request = Some(if self.bates_mode {
                            Request::Bates(options, self.bates.clone())
                        } else {
                            Request::Stamp(options)
                        });
                    }
                });
            });
        request
    }
}
