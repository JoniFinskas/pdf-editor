//! Generated cursor-dependent runs, with either independent lines or one chain.
use lopdf::{
    Document, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
pub fn cursor_page(seed: &[u8], count: usize, chain: bool) -> Vec<u8> {
    let mut pdf = Document::load_mem(seed).unwrap();
    let page = pdf.get_pages()[&1];
    pdf.get_dictionary_mut(page).unwrap().set(
        "MediaBox",
        vec![
            0.into(),
            0.into(),
            Object::Real(if chain {
                (count as f32 * 40.0 + 40.0).min(14400.0)
            } else {
                612.0
            }),
            792.into(),
        ],
    );
    let source = Content::decode(&pdf.get_page_content(page)).unwrap();
    let font = source
        .operations
        .iter()
        .find(|op| op.operator == "Tf")
        .unwrap()
        .operands[0]
        .clone();
    let first = source
        .operations
        .iter()
        .find(|op| op.operator == "Tj")
        .unwrap()
        .clone();
    let bytes = first.operands[0].as_str().unwrap();
    let second = Operation::new(
        "Tj",
        vec![Object::String(
            bytes[bytes.len() - 2..].to_vec(),
            lopdf::StringFormat::Hexadecimal,
        )],
    );
    let mut operations = vec![];
    for i in 0..count {
        if !chain || i == 0 {
            operations.push(Operation::new("BT", vec![]));
            operations.push(Operation::new("Tf", vec![font.clone(), 5.into()]));
            operations.push(Operation::new(
                "Tm",
                vec![
                    1.into(),
                    0.into(),
                    0.into(),
                    1.into(),
                    Object::Real(10.0 + (i % 3) as f32 * 200.0),
                    Object::Real(780.0 - (i / 3) as f32 * 7.0),
                ],
            ));
        }
        operations.push(first.clone());
        operations.push(second.clone());
        if !chain || i + 1 == count {
            operations.push(Operation::new("ET", vec![]));
        }
    }
    let stream = pdf.add_object(Stream::new(
        dictionary! {},
        Content { operations }.encode().unwrap(),
    ));
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    bytes
}
