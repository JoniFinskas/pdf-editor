use super::*;

pub(super) fn inspect(e: &Engine, doc: &Document, a: &Args<'_>) -> Result<V> {
    let offset = a.0.get("offset").map_or(Ok(0), |_| a.index("offset"))?;
    let limit = a.0.get("limit").map_or(Ok(200), |_| a.index("limit"))?;
    if limit == 0 || limit > 1000 {
        return Err(err("Inspect limit must be 1–1000"));
    }
    let page = a.page()?;
    let records:Vec<V>=match a.text("view","pages")? {
        "pages"=>e.pages(doc)?.into_iter().enumerate().map(|(page,p)|json!({"page":page,"width":p.width,"height":p.height})).collect(),
        "text"=>e.text_objects(doc,page)?.into_iter().map(|o|json!({"page":o.page,"index":o.index,"text":o.text,"font":o.font,"size":o.size,"color":&o.color[..3],"alpha":o.color[3],"bounds":rect(o.bounds),"matrix":o.matrix})).collect(),
        "objects"=>e.page_items(doc,page)?.into_iter().map(|o|json!({"page":o.page,"index":o.index,"kind":o.kind,"bounds":rect(o.bounds)})).collect(),
        "paragraphs"=>paragraphs(&e.text_objects(doc,page)?).into_iter().enumerate().map(|(index,o)|json!({"index":index,"text":o.text,"bounds":rect(o.bounds),"indices":o.objects.iter().map(|o|o.index).collect::<Vec<_>>(),"fallback":o.fallback,"line_spacing":o.line_spacing,"alignment":format!("{:?}",o.alignment)})).collect(),
        "annotations"=>e.annotations(doc)?.into_iter().filter(|o|o.markup.page==page).map(|o|json!({"page":o.markup.page,"index":o.index,"kind":format!("{:?}",o.markup.kind),"bounds":rect(o.markup.bounds),"color":o.markup.color,"opacity":o.markup.opacity,"text":o.markup.text})).collect(),
        "links"=>e.links(doc)?.into_iter().filter(|o|o.link.page==page).map(|o|{
            let target=match o.link.target {LinkTarget::Uri(uri)=>json!({"uri":uri}),LinkTarget::Page(page)=>json!({"target_page":page}),LinkTarget::Unsupported=>json!({"unsupported":true})};
            json!({"page":o.link.page,"index":o.index,"bounds":rect(o.link.bounds),"target":target})
        }).collect(),
        "pictures"=>e.placed_pictures(doc)?.into_iter().filter(|o|o.page==page).map(|o|json!({"page":o.page,"name":o.name,"left":o.placement.left,"bottom":o.placement.bottom,"width":o.placement.width,"height":o.placement.height})).collect(),
        "forms"=>e.form_fields(doc)?.into_iter().filter(|o|o.page==page).map(|o|{
            let value=match o.value {FieldValue::Text(v)=>json!({"text":v}),FieldValue::Check(v)|FieldValue::Radio(v)=>json!({"checked":v}),FieldValue::Choice {options,selected}=>json!({"options":options,"selected":selected})};
            json!({"page":o.page,"index":o.index,"name":o.name,"read_only":o.read_only,"value":value})
        }).collect(),
        "form_design"=>e.designed_fields(doc)?.into_iter().filter(|o|o.page==page).map(|o|json!({"page":o.page,"id":id(o.id),"name":o.name,"kind":o.kind,"required":o.required,"bounds":rect(o.bounds)})).collect(),
        "bookmarks"=>e.bookmarks(doc)?.items.into_iter().map(|o|json!({"id":id(o.id),"title":o.title,"page":o.page,"depth":o.depth,"sibling":o.sibling,"siblings":o.siblings})).collect(),
        _=>return Err(err("Unknown inspection view")),
    };
    let count = records.len();
    let records = records
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    Ok(json!({"total":count,"offset":offset,"items":records}))
}
pub(super) fn id(value: lopdf::ObjectId) -> String {
    format!("{}:{}", value.0, value.1)
}
pub(super) fn object_id(a: &Args<'_>, key: &str) -> Result<lopdf::ObjectId> {
    let (left, right) = a
        .s(key)?
        .split_once(':')
        .ok_or_else(|| err("Use an id from the current inventory"))?;
    Ok((
        left.parse().map_err(|_| err("Invalid object id"))?,
        right.parse().map_err(|_| err("Invalid object id"))?,
    ))
}
