use super::*;

pub(super) fn execute(
    s: &mut CommandSession<'_>,
    doc: &mut Document,
    op: &str,
    a: &Args<'_>,
) -> Result<V> {
    let e = s.engine;
    let page = if op == "compare" && a.0.get("page").is_some_and(V::is_null) {
        0
    } else {
        a.page()?
    };
    match op {
        "document.policy" => {
            doc.substitution = match a.s("substitution")? {
                "line" => Substitution::Line,
                "document" => Substitution::Document,
                _ => return Err(err("Substitution must be line or document")),
            };
        }
        "text.add" => e.add_text(
            doc,
            &NewText {
                page,
                x: a.n("x", 42.)?,
                y: a.n("y", 72.)?,
                text: a.s("text")?.into(),
                font: a.text("font", "Arial")?.into(),
                size: a.n("size", 12.)?,
                color: a.color()?,
            },
        )?,
        "text.edit" | "text.delete" => {
            let selected = texts(e, doc, a)?;
            let result = e.edit_group(
                doc,
                &selected,
                if op == "text.delete" {
                    ""
                } else {
                    a.s("text")?
                },
            )?;
            return Ok(edit_result(result));
        }
        "text.style" => {
            let selected = e
                .text_objects(doc, page)?
                .into_iter()
                .find(|o| Some(o.index) == a.index("index").ok())
                .ok_or_else(|| err("Text object not found"))?;
            e.style_text(
                doc,
                &selected,
                a.n("size", selected.size)?,
                if a.0.get("color").is_none() {
                    [selected.color[0], selected.color[1], selected.color[2]]
                } else {
                    a.color()?
                },
            )?;
        }
        "text.format" => {
            let selected = texts(e, doc, a)?;
            let result = e.format_text(
                doc,
                &selected,
                &TextFormat {
                    font: a.s("font")?.into(),
                    bold: a.b("bold", false)?,
                    italic: a.b("italic", false)?,
                    size: a.n("size", 12.)?,
                    color: a.color()?,
                },
            )?;
            return Ok(edit_result(result));
        }
        "paragraph.edit" => {
            let paragraphs = paragraphs(&e.text_objects(doc, page)?);
            let selected = paragraphs
                .get(a.index("index")?)
                .ok_or_else(|| err("Paragraph not found"))?;
            return Ok(edit_result(e.edit_paragraph(
                doc,
                selected,
                a.s("text")?,
            )?));
        }
        "text.replace" => {
            return Ok(
                json!({"replacements":e.replace_text_with_options(doc,a.s("find")?,a.s("text")?,if a.0.get("occurrence").is_none_or(V::is_null){None}else{Some(a.index("occurrence")?)},&search(a)?)?}),
            );
        }
        "text.find" => {
            let hits = e.find_text_with_options(doc, a.s("find")?, &search(a)?)?;
            return Ok(
                json!({"total":hits.len(),"truncated":hits.len()>10000,"hits":hits.into_iter().take(10000).map(|h|json!({"page":h.page,"bounds":h.bounds.into_iter().map(rect).collect::<Vec<_>>(),"excerpt":h.excerpt})).collect::<Vec<_>>()}),
            );
        }
        "text.characters" => {
            let chars = e.page_text_characters(doc, page)?;
            let offset = a.0.get("offset").map_or(Ok(0), |_| a.index("offset"))?;
            let limit = a.0.get("limit").map_or(Ok(1000), |_| a.index("limit"))?;
            if !(1..=10000).contains(&limit) {
                return Err(err("Character limit must be 1–10000"));
            }
            return Ok(
                json!({"total":chars.len(),"items":chars.into_iter().skip(offset).take(limit).map(|c|json!({"index":c.index,"text":c.text,"bounds":rect(c.bounds)})).collect::<Vec<_>>()}),
            );
        }
        "text.spelling" => {
            let index = a.index("index")?;
            let text = e
                .text_objects(doc, page)?
                .into_iter()
                .find(|t| t.index == index)
                .ok_or_else(|| err("Text object not found"))?;
            return Ok(
                json!({"issues":check_spelling(&text.text,a.text("language","en-US")?)?.into_iter().map(|i|json!({"start":i.range.start,"end":i.range.end,"word":i.word,"suggestions":i.suggestions})).collect::<Vec<_>>()}),
            );
        }
        "text.highlight" => e.highlight_text_selection(doc, &regions(a)?)?,
        "objects.edit" => {
            let selected = items(e, doc, a)?;
            let action = match a.s("action")? {
                "move" => ObjectAction::Move {
                    dx: a.n("dx", 0.)?,
                    dy: a.n("dy", 0.)?,
                },
                "duplicate" => ObjectAction::Duplicate {
                    dx: a.n("dx", 12.)?,
                    dy: a.n("dy", -12.)?,
                },
                "resize" => ObjectAction::Resize {
                    bounds: a.bounds()?,
                },
                "delete" => ObjectAction::Delete,
                "front" => ObjectAction::BringToFront,
                "back" => ObjectAction::SendToBack,
                _ => return Err(err("Unknown object action")),
            };
            e.edit_page_items(doc, &selected, action)?;
        }
        "objects.layout" => {
            let layout = match a.s("alignment")? {
                "left" => ObjectLayout::AlignLeft,
                "right" => ObjectLayout::AlignRight,
                "top" => ObjectLayout::AlignTop,
                "bottom" => ObjectLayout::AlignBottom,
                "horizontal_center" => ObjectLayout::AlignHorizontalCenter,
                "vertical_center" => ObjectLayout::AlignVerticalCenter,
                "distribute_horizontal" => ObjectLayout::DistributeHorizontal,
                "distribute_vertical" => ObjectLayout::DistributeVertical,
                _ => return Err(err("Unknown object layout")),
            };
            e.layout_page_items(doc, &items(e, doc, a)?, layout)?;
        }
        "objects.copy" => {
            s.clipboard = Some(e.copy_page_items(doc, &items(e, doc, a)?)?);
        }
        "objects.paste" => e.paste_page_items(
            doc,
            s.clipboard
                .as_ref()
                .ok_or_else(|| err("Copy objects first"))?,
            page,
        )?,
        "objects.ungroup" | "objects.prepare_text" => {
            let index = a.index("index")?;
            let item = e
                .page_items(doc, page)?
                .into_iter()
                .find(|i| i.index == index)
                .ok_or_else(|| err("Page object not found"))?;
            if op == "objects.ungroup" {
                e.ungroup_page_item(doc, &item)?;
            } else {
                e.prepare_grouped_text(doc, &item)?;
            }
        }
        "page.blank" => e.edit_page_batch(
            doc,
            &PageBatch::Blank {
                before: a.page_key("before")?,
                width: a.n("width", 595.276)?,
                height: a.n("height", 841.89)?,
            },
        )?,
        "page.rotate" => e.edit_page_batch(doc, &PageBatch::Rotate(a.pages()?))?,
        "page.delete" => e.edit_page_batch(doc, &PageBatch::Delete(a.pages()?))?,
        "page.move" => e.edit_page_batch(
            doc,
            &PageBatch::Move {
                pages: a.pages()?,
                before: a.page_key("before")?,
            },
        )?,
        "page.duplicate" => e.edit_page_batch(
            doc,
            &PageBatch::Duplicate {
                pages: a.pages()?,
                before: a.page_key("before")?,
            },
        )?,
        "page.import" => e.edit_pages(
            doc,
            &PageEdit::Insert {
                path: s.path(a.s("path")?, false, false)?,
                before: a.page_key("before")?,
            },
        )?,
        "page.merge" => e.merge_pdfs(
            doc,
            &a.strings("paths")?
                .iter()
                .map(|p| s.path(p, false, false))
                .collect::<Result<Vec<_>>>()?,
        )?,
        "page.crop" => e.crop_page(doc, page, a.bounds()?)?,
        "page.extract" => e.extract_pages(doc, &a.pages()?, &s.path(a.s("path")?, true, false)?)?,
        "page.split" => {
            let paths = e.split_pdf(doc, a.s("ranges")?, &s.path(a.s("folder")?, false, true)?)?;
            return Ok(json!({"files":paths.len()}));
        }
        "page.split_bookmarks" => {
            let paths = e.split_pdf_at_bookmarks(doc, &s.path(a.s("folder")?, false, true)?)?;
            return Ok(json!({"files":paths.len()}));
        }
        "image.add" => {
            let picture = import_picture(&s.path(a.s("path")?, false, false)?)?;
            e.place_picture(doc, &picture, placement(a, page)?)?;
        }
        "image.move" => {
            let name = a.s("name")?;
            let selected = e
                .placed_pictures(doc)?
                .into_iter()
                .find(|p| p.page == page && p.name == name)
                .ok_or_else(|| err("Placed image not found"))?;
            let old = selected.placement;
            e.move_picture(
                doc,
                &selected,
                Placement {
                    page,
                    left: a.n("left", old.left)?,
                    bottom: a.n("bottom", old.bottom)?,
                    width: a.n("width", old.width)?,
                    height: a.n("height", old.height)?,
                },
            )?;
        }
        "draw" => {
            let points = a
                .array("points")?
                .iter()
                .map(|v| {
                    let p = v
                        .as_array()
                        .filter(|p| p.len() == 2)
                        .ok_or_else(|| err("Expected [x,y] points"))?;
                    Ok([number(&p[0])?, number(&p[1])?])
                })
                .collect::<Result<_>>()?;
            let kind = match a.s("kind")? {
                "freehand" => DrawingKind::Freehand,
                "rectangle" => DrawingKind::Rectangle,
                "ellipse" => DrawingKind::Ellipse,
                "line" => DrawingKind::Line,
                "arrow" => DrawingKind::Arrow,
                _ => return Err(err("Unknown drawing kind")),
            };
            e.add_drawing(
                doc,
                &Drawing {
                    page,
                    kind,
                    points,
                    color: a.color()?,
                    width: a.n("width", 1.)?,
                    opacity: a.n("opacity", 1.)?,
                },
            )?;
        }
        "annotation.add" => e.add_annotation(doc, &markup(a, None)?)?,
        "annotation.update" | "annotation.delete" => {
            let index = a.index("index")?;
            let selected = e
                .annotations(doc)?
                .into_iter()
                .find(|o| o.markup.page == page && o.index == index)
                .ok_or_else(|| err("Annotation not found"))?;
            if op.ends_with("delete") {
                e.delete_annotation(doc, &selected)?;
            } else {
                e.update_annotation(doc, &selected, &markup(a, Some(&selected.markup))?)?;
            }
        }
        "link.add" => e.add_link(doc, &link(a, None)?)?,
        "link.update" | "link.delete" => {
            let index = a.index("index")?;
            let selected = e
                .links(doc)?
                .into_iter()
                .find(|o| o.link.page == page && o.index == index)
                .ok_or_else(|| err("Link not found"))?;
            if op.ends_with("delete") {
                e.delete_link(doc, &selected)?;
            } else {
                e.update_link(doc, &selected, &link(a, Some(&selected.link))?)?;
            }
        }
        "form.add" => {
            let kind = match a.text("kind", "text")? {
                "text" => DesignFieldKind::Text,
                "date" => DesignFieldKind::Date,
                "checkbox" => DesignFieldKind::Checkbox,
                "radio" => DesignFieldKind::Radio {
                    options: a.strings("options")?,
                },
                "dropdown" => DesignFieldKind::Dropdown {
                    options: a.strings("options")?,
                },
                _ => return Err(err("Unknown field kind")),
            };
            e.design_form(
                doc,
                FormDesignEdit::Add(NewFormField {
                    page,
                    name: a.s("name")?.into(),
                    required: a.b("required", false)?,
                    bounds: a.bounds()?,
                    kind,
                }),
            )?;
        }
        "form.update" | "form.delete" => {
            let id = inspect::object_id(a, "id")?;
            let field = e
                .designed_fields(doc)?
                .into_iter()
                .find(|f| f.id == id && f.page == page)
                .ok_or_else(|| err("Designed field not found"))?;
            let edit = if op.ends_with("delete") {
                FormDesignEdit::Delete(field)
            } else {
                FormDesignEdit::Update {
                    name: a.text("name", &field.name)?.into(),
                    required: a.b("required", field.required)?,
                    bounds: if a.0.get("bounds").is_some() {
                        a.bounds()?
                    } else {
                        field.bounds
                    },
                    field,
                }
            };
            e.design_form(doc, edit)?;
        }
        "form.fill" => {
            let index = a.index("index")?;
            let field = e
                .form_fields(doc)?
                .into_iter()
                .find(|f| f.page == page && usize::from(f.index) == index)
                .ok_or_else(|| err("Form field not found"))?;
            let key = match &field.value {
                FieldValue::Text(_) => "text",
                FieldValue::Check(_) | FieldValue::Radio(_) => "checked",
                FieldValue::Choice { .. } => "selected",
            };
            if a.0.get(key).is_none()
                || ["text", "checked", "selected"]
                    .iter()
                    .any(|k| *k != key && a.0.get(k).is_some())
            {
                return Err(err(&format!("This field requires only the {key} value")));
            }
            let value = match &field.value {
                FieldValue::Text(_) => FieldValue::Text(a.s("text")?.into()),
                FieldValue::Check(_) => FieldValue::Check(a.b("checked", false)?),
                FieldValue::Radio(_) => FieldValue::Radio(a.b("checked", false)?),
                FieldValue::Choice { options, .. } => FieldValue::Choice {
                    options: options.clone(),
                    selected: if a.0.get("selected").is_none_or(V::is_null) {
                        None
                    } else {
                        Some(a.index("selected")?)
                    },
                },
            };
            e.fill_field(doc, &field, &value)?;
        }
        "form.flatten" => {
            return Ok(json!({"fields":e.flatten_forms(doc)?}));
        }
        "bookmark.add" | "bookmark.rename" | "bookmark.delete" | "bookmark.move" => {
            let inventory = e.bookmarks(doc)?;
            let selected = if op == "bookmark.add" {
                None
            } else {
                Some(inspect::object_id(a, "id")?)
            };
            if selected.is_some_and(|id| !inventory.items.iter().any(|b| b.id == id)) {
                return Err(err("Bookmark not found"));
            }
            let action = match op {
                "bookmark.add" => {
                    let parent = if a.0.get("parent").is_none_or(V::is_null) {
                        None
                    } else {
                        Some(inspect::object_id(a, "parent")?)
                    };
                    if parent.is_some_and(|id| !inventory.items.iter().any(|b| b.id == id)) {
                        return Err(err("Bookmark parent not found"));
                    }
                    BookmarkEdit::Add {
                        title: a.s("title")?.into(),
                        page,
                        parent,
                    }
                }
                "bookmark.rename" => BookmarkEdit::Rename {
                    id: selected.ok_or_else(|| err("Bookmark missing"))?,
                    title: a.s("title")?.into(),
                },
                "bookmark.delete" => BookmarkEdit::Delete {
                    id: selected.ok_or_else(|| err("Bookmark missing"))?,
                },
                _ => BookmarkEdit::Move {
                    id: selected.ok_or_else(|| err("Bookmark missing"))?,
                    position: a.index("position")?,
                },
            };
            e.edit_bookmarks(doc, inventory.revision, action)?;
        }
        "stamp" => e.stamp_page_range(doc, &stamp(a)?)?,
        "bates" => e.stamp_bates(
            doc,
            &stamp(a)?,
            &BatesNumber {
                prefix: a.text("prefix", "")?.into(),
                start: if a.0.get("start").is_some() {
                    a.u("start")?
                } else {
                    1
                },
                digits: if a.0.get("digits").is_some() {
                    byte(a.0.get("digits").ok_or_else(|| err("Missing digits"))?)?
                } else {
                    6
                },
            },
        )?,
        "ocr" => {
            let r = e.ocr_pages(doc, &a.pages()?)?;
            return Ok(
                json!({"pages_recognized":r.pages_recognized,"words_added":r.words_added,"skipped_text":r.skipped_text,"skipped_empty":r.skipped_empty}),
            );
        }
        "compress" => {
            let r = e.compress_images(
                doc,
                u32::try_from(a.integer("edge", 1600)?).map_err(|_| err("Invalid image edge"))?,
                u8::try_from(a.integer("quality", 80)?).map_err(|_| err("Invalid quality"))?,
            )?;
            return Ok(json!({"report":format!("{r:?}")}));
        }
        "scan.cleanup" => {
            let mut options = ScanOptions::default();
            if let Some(v) = a.0.get("corners") {
                let corners = v
                    .as_array()
                    .filter(|v| v.len() == 4)
                    .ok_or_else(|| err("Four scan corners are required"))?;
                for (target, source) in options.corners.iter_mut().zip(corners) {
                    let p = source
                        .as_array()
                        .filter(|v| v.len() == 2)
                        .ok_or_else(|| err("Corner must be [x,y]"))?;
                    *target = [number(&p[0])?, number(&p[1])?];
                }
            }
            options.deskew_degrees = a.n("deskew", 0.)?;
            options.color = match a.text("color", "color")? {
                "color" => ScanColor::Color,
                "grayscale" => ScanColor::Grayscale,
                "black_white" => ScanColor::BlackWhite,
                _ => return Err(err("Unknown scan color mode")),
            };
            options.ocr = a.b("ocr", false)?;
            e.cleanup_scan(doc, page, &options)?;
        }
        "scan.preview" => {
            let picture = e.scan_preview(doc, page)?;
            let corners = detect_scan_corners(&picture).ok();
            let image = Image {
                width: picture.width,
                height: picture.height,
                rgba: picture.rgba,
            };
            write_png(&s.path(a.s("path")?, true, false)?, &image)?;
            return Ok(json!({"corners":corners}));
        }
        "privacy.preview" => {
            let query = match a.s("kind")? {
                "text" => PrivacyQuery::Text(a.s("text")?.into()),
                "email" => PrivacyQuery::Email,
                "phone" => PrivacyQuery::Phone,
                "iban" => PrivacyQuery::Iban,
                "finnish_id" => PrivacyQuery::FinnishId,
                "credit_card" => PrivacyQuery::CreditCard,
                _ => return Err(err("Unknown privacy search kind")),
            };
            let preview = e.preview_redactions(doc, &query)?;
            s.preview_number += 1;
            let token = format!("{}:{}:{}", s.number, doc.revision(), s.preview_number);
            let matches=preview.matches.iter().map(|m|json!({"page":m.page,"bounds":m.bounds.iter().copied().map(rect).collect::<Vec<_>>()})).collect::<Vec<_>>();
            s.preview = Some((token.clone(), preview));
            return Ok(json!({"preview":token,"matches":matches}));
        }
        "privacy.apply" => {
            let (token, preview) = s
                .preview
                .as_ref()
                .ok_or_else(|| err("Preview redactions first"))?;
            if token != a.s("preview")? {
                return Err(err("Privacy preview is stale"));
            }
            e.apply_redaction_preview(doc, preview)?;
        }
        "redact" => e.redact_regions(doc, &regions(a)?)?,
        "sanitize" => return Ok(json!({"report":e.sanitize(doc,sanitize(a)?)?.to_string()})),
        "export.sanitized" => {
            return Ok(
                json!({"report":e.export_sanitized(doc,sanitize(a)?,&s.path(a.s("path")?,true,false)?)?.to_string()}),
            );
        }
        "render" => e.export_page_image(
            doc,
            page,
            u16::try_from(a.integer("dpi", 144)?).map_err(|_| err("Invalid DPI"))?,
            image_format(a.text("format", "png")?)?,
            &s.path(a.s("path")?, true, false)?,
        )?,
        "export.images" => {
            let selected = if a.0.get("pages").is_some() {
                a.pages()?
            } else {
                (0..e.pages(doc)?.len()).map(|p| p as u16).collect()
            };
            let paths = e.export_page_images(
                doc,
                &selected,
                u16::try_from(a.integer("dpi", 144)?).map_err(|_| err("Invalid DPI"))?,
                image_format(a.text("format", "png")?)?,
                &s.path(a.s("folder")?, false, true)?,
            )?;
            return Ok(json!({"files":paths.len()}));
        }
        "export.word" => e.export_word(doc, &s.path(a.s("path")?, true, false)?)?,
        "table.detect" => {
            let table = e.detect_table(doc, page, a.bounds()?)?;
            let result = json!({"cells":table.cells});
            s.table = Some((doc.revision(), table));
            return Ok(result);
        }
        "table.edit" => {
            let table = s
                .table
                .as_mut()
                .filter(|(rev, _)| *rev == doc.revision())
                .ok_or_else(|| err("Detect a current table first"))?;
            let cells = a
                .array("cells")?
                .iter()
                .map(|v| Args(&json!({"row":v})).strings("row"))
                .collect::<Result<Vec<_>>>()?;
            if cells
                .iter()
                .map(|row| row.iter().map(String::len).sum::<usize>())
                .sum::<usize>()
                > 4_000_000
            {
                return Err(err("Table text exceeds limits"));
            }
            table.1.cells = cells;
        }
        "table.export" => {
            let table = s
                .table
                .as_ref()
                .filter(|(rev, _)| *rev == doc.revision())
                .ok_or_else(|| err("Detect a current table first"))?;
            let format = match a.text("format", "csv")? {
                "csv" => TableFormat::Csv,
                "xlsx" => TableFormat::Xlsx,
                _ => return Err(err("Table format must be csv or xlsx")),
            };
            table
                .1
                .export(&s.path(a.s("path")?, true, false)?, format)?;
        }
        "compare.load" => {
            s.reference = Some(e.open_with_password(
                &s.path(a.s("path")?, false, false)?,
                a.text("password", "")?,
            )?);
        }
        "compare" => {
            let reference = s
                .reference
                .as_ref()
                .ok_or_else(|| err("Choose a comparison document first"))?;
            let pair = PagePair {
                left: if a.0.get("page").is_some_and(V::is_null) {
                    None
                } else {
                    Some(page)
                },
                right: if a.0.get("revised_page").is_some_and(V::is_null) {
                    None
                } else {
                    Some(if a.0.get("revised_page").is_some() {
                        a.page_key("revised_page")?
                    } else {
                        page
                    })
                },
            };
            let c = e.compare_page_pair(reference, doc, pair, a.b("visual", false)?)?;
            return Ok(
                json!({"changed_pixels":c.changed_pixels,"changes":c.changes.iter().map(|c|json!({"kind":format!("{:?}",c.kind),"before":c.before,"after":c.after,"left":c.left.map(rect),"right":c.right.map(rect)})).collect::<Vec<_>>()}),
            );
        }
        _ => return Err(err("Editing command is not implemented")),
    }
    Ok(json!({"applied":true}))
}
fn edit_result(result: EditResult) -> V {
    json!({"objects_changed":result.objects_changed,"warning":result.warning})
}
fn placement(a: &Args<'_>, page: u16) -> Result<Placement> {
    Ok(Placement {
        page,
        left: a.n("left", 0.)?,
        bottom: a.n("bottom", 0.)?,
        width: a.n("width", 100.)?,
        height: a.n("height", 100.)?,
    })
}
fn markup(a: &Args<'_>, old: Option<&Markup>) -> Result<Markup> {
    let kind = if a.0.get("kind").is_none() {
        old.map_or(MarkupKind::Note, |o| o.kind)
    } else {
        match a.s("kind")? {
            "highlight" => MarkupKind::Highlight,
            "underline" => MarkupKind::Underline,
            "strikethrough" => MarkupKind::Strikethrough,
            "note" => MarkupKind::Note,
            _ => return Err(err("Unknown markup kind")),
        }
    };
    Ok(Markup {
        page: a.page()?,
        kind,
        bounds: if a.0.get("bounds").is_some() {
            a.bounds()?
        } else {
            old.ok_or_else(|| err("Missing bounds"))?.bounds
        },
        color: if a.0.get("color").is_some() {
            a.color()?
        } else {
            old.map_or([255, 210, 40], |o| o.color)
        },
        opacity: a.n("opacity", old.map_or(0.5, |o| o.opacity))?,
        text: a.text("text", old.map_or("", |o| o.text.as_str()))?.into(),
    })
}
fn link(a: &Args<'_>, old: Option<&LinkSpec>) -> Result<LinkSpec> {
    let target = match (a.0.get("uri"), a.0.get("target_page")) {
        (Some(_), None) => LinkTarget::uri(a.s("uri")?)?,
        (None, Some(_)) => LinkTarget::Page(a.page_key("target_page")?),
        (None, None) => old
            .ok_or_else(|| err("Choose uri or target_page"))?
            .target
            .clone(),
        _ => return Err(err("Choose only one link destination")),
    };
    Ok(LinkSpec {
        page: a.page()?,
        bounds: if a.0.get("bounds").is_some() {
            a.bounds()?
        } else {
            old.ok_or_else(|| err("Missing link bounds"))?.bounds
        },
        target,
    })
}
fn regions(a: &Args<'_>) -> Result<Vec<(u16, Bounds)>> {
    a.array("regions")?
        .iter()
        .map(|v| {
            let r = Args(v);
            if v.as_object()
                .is_none_or(|o| o.keys().any(|k| k != "page" && k != "bounds"))
            {
                return Err(err("Regions contain page and bounds only"));
            }
            Ok((r.page()?, r.bounds()?))
        })
        .collect()
}
fn search(a: &Args<'_>) -> Result<SearchOptions> {
    Ok(SearchOptions {
        case_sensitive: a.b("case_sensitive", false)?,
        whole_word: a.b("whole_word", false)?,
        pages: if a.0.get("pages").is_some() {
            Some(a.pages()?)
        } else {
            None
        },
        regions: if a.0.get("regions").is_some() {
            regions(a)?
        } else {
            vec![]
        },
    })
}
fn sanitize(a: &Args<'_>) -> Result<SanitizeOptions> {
    Ok(SanitizeOptions {
        remove_comments: a.b("comments", false)?,
        remove_links: a.b("links", false)?,
        flatten_visible: a.b("flatten", false)?,
    })
}
fn image_format(value: &str) -> Result<PageImageFormat> {
    match value {
        "png" => Ok(PageImageFormat::Png),
        "jpg" | "jpeg" => Ok(PageImageFormat::Jpeg),
        _ => Err(err("Image format must be png or jpg")),
    }
}
fn stamp(a: &Args<'_>) -> Result<RangeStamp> {
    Ok(RangeStamp {
        stamp: TextStamp {
            template: a.text("text", "")?.into(),
            font: a.text("font", "Arial")?.into(),
            size: a.n("size", 12.)?,
            color: a.color()?,
            opacity: a.n("opacity", 1.)?,
            x: a.n("x", 30.)?,
            y: a.n("y", 24.)?,
        },
        pages: a.text("pages", "")?.into(),
        date: a.text("date", "")?.into(),
        placement: match a.text("placement", "footer")? {
            "absolute" => StampPlacement::Absolute,
            "header" => StampPlacement::Header,
            "footer" => StampPlacement::Footer,
            _ => return Err(err("Unknown stamp placement")),
        },
    })
}
fn write_png(path: &Path, image: &Image) -> Result<()> {
    use image::ImageEncoder;
    let mut bytes = vec![];
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &image.rgba,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|_| err("Image encoding failed"))?;
    save::write_new(path, &bytes)?;
    Ok(())
}
