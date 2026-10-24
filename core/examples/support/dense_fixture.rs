//! Deterministic dense pages using only the committed generated CID fixture.
use lopdf::{
    Document, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
pub fn dense_page(seed: &[u8], count: usize) -> Vec<u8> {
    let mut pdf = Document::load_mem(seed).unwrap();
    let page = pdf.get_pages()[&1];
    pdf.get_dictionary_mut(page).unwrap().set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
    let source = Content::decode(&pdf.get_page_content(page)).unwrap();
    let font = source
        .operations
        .iter()
        .find(|op| op.operator == "Tf")
        .unwrap()
        .operands[0]
        .clone();
    let text = source
        .operations
        .iter()
        .find(|op| matches!(op.operator.as_str(), "Tj" | "TJ"))
        .unwrap()
        .clone();
    let mut operations = Vec::new();
    for i in 0..count {
        operations.push(Operation::new("BT", vec![]));
        operations.push(Operation::new("Tf", vec![font.clone(), 5.into()]));
        operations.push(Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                Object::Real(10.0 + (i % 4) as f32 * 150.0),
                Object::Real(780.0 - (i / 4) as f32 * 3.0),
            ],
        ));
        operations.push(text.clone());
        operations.push(Operation::new("ET", vec![]));
    }
    let stream = pdf.add_object(Stream::new(
        dictionary! {},
        Content { operations }.encode().unwrap(),
    ));
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}
