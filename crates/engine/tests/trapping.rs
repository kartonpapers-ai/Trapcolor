#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;

fn run(s: &mut Session, command: &str, p: Value) -> Value {
    s.execute(command, &p).unwrap()
}

fn setup() -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 200, "height": 150}));
    for (x, y, size, color) in
        [(0, 0, 100, json!({"model":"cmyk", "c":1, "m":0, "y":0, "k":0})), (30, 30, 40, json!({"model":"cmyk", "c":0, "m":0, "y":1, "k":0}))]
    {
        let id = run(&mut s, "shape.rectangle", json!({"x":x, "y":y, "width":size, "height":size}))["id"].clone();
        run(&mut s, "paint.setFill", json!({"ids":[id], "color":color}));
        run(&mut s, "paint.setStroke", json!({"ids":[id], "none":true}));
    }
    run(&mut s, "select.all", json!({}));
    s
}

#[test]
fn trapping_native_applies_without_installing_a_plugin_and_undo_redo_is_atomic() {
    let mut s = setup();
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    let report = run(&mut s, "trapping.apply", json!({"widthMm":0.2}));
    assert_eq!(report["pairs"].as_array().unwrap().len(), 1);
    assert_eq!(report["pairs"][0]["widthMm"], 0.2);
    let after = s.doc().unwrap().doc.clone();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc().unwrap().doc, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc().unwrap().doc, after);
}

#[test]
fn trapping_native_recalculates_same_pair_and_panel_edits_do_not_rename_art() {
    let mut s = setup();
    let r = run(&mut s, "trapping.apply", json!({}));
    let id = r["pairs"][0]["id"].clone();
    let original_ids = r["ids"].clone();
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":id, "widthMm":0.35, "mode":"center"}}));
    assert_eq!(r["ids"], original_ids);
    assert_eq!(r["pairs"][0]["relation"], "=");
    assert_eq!(r["pairs"][0]["widthMm"], 0.35);
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":id, "widthMm":0.35, "mode":"none"}}));
    assert_eq!(r["pairs"][0]["relation"], "-");
    assert!(matches!(s.doc().unwrap().doc.node(NodeId(id.as_u64().unwrap())).unwrap().appearance.fill_paint(), vectorcraft_color::Paint::None));
    let r = run(&mut s, "trapping.apply", json!({"resetExisting":true, "widthMm":0.15}));
    assert_eq!(r["pairs"][0]["widthMm"], 0.15);
    assert_eq!(r["pairs"][0]["relation"], ">");
    assert_eq!(r["pairs"][0]["id"], id);
}

#[test]
fn trapping_native_invalid_requests_and_no_contact_leave_doc_and_history_unchanged() {
    let mut s = setup();
    for p in [
        json!({"widthMm":-1}),
        json!({"widthMm":3}),
        json!({"difference":101}),
        json!({"direction":"unknown"}),
        json!({"pair":{"id":999, "widthMm":0.1}}),
    ] {
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(s.execute("trapping.apply", &p).is_err(), "{p}");
        assert_eq!(s.doc().unwrap().doc, before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }
    let ids = s.doc().unwrap().selection.objects.clone();
    run(&mut s, "select.set", json!({"ids":[ids[1].0]}));
    run(&mut s, "object.move", json!({"dx":500, "dy":0}));
    run(&mut s, "select.all", json!({}));
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn trapping_native_rejects_transparency_instead_of_silently_flattening_it() {
    let mut s = setup();
    run(&mut s, "transparency.set", json!({"opacity":50}));
    run(&mut s, "select.all", json!({}));
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn trapping_explicit_ids_work_without_changing_selection_first() {
    let mut s = setup();
    let ids: Vec<_> = s.doc().unwrap().selection.objects.iter().map(|id| id.0).collect();
    run(&mut s, "select.none", json!({}));
    let r = run(&mut s, "trapping.apply", json!({"ids":ids}));
    assert_eq!(r["pairs"].as_array().unwrap().len(), 1);
}

#[test]
fn near_black_pairs_keep_the_overprint_choice_after_centering() {
    let mut s = setup();
    let id = s.doc().unwrap().selection.objects[0].0;
    run(&mut s, "paint.setFill", json!({"ids":[id], "color":{"model":"cmyk", "c":0, "m":0, "y":0, "k":0.95}}));
    let r = run(&mut s, "trapping.apply", json!({"difference":0}));
    let pair = r["pairs"][0]["id"].clone();
    assert_eq!(r["pairs"][0]["relation"], "S");
    assert_eq!(r["pairs"][0]["blackLeft"], true);
    run(&mut s, "trapping.apply", json!({"pair":{"id":pair, "widthMm":0.2, "mode":"center"}}));
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":pair, "widthMm":0.2, "mode":"overprint"}}));
    assert_eq!(r["pairs"][0]["relation"], "S");
}

fn spot_fill(s: &mut Session, id: u64, name: &str, tint: f64) {
    run(s, "swatch.new", json!({"name":name, "spot":true, "color":{"model":"lab", "l":55, "a":45, "b":25}}));
    run(s, "paint.setFill", json!({"ids":[id], "swatch":name, "tint":tint}));
}

#[test]
fn spot_process_trap_preserves_link_tint_undo_save_and_pdf_separation() {
    let mut s = setup();
    let id = s.doc().unwrap().selection.objects[0].0;
    let name = "KX Ink 186 C";
    spot_fill(&mut s, id, name, 40.0);
    let analysis = run(&mut s, "trapping.analyze", json!({"difference":0}));
    let contact = &analysis["encounters"][0];
    assert!(contact["left"].as_str().unwrap().contains(name) || contact["right"].as_str().unwrap().contains(name));
    let before = s.doc().unwrap().doc.clone();
    let r = run(&mut s, "trapping.apply", json!({"difference":0}));
    let trap = NodeId(r["pairs"][0]["id"].as_u64().unwrap());
    let doc = &s.doc().unwrap().doc;
    assert_eq!(doc.node(NodeId(id)).unwrap(), before.node(NodeId(id)).unwrap());
    let fills = &doc.node(trap).unwrap().appearance.items;
    assert_eq!(fills.len(), 2);
    assert!(fills.iter().all(|f| f.overprint()));
    assert!(fills.iter().any(|f| matches!(f.paint(), vectorcraft_color::Paint::Solid {swatch:Some(n),tint,..}
        if n == name && (*tint-0.4).abs() < 1e-6)));
    assert!(r["pairs"][0]["left"].as_str().unwrap().contains(name) || r["pairs"][0]["right"].as_str().unwrap().contains(name));
    let native = vectorcraft_format::save(doc, false);
    assert_eq!(vectorcraft_format::load(&native).unwrap(), **doc);
    let settings = serde_json::from_value(json!({"compression":{"compressText":false}})).unwrap();
    let pdf = vectorcraft_pdf::export_with_report(doc, &vectorcraft_pdf::PdfOptions { settings, ..Default::default() }).unwrap();
    let text = String::from_utf8_lossy(&pdf.bytes);
    assert!(text.contains("/Separation"));
    assert!(text.contains("KX#20Ink#20186#20C"));
    assert!(text.contains("/OP true/op true/OPM 1"));
    assert!(pdf.warnings.iter().all(|w| !w.contains("overprint")), "{:?}", pdf.warnings);
    let after = doc.clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc().unwrap().doc, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc().unwrap().doc, after);
}

#[test]
fn two_spots_with_identical_lab_alternates_create_two_real_plates_and_recalculate() {
    let mut s = setup();
    let ids = s.doc().unwrap().selection.objects.clone();
    spot_fill(&mut s, ids[0].0, "Ink / A > 青", 35.0);
    spot_fill(&mut s, ids[1].0, "Ink B", 65.0);
    let r = run(&mut s, "trapping.apply", json!({}));
    let id = r["pairs"][0]["id"].clone();
    run(&mut s, "object.setProps", json!({"ids":[id], "name":"Borde de impresión"}));
    let snapshot = run(&mut s, "trapping.info", json!({}));
    assert_eq!(snapshot["pairs"][0]["name"], "Borde de impresión");
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":id, "mode":"center", "widthMm":0.25}}));
    assert_eq!(r["pairs"][0]["id"], id);
    assert_eq!(r["pairs"][0]["relation"], "=");
    assert_eq!(r["pairs"][0]["name"], "Borde de impresión");
    let n = s.doc().unwrap().doc.node(NodeId(id.as_u64().unwrap())).unwrap();
    assert_eq!(n.appearance.items.len(), 2);
    let names: Vec<_> = n
        .appearance
        .items
        .iter()
        .map(|f| match f.paint() {
            vectorcraft_color::Paint::Solid { swatch: Some(n), .. } => n.as_str(),
            _ => panic!("spot lost its identity"),
        })
        .collect();
    assert!(names.contains(&"Ink / A > 青") && names.contains(&"Ink B"));
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":id,"mode":"none","widthMm":0.25}}));
    assert_eq!(r["pairs"][0]["relation"], "-");
    assert!(s.doc().unwrap().doc.node(NodeId(id.as_u64().unwrap())).unwrap().appearance.items.is_empty());
}

#[test]
fn global_process_swatch_is_preserved_and_registration_is_rejected() {
    let mut s = setup();
    let id = s.doc().unwrap().selection.objects[0].0;
    run(&mut s, "swatch.new", json!({"name":"Global cyan", "global":true, "color":{"model":"cmyk","c":1,"m":0,"y":0,"k":0}}));
    run(&mut s, "paint.setFill", json!({"ids":[id],"swatch":"Global cyan","tint":60}));
    let before = s.doc().unwrap().doc.node(NodeId(id)).unwrap().clone();
    run(&mut s, "trapping.apply", json!({"difference":0}));
    assert_eq!(s.doc().unwrap().doc.node(NodeId(id)).unwrap(), &before);
    run(&mut s, "paint.setFill", json!({"ids":[id],"swatch":"[Registration]"}));
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn reserved_pdf_colorant_names_do_not_become_extra_printing_plates() {
    let mut s = setup();
    let id = s.doc().unwrap().selection.objects[0].0;
    spot_fill(&mut s, id, "All", 100.0);
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn knockout_groups_and_transparent_existing_traps_are_rejected_atomically() {
    let mut s = setup();
    let group = run(&mut s, "object.group", json!({}))["id"].clone();
    run(&mut s, "object.setProps", json!({"ids":[group],"knockout":"on"}));
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
    let mut s = setup();
    let r = run(&mut s, "trapping.apply", json!({}));
    run(&mut s, "object.setProps", json!({"ids":[r["pairs"][0]["id"]],"opacity":50}));
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn copied_traps_keep_their_rules_with_new_object_ids() {
    let mut s = setup();
    let id = s.doc().unwrap().selection.objects[0].0;
    spot_fill(&mut s, id, "Copy ink", 50.0);
    let r = run(&mut s, "trapping.apply", json!({}));
    let old = r["pairs"][0]["id"].clone();
    run(&mut s, "object.move", json!({"dx":130,"dy":0,"copy":true}));
    let r = run(&mut s, "trapping.info", json!({}));
    assert_eq!(r["pairs"].as_array().unwrap().len(), 1);
    let new = r["pairs"][0]["id"].clone();
    assert_ne!(old, new);
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":new,"mode":"center","widthMm":0.3}}));
    assert_eq!(r["pairs"][0]["id"], new);
    assert_eq!(r["pairs"][0]["relation"], "=");
    assert_eq!(r["pairs"][0]["widthMm"], 0.3);
}

#[test]
fn contact_analysis_is_a_query_and_matches_apply() {
    let mut s = setup();
    let doc = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let history = (s.doc().unwrap().history.undo.len(), s.doc().unwrap().history.redo.len());
    let dirty = s.doc().unwrap().is_dirty();
    let report = run(&mut s, "trapping.analyze", json!({"widthMm":0.2}));
    assert_eq!(report["encounters"].as_array().unwrap().len(), 1);
    assert!(report["encounters"][0]["bottomName"].is_string());
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(s.doc().unwrap().is_dirty(), dirty);
    assert_eq!((s.doc().unwrap().history.undo.len(), s.doc().unwrap().history.redo.len()), history);
    let applied = run(&mut s, "trapping.apply", json!({"widthMm":0.2}));
    assert_eq!(report["pairCount"], applied["pairs"].as_array().unwrap().len());
    assert_eq!(report["encounters"][0]["relation"], applied["pairs"][0]["relation"]);
    let saved = run(&mut s, "trapping.analyze", json!({}));
    assert_eq!(saved["encounters"][0]["reason"], "savedRule");
}

#[test]
fn contact_analysis_empty_and_unsupported_are_atomic() {
    let mut s = setup();
    let ids = s.doc().unwrap().selection.objects.clone();
    run(&mut s, "select.set", json!({"ids":[ids[1].0]}));
    run(&mut s, "object.move", json!({"dx":500,"dy":0}));
    run(&mut s, "select.all", json!({}));
    let doc = s.doc().unwrap().doc.clone();
    assert_eq!(run(&mut s, "trapping.analyze", json!({}))["encounters"], json!([]));
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
    run(&mut s, "transparency.set", json!({"opacity":50}));
    let doc = s.doc().unwrap().doc.clone();
    assert!(s.execute("trapping.analyze", &json!({})).is_err());
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
}

fn gradient_case(gradient_color: Value, solid_color: Value, above: bool) -> (Session, NodeId, NodeId) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width":240,"height":140}));
    let mut gradient_id = NodeId(0);
    let mut solid_id = NodeId(0);
    for is_gradient in if above { [false, true] } else { [true, false] } {
        let x = if is_gradient { 0 } else { 80 };
        let id = NodeId(run(&mut s, "shape.rectangle", json!({"x":x,"y":0,"width":100,"height":100}))["id"].as_u64().unwrap());
        if is_gradient {
            gradient_id = id;
            run(
                &mut s,
                "paint.setFill",
                json!({"gradient":{"kind":"linear","start":[0,100],"end":[0,0],"stops":[
                {"offset":0,"color":gradient_color},{"offset":1,"color":{"model":"cmyk","c":0,"m":0,"y":0,"k":0}}]}}),
            );
        } else {
            solid_id = id;
            run(&mut s, "paint.setFill", json!({"color":solid_color}));
        }
        run(&mut s, "paint.setStroke", json!({"none":true}));
    }
    run(&mut s, "select.all", json!({}));
    (s, gradient_id, solid_id)
}

#[test]
fn four_gradient_cases_use_only_solid_ink_and_preserve_sources_undo_save_and_pdf() {
    let cyan = json!({"model":"cmyk","c":1,"m":0,"y":0,"k":0});
    let magenta = json!({"model":"cmyk","c":0,"m":1,"y":0,"k":0});
    let yellow = json!({"model":"cmyk","c":0,"m":0,"y":1,"k":0});
    for (g, solid) in [(cyan.clone(), yellow.clone()), (magenta.clone(), yellow), (cyan.clone(), magenta.clone()), (magenta, cyan)] {
        for above in [false, true] {
            let (mut s, gid, sid) = gradient_case(g.clone(), solid.clone(), above);
            let before = s.doc().unwrap().doc.clone();
            let undo = s.doc().unwrap().history.undo.len();
            let report = run(&mut s, "trapping.analyze", json!({"widthMm":0.2,"direction":"reverse","difference":100}));
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
            assert_eq!(report["encounters"][0]["reason"], "solidIntoGradient");
            assert!(report["encounters"][0]["right"].as_str().unwrap().contains("Degradado"));
            let r = run(&mut s, "trapping.apply", json!({"widthMm":0.2,"direction":"center","difference":100}));
            assert_eq!(r["pairs"].as_array().unwrap().len(), 1);
            assert_eq!(r["pairs"][0]["relation"], ">");
            assert_eq!(r["pairs"][0]["solidIntoGradient"], true);
            let doc = &s.doc().unwrap().doc;
            assert_eq!(doc.node(gid), before.node(gid));
            assert_eq!(doc.node(sid), before.node(sid));
            let trap = doc.node(NodeId(r["pairs"][0]["id"].as_u64().unwrap())).unwrap();
            assert_eq!(trap.appearance.items.len(), 1);
            let fill = &trap.appearance.items[0];
            assert!(fill.overprint());
            let expected: vectorcraft_color::Color = serde_json::from_value(solid.clone()).unwrap();
            assert_eq!(fill.paint(), &vectorcraft_color::Paint::solid(expected));
            let bounds = trap.geometric_bounds().unwrap();
            let edge = if above { 100.0 } else { 80.0 };
            assert!((bounds.x0 - (edge - 0.2 * vectorcraft_trapping::PT_POR_MM)).abs() < 0.002, "{bounds:?}");
            assert!((bounds.x1 - edge).abs() < 0.002, "{bounds:?}");
            assert!((bounds.y0).abs() < 0.002 && (bounds.y1 - 100.0).abs() < 0.002);
            let bytes = vectorcraft_format::save(doc, false);
            assert_eq!(vectorcraft_format::load(&bytes).unwrap(), **doc);
            let settings = serde_json::from_value(json!({"compression":{"compressText":false}})).unwrap();
            let pdf = vectorcraft_pdf::export_with_report(doc, &vectorcraft_pdf::PdfOptions { settings, ..Default::default() }).unwrap();
            let text = String::from_utf8_lossy(&pdf.bytes);
            assert!(text.contains("/ShadingType 2"));
            assert!(text.contains("/OP true/op true/OPM 1"));
            let after = doc.clone();
            assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc().unwrap().doc, before);
            run(&mut s, "edit.redo", json!({}));
            assert_eq!(s.doc().unwrap().doc, after);
        }
    }
}

#[test]
fn gradient_reapply_keeps_identity_width_exclusion_and_fixed_direction() {
    let (mut s, _, _) = gradient_case(json!({"model":"cmyk","c":1,"m":0,"y":0,"k":0}), json!({"model":"cmyk","c":0,"m":0,"y":1,"k":0}), false);
    let first = run(&mut s, "trapping.apply", json!({}));
    let id = first["pairs"][0]["id"].clone();
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":id,"mode":"forward","widthMm":0.3}}));
    assert_eq!(r["ids"], first["ids"]);
    assert_eq!(r["pairs"][0]["widthMm"], 0.3);
    for mode in ["reverse", "center", "overprint"] {
        let doc = s.doc().unwrap().doc.clone();
        assert!(s.execute("trapping.apply", &json!({"pair":{"id":id,"mode":mode}})).is_err());
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
    }
    let r = run(&mut s, "trapping.apply", json!({"pair":{"id":id,"mode":"none","widthMm":0.3}}));
    assert_eq!(r["pairs"][0]["relation"], "-");
    let r = run(&mut s, "trapping.apply", json!({"resetExisting":true,"widthMm":0.15,"direction":"reverse"}));
    assert_eq!(r["pairs"][0]["id"], id);
    assert_eq!(r["pairs"][0]["relation"], ">");
    assert_eq!(r["pairs"][0]["widthMm"], 0.15);
}

#[test]
fn unsupported_gradient_paints_fail_without_editing() {
    use vectorcraft_color::{Paint, gradient::GradientKind};
    for variant in 0..8 {
        let (mut s, gid, _) = gradient_case(json!({"model":"cmyk","c":1,"m":0,"y":0,"k":0}), json!({"model":"cmyk","c":0,"m":1,"y":0,"k":0}), false);
        s.edit("Synthetic unsupported gradient", |d, _| {
            let n = d.node_mut(gid).unwrap();
            let mut p = n.appearance.fill_paint();
            let Paint::Gradient(ref mut g) = p else { panic!() };
            match variant {
                0 => g.gradient.kind = GradientKind::Radial,
                1 => g.gradient.stops[0].opacity = 0.5,
                2 => g.gradient.stops[0].offset = f32::NAN,
                3 => g.gradient.stops[0].color = vectorcraft_color::Color::Cmyk { c: f32::NAN, m: 0.0, y: 0.0, k: 0.0 },
                4 => {
                    let start = g.geom.unwrap().start;
                    g.geom.as_mut().unwrap().end = start;
                }
                5 => g.gradient.stops.clear(),
                6 => g.swatch = Some("Unsupported gradient link".into()),
                _ => g.gradient.kind = GradientKind::Freeform,
            }
            n.appearance.items[0] = vectorcraft_doc::AppearanceItem::Fill(vectorcraft_doc::FillLayer::new(p));
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        for command in ["trapping.analyze", "trapping.apply"] {
            assert!(s.execute(command, &json!({})).is_err(), "variant {variant}");
        }
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }
}

// Private user-owned artwork stays local. CI runs the original synthetic cases above.
#[test]
fn optional_user_gradient_files() {
    for (key, pdf) in [("KX27_GRADIENT_SVG", false), ("KX27_GRADIENT_PDF", true)] {
        let Ok(path) = std::env::var(key) else { continue };
        let bytes = std::fs::read(path).unwrap();
        let doc = if pdf { vectorcraft_pdf::import(&bytes).unwrap() } else { vectorcraft_svg::import(std::str::from_utf8(&bytes).unwrap()).unwrap() };
        let mut ids = Vec::new();
        doc.walk(|n| {
            if matches!(n.kind, vectorcraft_doc::NodeKind::Path { .. } | vectorcraft_doc::NodeKind::Compound { .. })
                && !n.appearance.fill_paint().is_none()
            {
                ids.push(n.id.0);
            }
        });
        let mut s = Session::new();
        s.add_document(doc, None);
        let before = s.doc().unwrap().doc.clone();
        let r = run(&mut s, "trapping.analyze", json!({"ids":ids}));
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(r["encounters"].as_array().unwrap().len(), 4, "{key}: {r}");
        let r = run(&mut s, "trapping.apply", json!({"ids":ids}));
        assert_eq!(r["pairs"].as_array().unwrap().len(), 4);
        for id in ids {
            assert_eq!(s.doc().unwrap().doc.node(NodeId(id)), before.node(NodeId(id)));
        }
        if let Ok(dir) = std::env::var("KX27_GRADIENT_OUTPUT") {
            std::fs::create_dir_all(&dir).unwrap();
            let bytes = vectorcraft_pdf::export_with_report(&s.doc().unwrap().doc, &Default::default()).unwrap().bytes;
            std::fs::write(std::path::Path::new(&dir).join(if pdf { "gradient-pdf-trapped.pdf" } else { "gradient-svg-trapped.pdf" }), bytes)
                .unwrap();
        }
    }
}

fn image_bytes(format: image::ImageFormat, alpha: u8) -> Vec<u8> {
    let pixels = image::RgbaImage::from_fn(4, 4, |x, y| image::Rgba([0, (100 + x * 30) as u8, (150 + y * 20) as u8, alpha]));
    let pixels = if format == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(pixels).to_rgb8())
    } else {
        image::DynamicImage::ImageRgba8(pixels)
    };
    let mut out = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut out, format).unwrap();
    out.into_inner()
}

fn image_case(format: image::ImageFormat, above: bool, alpha: u8) -> (Session, NodeId, NodeId) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width":240,"height":150}));
    let mut image_id = NodeId(0);
    let mut solid_id = NodeId(0);
    for is_image in if above { [false, true] } else { [true, false] } {
        if is_image {
            let bytes = image_bytes(format, alpha);
            let r = run(
                &mut s,
                "file.place",
                json!({"name":if format==image::ImageFormat::Jpeg {"Foto.jpg"} else {"Foto.png"},
                "dataBase64":vectorcraft_format::base64_encode(&bytes),"link":false,"rect":[0,0,100,100]}),
            );
            image_id = NodeId(r["ids"][0].as_u64().unwrap());
        } else {
            solid_id = NodeId(run(&mut s, "shape.rectangle", json!({"x":80,"y":0,"width":100,"height":100}))["id"].as_u64().unwrap());
            run(&mut s, "paint.setFill", json!({"color":{"model":"cmyk","c":0,"m":0,"y":1,"k":0}}));
            run(&mut s, "paint.setStroke", json!({"none":true}));
        }
    }
    run(&mut s, "select.all", json!({}));
    (s, image_id, solid_id)
}

#[test]
fn png_and_jpeg_trapping_preserves_pixels_kind_sources_stack_order_save_undo_and_pdf() {
    use vectorcraft_doc::NodeKind;
    for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg] {
        for above in [false, true] {
            let (mut s, iid, sid) = image_case(format, above, 255);
            let before = s.doc().unwrap().doc.clone();
            let undo = s.doc().unwrap().history.undo.len();
            let report = run(&mut s, "trapping.analyze", json!({"direction":"reverse","difference":100,"widthMm":0.2}));
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
            assert_eq!(s.doc().unwrap().history.undo.len(), undo);
            assert_eq!(report["encounters"][0]["reason"], "solidIntoImage");
            assert!(report["encounters"][0]["right"].as_str().unwrap().contains("Imagen"));
            let r = run(&mut s, "trapping.apply", json!({"direction":"center","difference":100,"widthMm":0.2}));
            assert_eq!(r["pairs"].as_array().unwrap().len(), 1);
            assert_eq!(r["pairs"][0]["solidIntoImage"], true);
            assert_eq!(r["pairs"][0]["relation"], ">");
            let d = &s.doc().unwrap().doc;
            assert_eq!(d.node(iid), before.node(iid));
            assert_eq!(d.node(sid), before.node(sid));
            assert_eq!(d.images, before.images);
            let NodeKind::Image(im) = &d.node(iid).unwrap().kind else { panic!("image converted to path") };
            assert!(std::sync::Arc::ptr_eq(&d.images[&im.key].bytes, &before.images[&im.key].bytes));
            let tid = NodeId(r["pairs"][0]["id"].as_u64().unwrap());
            let trap = d.node(tid).unwrap();
            assert_eq!(trap.appearance.items.len(), 1);
            assert!(trap.appearance.items[0].overprint());
            assert_eq!(
                trap.appearance.items[0].paint(),
                &vectorcraft_color::Paint::solid(vectorcraft_color::Color::Cmyk { c: 0.0, m: 0.0, y: 1.0, k: 0.0 })
            );
            let edge = if above { 100.0 } else { 80.0 };
            let b = trap.geometric_bounds().unwrap();
            assert!((b.x0 - (edge - 0.2 * vectorcraft_trapping::PT_POR_MM)).abs() < 0.002, "{b:?}");
            assert!((b.x1 - edge).abs() < 0.002);
            let mut order = Vec::new();
            d.walk(|n| order.push(n.id));
            assert!(order.iter().position(|id| *id == tid) > order.iter().position(|id| *id == iid));
            assert_eq!(vectorcraft_format::load(&vectorcraft_format::save(d, false)).unwrap(), **d);
            let settings = serde_json::from_value(json!({"compression":{"compressText":false}})).unwrap();
            let pdf = vectorcraft_pdf::export_with_report(d, &vectorcraft_pdf::PdfOptions { settings, ..Default::default() }).unwrap();
            let text = String::from_utf8_lossy(&pdf.bytes);
            assert!(text.contains("/Subtype/Image"));
            assert!(text.contains("/OP true/op true/OPM 1"));
            if let Ok(dir) = std::env::var("KX27_IMAGE_OUTPUT") {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(std::path::Path::new(&dir).join(format!("image-{format:?}-{above}.pdf")), pdf.bytes).unwrap();
            }
            let after = d.clone();
            assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc().unwrap().doc, before);
            run(&mut s, "edit.redo", json!({}));
            assert_eq!(s.doc().unwrap().doc, after);
            let again = run(&mut s, "trapping.apply", json!({"pair":{"id":tid.0,"mode":"forward","widthMm":0.3}}));
            assert_eq!(again["ids"], r["ids"]);
            assert_eq!(again["pairs"][0]["widthMm"], 0.3);
            assert_eq!(s.doc().unwrap().doc.node(iid), before.node(iid));
            for mode in ["reverse", "center", "overprint"] {
                assert!(s.execute("trapping.apply", &json!({"pair":{"id":tid.0,"mode":mode}})).is_err());
            }
        }
    }
}

#[test]
fn transformed_image_uses_its_actual_frame_and_not_its_axis_aligned_bounds() {
    for xf in [
        kurbo::Affine::new([0.0, 25.0, -25.0, 0.0, 100.0, 0.0]),
        kurbo::Affine::new([-25.0, 0.0, 0.0, 25.0, 100.0, 0.0]),
        kurbo::Affine::new([25.0, 0.0, 12.5, 25.0, 0.0, 0.0]),
    ] {
        let (mut s, iid, _) = image_case(image::ImageFormat::Png, true, 255);
        s.edit("Synthetic placement", |d, _| {
            if let vectorcraft_doc::NodeKind::Image(im) = &mut d.node_mut(iid).unwrap().kind {
                im.xf = xf;
            }
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.node(iid).unwrap().clone();
        let r = run(&mut s, "trapping.apply", json!({"widthMm":0.2}));
        let trap = s.doc().unwrap().doc.node(NodeId(r["pairs"][0]["id"].as_u64().unwrap())).unwrap();
        let (path, _, _) = vectorcraft_plugins::objects::geometry(trap).unwrap();
        for (_, _, a) in path.anchors() {
            let p = xf.inverse() * a.p;
            assert!(p.x >= -0.001 && p.x <= 4.001 && p.y >= -0.001 && p.y <= 4.001, "{p:?}");
        }
        assert_eq!(s.doc().unwrap().doc.node(iid), Some(&before));
    }
}

#[test]
fn transparent_missing_linked_invalid_and_budget_exceeding_images_are_atomic() {
    for variant in 0..7 {
        let (mut s, iid, _) = image_case(image::ImageFormat::Png, false, if variant == 0 { 254 } else { 255 });
        if variant > 0 {
            {
                let d = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
                let vectorcraft_doc::NodeKind::Image(im) = &mut d.node_mut(iid).unwrap().kind else { panic!() };
                let key = im.key.clone();
                match variant {
                    1 => im.link = Some(vectorcraft_doc::LinkInfo::new("missing.png")),
                    2 => im.xf = kurbo::Affine::scale(0.0),
                    3 => im.width = 5,
                    4 => {
                        d.images.remove(&key);
                    }
                    5 => {
                        d.images.get_mut(&key).unwrap().bytes = std::sync::Arc::new(vec![1, 2, 3]);
                    }
                    _ => im.xf = kurbo::Affine::translate((f64::INFINITY, 0.0)),
                }
            }
        }
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        for c in ["trapping.analyze", "trapping.apply"] {
            assert!(s.execute(c, &json!({})).is_err(), "variant {variant}");
        }
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }
    // Alpha 65534 must not be rounded up to 255 by an 8-bit opacity check.
    let (mut s, iid, _) = image_case(image::ImageFormat::Png, false, 255);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(4, 4, image::Rgba([0, 0, 0, 65534u16])))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    s.edit("Synthetic 16-bit alpha", |d, _| {
        let vectorcraft_doc::NodeKind::Image(im) = &d.node(iid).unwrap().kind else { panic!() };
        let key = im.key.clone();
        d.images.insert(key, vectorcraft_doc::ImageBlob::png(bytes.into_inner()));
        Ok(())
    })
    .unwrap();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
}

#[test]
fn image_references_are_read_only_in_the_vector_adapter_and_trap_names_do_not_repaint_images() {
    use vectorcraft_plugins::objects::{self, OutObject};
    let (mut s, iid, _) = image_case(image::ImageFormat::Png, false, 255);
    let mut d = (*s.doc().unwrap().doc).clone();
    let before = d.clone();
    assert!(objects::filter_targets(&d, &[iid]).is_empty());
    objects::apply_output(&mut d, &[iid], vec![OutObject { id: Some(iid), ..Default::default() }], None).unwrap();
    assert_eq!(d, before);
    assert!(
        objects::apply_output(&mut d, &[iid], vec![OutObject { id: Some(iid), name: Some("changed".into()), ..Default::default() }], None).is_err()
    );
    assert_eq!(d, before);
    run(&mut s, "object.setProps", json!({"ids":[iid.0],"name":format!("trap Y>IM{} 0.1",iid.0)}));
    run(&mut s, "select.all", json!({}));
    let original = s.doc().unwrap().doc.node(iid).unwrap().clone();
    run(&mut s, "trapping.apply", json!({}));
    assert_eq!(s.doc().unwrap().doc.node(iid), Some(&original));
}

#[test]
fn image_frame_without_contact_is_a_successful_empty_analysis() {
    let (mut s, iid, _) = image_case(image::ImageFormat::Png, false, 255);
    run(&mut s, "select.set", json!({"ids":[iid.0]}));
    run(&mut s, "object.move", json!({"dx":400,"dy":0}));
    run(&mut s, "select.all", json!({}));
    let before = s.doc().unwrap().doc.clone();
    assert_eq!(run(&mut s, "trapping.analyze", json!({}))["encounters"], json!([]));
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}

#[test]
fn huge_raster_headers_and_image_image_or_gradient_contacts_are_rejected() {
    let (mut s, iid, _) = image_case(image::ImageFormat::Png, false, 255);
    let mut bytes = image_bytes(image::ImageFormat::Png, 255);
    bytes[16..20].copy_from_slice(&10000u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&10000u32.to_be_bytes());
    let crc = crc32fast::hash(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    s.edit("Synthetic large header", |d, _| {
        let vectorcraft_doc::NodeKind::Image(im) = &mut d.node_mut(iid).unwrap().kind else { panic!() };
        im.width = 10000;
        im.height = 10000;
        let key = im.key.clone();
        d.images.insert(key, vectorcraft_doc::ImageBlob::png(bytes));
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    let error = s.execute("trapping.apply", &json!({})).unwrap_err().to_string();
    assert!(error.contains("32 megapíxeles"), "{error}");
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    for gradient in [false, true] {
        let (mut s, _, sid) = image_case(image::ImageFormat::Png, false, 255);
        if gradient {
            run(
                &mut s,
                "paint.setFill",
                json!({"ids":[sid.0],"gradient":{"kind":"linear","stops":[{"offset":0,"color":"#ffffff"},{"offset":1,"color":"#ff0000"}]}}),
            );
        } else {
            run(
                &mut s,
                "file.place",
                json!({"name":"second.png","dataBase64":vectorcraft_format::base64_encode(&image_bytes(image::ImageFormat::Png,255)),"link":false,"rect":[80,0,100,100]}),
            );
        }
        run(&mut s, "select.all", json!({}));
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("trapping.analyze", &json!({})).is_err());
        assert!(s.execute("trapping.apply", &json!({})).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
}
