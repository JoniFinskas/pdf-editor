use crate::{
    icons::Icon,
    theme::{self, Palette},
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui::{self, RichText};
use pdf_editor_core::{DesignedField, FieldValue, FormField};
use std::collections::{HashMap, HashSet};
#[derive(Default)]
pub struct Forms {
    pub open: bool,
    pub fields: Vec<FormField>,
    pub designer: crate::form_design::Designer,
    design: bool,
    committed: HashMap<(u16,u16),FieldValue>,
    drafts: HashSet<(u16,u16)>,
}
fn identities(fields:&[DesignedField])->HashMap<(u16,&str),Option<(u32,u16)>> {
    let mut ids=HashMap::new();
    for field in fields {ids.entry((field.page,field.name.as_str())).and_modify(|id|*id=None).or_insert(Some(field.id));}
    ids
}
impl Forms {
    pub fn has_drafts(&self)->bool {!self.drafts.is_empty()}
    pub fn reset_document(&mut self) {
        self.fields.clear();self.committed.clear();self.drafts.clear();self.designer=Default::default();
    }
    pub fn receive(&mut self,mut fields:Vec<FormField>,designed:Vec<DesignedField>)->usize {
        let previous:HashMap<_,_>=self.fields.iter().map(|field|((field.page,field.index),field)).collect();
        let old_ids=identities(&self.designer.fields);let new_ids=identities(&designed);
        let committed:HashMap<_,_>=fields.iter().map(|field|((field.page,field.index),field.value.clone())).collect();
        let mut drafts=HashSet::new();
        for field in &mut fields {
            let key=(field.page,field.index);
            let Some(old)=previous.get(&key) else {continue;};
            let name=(field.page,field.name.as_str());
            let same_id=old_ids.get(&name).is_some_and(|id|id.is_some() && Some(id)==new_ids.get(&name));
            if self.drafts.contains(&key) && !field.read_only && !old.read_only && old.name==field.name
                && self.committed.get(&key)==Some(&field.value) && same_id {
                field.value=old.value.clone();drafts.insert(key);
            }
        }
        let cleared=self.drafts.iter().filter(|key|!drafts.contains(*key) && previous.get(*key).is_some_and(|old|committed.get(*key)!=Some(&old.value))).count();
        self.fields=fields;self.committed=committed;self.drafts=drafts;self.designer.refresh(designed);
        cleared
    }
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Forms", Icon::Form)
            .width(if self.design { 560.0 } else { 420.0 })
            .show(ctx, p, &mut self.open, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.design, false, "Fill");
                    ui.selectable_value(&mut self.design, true, "Design");
                    if ui
                        .add_enabled(
                            idle && !self.fields.is_empty() && self.drafts.is_empty(),
                            egui::Button::new("Flatten forms"),
                        )
                        .on_hover_text(
                            "Turn filled fields into page content. Undo restores editable fields.",
                        )
                        .on_disabled_hover_text("Apply or revert field changes first.")
                        .clicked()
                    {
                        request = Some(Request::FlattenForms);
                    }
                });
                if self.design {
                    if let Some(design_request) = self.designer.show(ui, p, idle) {
                        request = Some(design_request);
                    }
                    return;
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Btn::icon(Icon::RotateRight)
                            .height(30.0)
                            .tip("Refresh fields")
                            .enabled(idle)
                            .show(ui, p)
                            .clicked()
                        {
                            request = Some(Request::FormFields);
                        }
                    });
                });
                if self.fields.is_empty() {
                    widgets::hint(ui, p, "No supported form fields.");
                }
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for field in &mut self.fields {
                            let key=(field.page,field.index);
                            ui.push_id(key, |ui| {
                                widgets::rule(ui, p);
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&field.name)
                                            .font(theme::semibold(13.5))
                                            .color(p.text),
                                    );
                                    ui.label(
                                        RichText::new(format!("Page {}", field.page + 1))
                                            .size(12.5)
                                            .color(p.muted),
                                    );
                                    if field.read_only {
                                        ui.label(
                                            RichText::new("Read-only").size(12.5).color(p.muted),
                                        );
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(idle && !field.read_only, |ui| {
                                        let mut changed=false;
                                        match &mut field.value {
                                            FieldValue::Text(value) => {
                                                changed=ui.add(
                                                    egui::TextEdit::singleline(value)
                                                        .margin(egui::Margin::symmetric(10, 7))
                                                        .desired_width(260.0),
                                                ).changed();
                                            }
                                            FieldValue::Check(value) => {
                                                changed=widgets::checkbox(ui, value, "Checked").changed();
                                            }
                                            FieldValue::Radio(value) => {
                                                if ui.radio(*value, "Selected").clicked() {
                                                    changed = !*value;*value = true;
                                                }
                                            }
                                            FieldValue::Choice { options, selected } => {
                                                egui::ComboBox::from_id_salt("choice")
                                                    .width(244.0)
                                                    .selected_text(
                                                        selected
                                                            .and_then(|i| options.get(i))
                                                            .map_or("Select…", String::as_str),
                                                    )
                                                    .show_ui(ui, |ui| {
                                                        for (i, option) in
                                                            options.iter().enumerate()
                                                        {
                                                            changed|=ui.selectable_value(
                                                                selected,
                                                                Some(i),
                                                                option,
                                                            ).changed();
                                                        }
                                                    });
                                            }
                                        }
                                        if changed {
                                            if self.committed.get(&key)==Some(&field.value) {self.drafts.remove(&key);}
                                            else {self.drafts.insert(key);}
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if Btn::new("Apply")
                                                    .look(Look::OutlineAccent)
                                                    .height(30.0)
                                                    .tip("Apply field")
                                                    .enabled(self.drafts.contains(&key))
                                                    .show(ui, p)
                                                    .clicked()
                                                {
                                                    request = Some(Request::FillField(
                                                        field.clone(),
                                                        field.value.clone(),
                                                    ));
                                                }
                                                if self.drafts.contains(&key) && Btn::icon(Icon::Undo).id(&format!("form-revert-{}-{}",field.page,field.index)).look(Look::Ghost).height(30.).tip("Revert field").show(ui,p).clicked()
                                                    && let Some(value)=self.committed.get(&key) {
                                                    field.value=value.clone();self.drafts.remove(&key);
                                                }
                                            },
                                        );
                                    });
                                });
                                ui.add_space(4.0);
                            });
                        }
                    });
            });
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_drafts_do_not_transfer_to_replaced_ambiguous_or_changed_fields() {
        let field=FormField {page:0,index:0,revision:1,name:"Generated".into(),read_only:false,value:FieldValue::Text(String::new())};
        let designed=DesignedField {id:(7,0),page:0,revision:1,name:field.name.clone(),required:false,
            bounds:pdf_editor_core::Bounds {left:36.,right:220.,bottom:100.,top:128.},kind:"Text".into()};
        for case in 0..5 {
            let mut forms=Forms::default();forms.receive(vec![field.clone()],vec![designed.clone()]);
            forms.fields[0].value=FieldValue::Text("Unapplied draft".into());forms.drafts.insert((0,0));
            let mut next=field.clone();next.revision=2;let mut id=designed.clone();id.revision=2;let mut ids=vec![id];
            match case {
                0=>ids[0].id=(9,0),
                1=>ids.push(DesignedField {id:(8,0),..designed.clone()}),
                2=>next.value=FieldValue::Text("Changed in document".into()),
                3=>next.read_only=true,
                _=>forms.reset_document(),
            }
            let expected=next.value.clone();let cleared=forms.receive(vec![next],ids);
            assert_eq!(cleared,usize::from(case!=4));
            assert_eq!(forms.fields[0].value,expected,"Draft leaked across a field identity change");assert!(!forms.has_drafts());
        }
    }
}
