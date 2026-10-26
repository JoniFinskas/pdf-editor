use super::*;

fn scene(rotation: i64, matrix: [f32; 6], image: bool) -> Vec<u8> {
    let mut pdf = readable_drag_fixture(rotation);
    let page = pdf.get_pages()[&1];
    let [a, b, c, d, x, y] = matrix;
    let artwork = if image {
        let id = pdf.add_object(Stream::new(
            dictionary! {
                "Type"=>"XObject", "Subtype"=>"Image", "Width"=>1, "Height"=>1,
                "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8,
            },
            vec![51, 128, 204],
        ));
        pdf.get_dictionary_mut(page)
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("XObject", dictionary! {"Im"=>id});
        "q 80 0 0 15 100 195 cm /Im Do Q"
    } else {
        "0.2 0.5 0.8 rg 100 195 80 15 re f"
    };
    let content = format!(
        "q {a} {b} {c} {d} {x} {y} cm \
         q 40 190 100 60 re W n \
         BT /F1 20 Tf 1 0 0 1 50 220 Tm (Move me) Tj ET \
         {artwork} Q Q \
         q BT /F1 20 Tf 1 0 0 1 300 50 Tm (Neighbor) Tj ET Q"
    );
    pdf.change_page_content(page, content.into_bytes()).unwrap();
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn mixed_clipped_objects_move_and_resize_match_transformed_scene() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        for image in [false, true] {
            for resize in [false, true] {
                let source = temp(&format!("mixed-gesture-{rotation}-{resize}.pdf"));
                let bytes = scene(rotation, [1., 0., 0., 1., 0., 0.], image);
                fs::write(&source, &bytes).unwrap();
                let mut doc = e.open(&source).unwrap();
                let items = e.page_items(&doc, 0).unwrap();
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].kind, "Text");
                assert_eq!(items[1].kind, if image { "Image" } else { "Path" });
                let mut old = items[0].bounds;
                old.left = old.left.min(items[1].bounds.left);
                old.right = old.right.max(items[1].bounds.right);
                old.bottom = old.bottom.min(items[1].bounds.bottom);
                old.top = old.top.max(items[1].bounds.top);
                let matrix = if resize {
                    [
                        1.3,
                        0.,
                        0.,
                        1.5,
                        160. - old.left * 0.3,
                        -120. - old.bottom * 0.5,
                    ]
                } else {
                    [1., 0., 0., 1., 160., -120.]
                };
                let [sx, _, _, sy, tx, ty] = matrix;
                let action = if resize {
                    ObjectAction::Resize {
                        bounds: Bounds {
                            left: old.left * sx + tx,
                            right: old.right * sx + tx,
                            bottom: old.bottom * sy + ty,
                            top: old.top * sy + ty,
                        },
                    }
                } else {
                    ObjectAction::Move { dx: tx, dy: ty }
                };
                e.edit_page_items(&mut doc, &items[..2], action).unwrap();
                let expected_path = temp(&format!("mixed-expected-{rotation}-{resize}.pdf"));
                fs::write(&expected_path, scene(rotation, matrix, image)).unwrap();
                let expected = e.render(&e.open(&expected_path).unwrap(), 0, 2.).unwrap();
                let actual = e.render(&doc, 0, 2.).unwrap();
                let changed = actual
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .filter(|(a, b)| (i16::from(**a) - i16::from(**b)).abs() > 3)
                    .count();
                assert_eq!(
                    changed, 0,
                    "Clipped mixed selection differs: rotation={rotation}, resize={resize}"
                );
                let transformed = doc.bytes.clone();
                assert!(doc.undo());
                assert_eq!(doc.bytes, bytes);
                assert!(!doc.can_undo());
                assert!(doc.redo());
                assert_eq!(doc.bytes, transformed);
                let saved = temp(&format!("mixed-saved-{rotation}-{resize}.pdf"));
                if saved.exists() {
                    fs::remove_file(&saved).unwrap();
                }
                doc.save_as(&saved).unwrap();
                assert_eq!(
                    e.render(&e.open(&saved).unwrap(), 0, 2.).unwrap().rgba,
                    actual.rgba
                );
                assert_eq!(fs::read(source).unwrap(), bytes);
            }
        }
    }
}
