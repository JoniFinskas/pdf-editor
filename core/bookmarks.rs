use crate::*;
use lopdf::{Dictionary, Object, ObjectId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Bookmark {
    pub id: ObjectId,
    pub title: String,
    pub page: Option<u16>,
    pub depth: usize,
    pub sibling: usize,
    pub siblings: usize,
}
#[derive(Clone, Debug, Default)]
pub struct BookmarkSet {
    pub revision: u64,
    pub items: Vec<Bookmark>,
}
pub enum BookmarkEdit {
    Add {
        title: String,
        page: u16,
        parent: Option<ObjectId>,
    },
    Rename {
        id: ObjectId,
        title: String,
    },
    Delete {
        id: ObjectId,
    },
    Move {
        id: ObjectId,
        position: usize,
    },
}
struct Node {
    id: ObjectId,
    dict: Dictionary,
    children: Vec<Node>,
}
fn reference(dict: &Dictionary, key: &[u8]) -> Result<Option<ObjectId>> {
    match dict.get(key) {
        Ok(Object::Null) | Err(_) => Ok(None),
        Ok(value) => Ok(Some(value.as_reference()?)),
    }
}
fn read_nodes(
    pdf: &lopdf::Document,
    mut next: Option<ObjectId>,
    depth: usize,
    seen: &mut BTreeSet<ObjectId>,
) -> Result<Vec<Node>> {
    if depth > 64 {
        return Err(err("Bookmark nesting exceeds 64 levels"));
    }
    let mut nodes = vec![];
    while let Some(id) = next {
        if seen.len() >= 10000 || !seen.insert(id) {
            return Err(err("Invalid or excessive bookmark tree"));
        }
        let dict = pdf.get_dictionary(id)?.clone();
        let children = read_nodes(pdf, reference(&dict, b"First")?, depth + 1, seen)?;
        next = reference(&dict, b"Next")?;
        nodes.push(Node { id, dict, children });
    }
    Ok(nodes)
}
fn tree(pdf: &lopdf::Document) -> Result<Vec<Node>> {
    let Some(root) = reference(pdf.catalog()?, b"Outlines")? else {
        return Ok(vec![]);
    };
    read_nodes(
        pdf,
        reference(pdf.get_dictionary(root)?, b"First")?,
        0,
        &mut BTreeSet::from([root]),
    )
}
fn named_tree(
    pdf: &lopdf::Document,
    object: &Object,
    names: &mut BTreeMap<Vec<u8>, Object>,
    visited: &mut usize,
    depth: usize,
) -> Result<()> {
    *visited += 1;
    if depth > 64 || *visited > 10000 {
        return Err(err("Invalid destination name tree"));
    }
    let dict = pdf.dereference(object)?.1.as_dict()?;
    if let Ok(items) = dict.get(b"Names") {
        let items = pdf.dereference(items)?.1.as_array()?;
        if items.len() % 2 != 0 || items.len() > 20000 {
            return Err(err("Invalid destination names"));
        }
        for pair in items.as_chunks::<2>().0 {
            names.insert(pair[0].as_str()?.to_vec(), pair[1].clone());
        }
    }
    if let Ok(children) = dict.get(b"Kids") {
        for child in pdf.dereference(children)?.1.as_array()? {
            named_tree(pdf, child, names, visited, depth + 1)?;
        }
    }
    Ok(())
}
fn destinations(pdf: &lopdf::Document) -> Result<BTreeMap<Vec<u8>, Object>> {
    let mut names = BTreeMap::new();
    if let Ok(old) = pdf.catalog()?.get(b"Dests") {
        for (key, value) in pdf.dereference(old)?.1.as_dict()? {
            names.insert(key.clone(), value.clone());
        }
    }
    if let Ok(root) = pdf.catalog()?.get(b"Names")
        && let Ok(dests) = pdf.dereference(root)?.1.as_dict()?.get(b"Dests")
    {
        named_tree(pdf, dests, &mut names, &mut 0, 0)?;
    }
    Ok(names)
}
fn destination(
    pdf: &lopdf::Document,
    value: &Object,
    pages: &BTreeMap<ObjectId, u16>,
    names: &BTreeMap<Vec<u8>, Object>,
    depth: usize,
) -> Option<u16> {
    if depth > 8 {
        return None;
    }
    match pdf.dereference(value).ok()?.1 {
        Object::Array(values) => match values.first()? {
            Object::Reference(id) => pages.get(id).copied(),
            Object::Integer(index) => u16::try_from(*index)
                .ok()
                .filter(|i| usize::from(*i) < pages.len()),
            _ => None,
        },
        Object::Dictionary(dict) => destination(pdf, dict.get(b"D").ok()?, pages, names, depth + 1),
        Object::Name(key) | Object::String(key, _) => {
            destination(pdf, names.get(key)?, pages, names, depth + 1)
        }
        _ => None,
    }
}
fn flatten(
    pdf: &lopdf::Document,
    nodes: &[Node],
    depth: usize,
    pages: &BTreeMap<ObjectId, u16>,
    names: &BTreeMap<Vec<u8>, Object>,
    items: &mut Vec<Bookmark>,
) -> Result<()> {
    for (sibling, node) in nodes.iter().enumerate() {
        let title = lopdf::decode_text_string(node.dict.get(b"Title")?)?;
        let dest = node.dict.get(b"Dest").ok().or_else(|| {
            let action = pdf
                .dereference(node.dict.get(b"A").ok()?)
                .ok()?
                .1
                .as_dict()
                .ok()?;
            if action.get(b"S").ok()?.as_name().ok()? != b"GoTo" {
                return None;
            }
            action.get(b"D").ok()
        });
        let page = dest.and_then(|d| destination(pdf, d, pages, names, 0));
        items.push(Bookmark {
            id: node.id,
            title,
            page,
            depth,
            sibling,
            siblings: nodes.len(),
        });
        flatten(pdf, &node.children, depth + 1, pages, names, items)?;
    }
    Ok(())
}
fn find(nodes: &mut [Node], id: ObjectId) -> Option<&mut Node> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}
fn siblings(nodes: &mut Vec<Node>, id: ObjectId) -> Option<&mut Vec<Node>> {
    if nodes.iter().any(|n| n.id == id) {
        return Some(nodes);
    }
    for node in nodes {
        if let Some(found) = siblings(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}
fn title(value: &str) -> Result<Object> {
    if value.trim().is_empty() || value.chars().count() > 16384 {
        return Err(err("Bookmark title must contain 1–16384 characters"));
    }
    let bytes = [
        vec![0xfe, 0xff],
        value.encode_utf16().flat_map(u16::to_be_bytes).collect(),
    ]
    .concat();
    Ok(Object::String(bytes, lopdf::StringFormat::Hexadecimal))
}
fn write_nodes(pdf: &mut lopdf::Document, nodes: &[Node], parent: ObjectId) -> usize {
    let mut total = 0;
    for (index, node) in nodes.iter().enumerate() {
        let mut dict = node.dict.clone();
        for key in [
            b"Parent".as_slice(),
            b"Prev",
            b"Next",
            b"First",
            b"Last",
            b"Count",
        ] {
            dict.remove(key);
        }
        dict.set("Parent", parent);
        if index > 0 {
            dict.set("Prev", nodes[index - 1].id);
        }
        if let Some(next) = nodes.get(index + 1) {
            dict.set("Next", next.id);
        }
        let count = write_nodes(pdf, &node.children, node.id);
        if let (Some(first), Some(last)) = (node.children.first(), node.children.last()) {
            dict.set("First", first.id);
            dict.set("Last", last.id);
            dict.set("Count", count as i64);
        }
        pdf.objects.insert(node.id, Object::Dictionary(dict));
        total += 1 + count;
    }
    total
}
impl Engine {
    pub fn bookmarks(&self, doc: &Document) -> Result<BookmarkSet> {
        guarded(|| {
            let pdf = parse(&doc.bytes)?;
            let nodes = tree(&pdf)?;
            let names = destinations(&pdf)?;
            let pages = pdf
                .get_pages()
                .into_iter()
                .map(|(n, id)| (id, (n - 1) as u16))
                .collect();
            let mut items = vec![];
            flatten(&pdf, &nodes, 0, &pages, &names, &mut items)?;
            Ok(BookmarkSet {
                revision: doc.revision,
                items,
            })
        })
    }
    pub fn edit_bookmarks(
        &self,
        doc: &mut Document,
        revision: u64,
        edit: BookmarkEdit,
    ) -> Result<()> {
        guarded(|| {
            if revision != doc.revision {
                return Err(err("Bookmarks changed; refresh the sidebar"));
            }
            let mut pdf = parse(&doc.bytes)?;
            let mut nodes = tree(&pdf)?;
            match edit {
                BookmarkEdit::Add {
                    title: text,
                    page,
                    parent,
                } => {
                    let target = *pdf
                        .get_pages()
                        .get(&(u32::from(page) + 1))
                        .ok_or_else(|| err("Invalid bookmark page"))?;
                    let mut dict = Dictionary::new();
                    dict.set("Title", title(&text)?);
                    dict.set(
                        "Dest",
                        vec![Object::Reference(target), Object::Name(b"Fit".to_vec())],
                    );
                    let node = Node {
                        id: pdf.new_object_id(),
                        dict,
                        children: vec![],
                    };
                    if let Some(parent) = parent {
                        find(&mut nodes, parent)
                            .ok_or_else(|| err("Bookmark no longer exists"))?
                            .children
                            .push(node);
                    } else {
                        nodes.push(node);
                    }
                }
                BookmarkEdit::Rename { id, title: text } => {
                    find(&mut nodes, id)
                        .ok_or_else(|| err("Bookmark no longer exists"))?
                        .dict
                        .set("Title", title(&text)?);
                }
                BookmarkEdit::Delete { id } => {
                    siblings(&mut nodes, id)
                        .ok_or_else(|| err("Bookmark no longer exists"))?
                        .retain(|n| n.id != id);
                }
                BookmarkEdit::Move { id, position } => {
                    let list =
                        siblings(&mut nodes, id).ok_or_else(|| err("Bookmark no longer exists"))?;
                    if position >= list.len() {
                        return Err(err("Invalid bookmark position"));
                    }
                    let index = list
                        .iter()
                        .position(|n| n.id == id)
                        .ok_or_else(|| err("Bookmark no longer exists"))?;
                    let node = list.remove(index);
                    list.insert(position, node);
                }
            }
            let catalog = pdf.trailer.get(b"Root")?.as_reference()?;
            if nodes.is_empty() {
                pdf.get_dictionary_mut(catalog)?.remove(b"Outlines");
            } else {
                let root = pdf.new_object_id();
                let count = write_nodes(&mut pdf, &nodes, root);
                let mut dict = Dictionary::new();
                dict.set("Type", Object::Name(b"Outlines".to_vec()));
                dict.set("First", nodes[0].id);
                dict.set("Last", nodes[nodes.len() - 1].id);
                dict.set("Count", count as i64);
                pdf.objects.insert(root, Object::Dictionary(dict));
                pdf.get_dictionary_mut(catalog)?.set("Outlines", root);
            }
            let candidate = self.candidate(doc, pdf)?;
            self.bookmarks(&candidate)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
