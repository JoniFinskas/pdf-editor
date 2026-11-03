use super::*;

#[test]
fn text_font_resize_preserves_shape_spacing_neighbors_and_single_undo() {
    let _lock=PDFIUM.lock().unwrap_or_else(|e|e.into_inner());
    let e=engine();
    for rotation in [0,90,180,270] {
        for clipped in [false,true] {
            for factor in [0.7,2.4] {
                let mut pdf=readable_drag_fixture(rotation);
                let page=pdf.get_pages()[&1];
                let clip=if clipped {"40 180 250 75 re W n"} else {""};
                let data=format!("q 1.1 0.1 -0.05 0.9 12 20 cm q {clip} BT /F1 16 Tf 1.3 Tc 2.4 Tw 90 Tz 0.4 Ts 0.2 0.4 0.6 rg 0.7 0.2 0.1 RG 1 0 0 1 60 210 Tm (Hello) Tj [( ) -30 (world)] TJ ( again) Tj ET Q Q q BT /F1 18 Tf 1.7 Tc 3.1 Tw 1 0 0 1 320 60 Tm (Neighbor) Tj ET Q");
                pdf.change_page_content(page,data.into_bytes()).unwrap();
                let source=temp(&format!("font-resize-{rotation}-{clipped}-{factor}.pdf"));
                let mut bytes=vec![];pdf.save_to(&mut bytes).unwrap();fs::write(&source,&bytes).unwrap();
                let mut doc=e.open(&source).unwrap();
                let before=e.text_objects(&doc,0).unwrap();
                let items=e.page_items(&doc,0).unwrap().into_iter().filter(|i|before.iter().any(|o|o.index==i.index && o.text!="Neighbor")).collect::<Vec<_>>();
                assert_eq!(items.len(),3);
                let old=items.iter().skip(1).fold(items[0].bounds,|b,i|Bounds {left:b.left.min(i.bounds.left),right:b.right.max(i.bounds.right),bottom:b.bottom.min(i.bounds.bottom),top:b.top.max(i.bounds.top)});
                let target=Bounds {right:old.left+(old.right-old.left)*factor,top:old.bottom+(old.top-old.bottom)*factor,..old};
                let original=doc.bytes.clone();
                let original_image=e.render(&doc,0,1.5).unwrap();
                let history=doc.recovery_bytes().unwrap();
                let invalid=Bounds {right:target.right+20.,..target};
                assert!(e.edit_page_items(&mut doc,&items,ObjectAction::ResizeText {bounds:invalid}).is_err());
                assert!(doc.bytes==original && doc.recovery_bytes().unwrap()==history);
                e.edit_page_items(&mut doc,&items,ObjectAction::ResizeText {bounds:target}).unwrap_or_else(|error|panic!("rotation={rotation}, clip={clipped}, factor={factor}: {error}"));
                let after=e.text_objects(&doc,0).unwrap();
                assert_eq!(before.len(),after.len());
                for (a,b) in before.iter().zip(&after) {
                    assert_eq!(a.text,b.text);assert_eq!(a.font,b.font);assert_eq!(a.color,b.color);
                    assert!(a.matrix[..4].iter().zip(&b.matrix[..4]).all(|(x,y)|(x-y).abs()<0.001));
                    let selected=items.iter().any(|i|i.index==a.index);
                    assert!((b.size-a.size*if selected {factor}else{1.}).abs()<0.001);
                    if selected {
                        assert!((b.matrix[4]-(old.left+(a.matrix[4]-old.left)*factor)).abs()<0.05);
                        assert!((b.matrix[5]-(old.bottom+(a.matrix[5]-old.bottom)*factor)).abs()<0.05);
                    } else {assert_eq!(a.matrix,b.matrix);assert_eq!(a.bounds,b.bounds);}
                }
                let resized=e.page_items(&doc,0).unwrap();
                let actual=items.iter().map(|i|resized.iter().find(|new|new.index==i.index).unwrap().bounds).reduce(|a,b|Bounds {left:a.left.min(b.left),right:a.right.max(b.right),bottom:a.bottom.min(b.bottom),top:a.top.max(b.top)}).unwrap();
                assert!((actual.left-target.left).abs()<0.15 && (actual.right-target.right).abs()<0.15 && (actual.bottom-target.bottom).abs()<0.15 && (actual.top-target.top).abs()<0.15,"rotation={rotation}, clip={clipped}, factor={factor}: {actual:?} != {target:?}");
                let rendered=e.render(&doc,0,1.5).unwrap();
                let mut reference=e.open(&source).unwrap();
                e.edit_page_items(&mut reference,&items,ObjectAction::Resize {bounds:target}).unwrap();
                let expected_image=e.render(&reference,0,1.5).unwrap();
                let region=Bounds {left:old.left.min(actual.left),right:old.right.max(actual.right),bottom:old.bottom.min(actual.bottom),top:old.top.max(actual.top)};
                let corners=[(region.left,region.bottom),(region.right,region.bottom),(region.right,region.top),(region.left,region.top)].map(|(x,y)|e.page_to_pixel(&doc,0,1.5,x,y).unwrap());
                let min_x=corners.iter().map(|p|p.0).min().unwrap()-4;let max_x=corners.iter().map(|p|p.0).max().unwrap()+4;
                let min_y=corners.iter().map(|p|p.1).min().unwrap()-4;let max_y=corners.iter().map(|p|p.1).max().unwrap()+4;
                let mut actual_ink=0usize;let mut expected_ink=0usize;
                for y in 0..rendered.height as i32 {for x in 0..rendered.width as i32 {
                    let i=(y as usize*rendered.width as usize+x as usize)*4;
                    if x<min_x || x>max_x || y<min_y || y>max_y {
                        assert_eq!(&original_image.rgba[i..i+4],&rendered.rgba[i..i+4],"Outside font resize: rotation={rotation}, clip={clipped}, factor={factor}, pixel={x},{y}");
                    } else {
                        actual_ink+=usize::from(rendered.rgba[i..i+3].iter().map(|v|u32::from(*v)).sum::<u32>()<600);
                        expected_ink+=usize::from(expected_image.rgba[i..i+3].iter().map(|v|u32::from(*v)).sum::<u32>()<600);
                    }
                }}
                let ink_error=actual_ink.abs_diff(expected_ink) as f32/expected_ink.max(1) as f32;
                assert!(expected_ink>0 && ink_error<0.02,"Font preview visibility differs: {actual_ink} vs {expected_ink}, clip={clipped}, factor={factor}");
                let saved=temp(&format!("font-resize-saved-{rotation}-{clipped}-{factor}.pdf"));
                if saved.exists(){fs::remove_file(&saved).unwrap();}
                doc.save_as(&saved).unwrap();
                assert_eq!(e.render(&e.open(&saved).unwrap(),0,1.5).unwrap().rgba,rendered.rgba);
                let changed=doc.bytes.clone();assert!(doc.undo());assert!(doc.bytes==original);assert!(!doc.can_undo());assert!(doc.redo());assert!(doc.bytes==changed);
                assert_eq!(fs::read(source).unwrap(),bytes);fs::remove_file(saved).unwrap();
            }
        }
    }
}

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
