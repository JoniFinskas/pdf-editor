use super::*;

pub(super) fn describe() -> V {
    json!({"protocol":1,"coordinates":"PDF points, bottom-left origin; zero-based pages and native inventory indices. Paragraph indices use the paragraphs inventory.",
        "input":"one JSON object per line","limits":{"line_bytes":MAX_COMMAND_BYTES,"commands":10000,"response_bytes":8_388_608},
        "selection":"Mutations require document and revision from the last response. Explicit expected state is also checked on reads and exports.",
        "files":"Only relative paths inside --root; existing output files, redirected paths and traversal are rejected. Folder '.' means the workspace root.",
        "excluded":["network","shell","PDF actions","physical printing","OS configuration","persistent signature library"],
        "commands":OPS.iter().map(|(op,fields,revision)|{
            let mut properties=serde_json::Map::new();properties.insert("op".into(),json!({"const":op}));
            properties.insert("request_id".into(),json!({"type":["string","number"],"description":"Optional correlation value; strings at most 128 bytes"}));
            properties.insert("document".into(),integer());properties.insert("revision".into(),integer());
            for field in fields.split_whitespace(){properties.insert(field.into(),field_schema(op,field));}
            let mut required=vec!["op"];if *revision {required.extend(["document","revision"]);}
            required.extend(required_fields(op).split_whitespace());
            json!({"op":op,"input_schema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
        }).collect::<Vec<_>>(),
        "examples":[{"op":"new","preset":"a4","pages":1},{"op":"text.add","document":1,"revision":0,"page":0,"x":42,"y":760,"text":"Example heading","font":"Arial","size":20},
            {"op":"inspect","view":"text","page":0},{"op":"save","document":1,"revision":1,"path":"example.pdf"}],
        "cv":"Single-column selectable text with wrapping and pagination. cv.check returns extraction and file warnings, never an ATS score or screening guarantee."})
}
fn integer() -> V {
    json!({"type":"integer","minimum":0})
}
fn strings() -> V {
    json!({"type":"array","items":{"type":"string"},"maxItems":10000})
}
fn rectangle() -> V {
    json!({"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4,"description":"left, bottom, right, top"})
}
fn field_schema(op: &str, field: &str) -> V {
    match field {
        "bounds" => rectangle(),
        "regions" => {
            json!({"type":"array","maxItems":10000,"items":{"type":"object","properties":{"page":integer(),"bounds":rectangle()},"required":["page","bounds"],"additionalProperties":false}})
        }
        "points" | "corners" => {
            json!({"type":"array","maxItems":10000,"items":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"description":if field=="corners" {"Four normalized clockwise corners, starting top left"}else{"PDF-point x/y pairs"}})
        }
        "color" if op == "scan.cleanup" => json!({"enum":["color","grayscale","black_white"]}),
        "color" => {
            json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer","minimum":0,"maximum":255}})
        }
        "pages" if op == "new" => json!({"type":"integer","minimum":1,"maximum":1000,"default":1}),
        "pages" if matches!(op, "stamp" | "bates") => {
            json!({"type":"string","description":"One-based ranges, e.g. 1,3-5; empty means all"})
        }
        "pages" | "indices" => {
            json!({"type":"array","items":integer(),"maxItems":10000,"uniqueItems":true})
        }
        "paths" | "contact" | "options" => strings(),
        "sections" => {
            json!({"type":"array","maxItems":if op=="cv.draft.write" {21}else{20},"items":{"type":"object","properties":{"heading":{"type":"string","maxLength":if op=="cv.draft.write" {64000}else{120}},"body":{"type":"string"}},"required":["heading","body"],"additionalProperties":false}})
        }
        "cells" => json!({"type":"array","maxItems":10000,"items":strings()}),
        "discard" | "landscape" | "letter" | "case_sensitive" | "whole_word" | "bold"
        | "italic" | "required" | "checked" | "ocr" | "comments" | "links" | "flatten"
        | "visual" => json!({"type":"boolean"}),
        "x" | "y" | "dx" | "dy" | "width" | "height" | "left" | "bottom" | "size" | "opacity"
        | "deskew"
            if !(field == "size" && op == "images.create") =>
        {
            json!({"type":"number"})
        }
        "page" | "revised_page" if op == "compare" => {
            json!({"type":["integer","null"],"minimum":0})
        }
        "index" | "page" | "target_page" | "revised_page" | "before" | "offset" | "limit"
        | "position" | "edge" | "quality" | "dpi" | "start" | "digits" => integer(),
        "occurrence" | "selected" => json!({"type":["integer","null"],"minimum":0}),
        "id" | "parent" => {
            json!({"type":if field=="parent" {vec!["string","null"]}else{vec!["string"]},"pattern":"^[0-9]+:[0-9]+$","description":"Object id from the current inventory"})
        }
        "preset" => json!({"enum":["a4","a5","letter","custom"],"default":"a4"}),
        "size" => json!({"enum":["a4","letter","image"]}),
        "view" => {
            json!({"enum":["pages","text","objects","paragraphs","annotations","links","pictures","forms","form_design","bookmarks"]})
        }
        "substitution" => json!({"enum":["line","document"]}),
        "language" => json!({"enum":["en-US","fi-FI"],"default":"en-US"}),
        "kind" => match op {
            "draw" => json!({"enum":["freehand","rectangle","ellipse","line","arrow"]}),
            "form.add" => json!({"enum":["text","date","checkbox","radio","dropdown"]}),
            "privacy.preview" => {
                json!({"enum":["text","email","phone","iban","finnish_id","credit_card"]})
            }
            _ => json!({"enum":["highlight","underline","strikethrough","note"]}),
        },
        "action" => json!({"enum":["move","duplicate","resize","delete","front","back"]}),
        "alignment" => {
            json!({"enum":["left","right","top","bottom","horizontal_center","vertical_center","distribute_horizontal","distribute_vertical"]})
        }
        "placement" => json!({"enum":["absolute","header","footer"]}),
        "format" => {
            if op == "table.export" {
                json!({"enum":["csv","xlsx"]})
            } else {
                json!({"enum":["png","jpg"]})
            }
        }
        _ => json!({"type":"string"}),
    }
}
fn required_fields(op: &str) -> &'static str {
    match op {
        "open" | "save" | "compare.load" | "render" | "scan.preview" | "export.word"
        | "export.sanitized" | "cv.draft.read" => "path",
        "images.create" => "paths",
        "cv.create" => "name sections",
        "cv.draft.write" => "path name sections",
        "document.policy" => "substitution",
        "text.add" => "text",
        "text.edit" => "indices text",
        "text.delete" => "indices",
        "text.style" => "index",
        "text.format" => "indices font",
        "paragraph.edit" => "index text",
        "text.replace" => "find text",
        "text.find" => "find",
        "text.spelling" | "objects.ungroup" | "objects.prepare_text" => "index",
        "text.highlight" | "redact" => "regions",
        "objects.edit" => "indices action",
        "objects.layout" => "indices alignment",
        "objects.copy" => "indices",
        "page.blank" => "before",
        "page.move" | "page.duplicate" => "pages before",
        "page.rotate" | "page.delete" | "ocr" => "pages",
        "page.import" => "path before",
        "page.merge" => "paths",
        "page.crop" | "table.detect" => "bounds",
        "page.extract" => "pages path",
        "page.split" => "ranges folder",
        "page.split_bookmarks" | "export.images" => "folder",
        "image.add" => "path",
        "image.move" => "name",
        "draw" => "kind points",
        "annotation.add" => "bounds",
        "annotation.update" | "annotation.delete" | "link.update" | "link.delete" | "form.fill" => {
            "index"
        }
        "link.add" => "bounds",
        "form.add" => "name bounds",
        "form.update" | "form.delete" => "id",
        "bookmark.add" => "title",
        "bookmark.rename" => "id title",
        "bookmark.delete" => "id",
        "bookmark.move" => "id position",
        "privacy.preview" => "kind",
        "privacy.apply" => "preview",
        "table.edit" => "cells",
        "table.export" => "path",
        _ => "",
    }
}
