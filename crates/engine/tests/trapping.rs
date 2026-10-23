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
