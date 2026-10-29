#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use serde_json::{Value, json};
use std::sync::Arc;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Node, NodeId};
use vectorcraft_engine::Session;
use vectorcraft_geom::{Rect, shapes};
static CHECKS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn rect(s: &mut Session, r: Rect, c: Color) -> NodeId {
    s.edit("Fixture", |d, _| {
        let id = d.alloc_id();
        let layer = d.layers.first().map(|n| n.id);
        d.insert(layer, usize::MAX, Node::path(id, shapes::rectangle(r), Appearance::basic(Paint::solid(c), Paint::None, 0.0)))?;
        Ok(id)
    })
    .unwrap()
}
fn setup() -> (Session, [NodeId; 2]) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width":200,"height":100})).unwrap();
    let a = rect(&mut s, Rect::new(0.0, 0.0, 100.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = rect(&mut s, Rect::new(100.0, 0.0, 200.0, 100.0), Color::cmyk(0.0, 1.0, 0.0, 0.0));
    s.execute("select.set", &json!({"ids":[a,b]})).unwrap();
    (s, [a, b])
}
fn preview(s: &mut Session, ids: [NodeId; 2]) -> Value {
    s.execute("trapping.previewLocal", &json!({"ids":ids,"region":[99,10,101,90],"widthMm":0.1})).unwrap()
}
fn incorporate(s: &mut Session, p: &Value) -> Value {
    s.execute("trapping.incorporateLocal", &json!({"previewId":p["previewId"]})).unwrap()
}
fn reject(s: &mut Session, p: &Value) {
    let doc = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.objects.clone();
    let undo = s.doc().unwrap().history.undo.len();
    let redo = s.doc().unwrap().history.redo.len();
    assert!(s.execute("trapping.incorporateLocal", p).is_err());
    assert!(Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
    assert_eq!(selection, s.doc().unwrap().selection.objects);
    assert_eq!(undo, s.doc().unwrap().history.undo.len());
    assert_eq!(redo, s.doc().unwrap().history.redo.len());
}
#[test]
fn exact_preview_becomes_one_overprint_path_in_top_layer_and_one_undo_step() {
    let _serial = CHECKS.lock().unwrap();
    for reverse in [false, true] {
        let (mut s, mut ids) = setup();
        if reverse {
            ids.reverse();
        }
        let p = preview(&mut s, ids);
        let before = s.doc().unwrap().doc.clone();
        let selected = s.doc().unwrap().selection.objects.clone();
        let undo = s.doc().unwrap().history.undo.len();
        let r = incorporate(&mut s, &p);
        let id = NodeId(r["id"].as_u64().unwrap());
        let layer = NodeId(r["layerId"].as_u64().unwrap());
        let d = &s.doc().unwrap().doc;
        let n = d.node(id).unwrap();
        assert_eq!(n.path_data().unwrap().to_bezpath().to_svg(), p["bandPath"].as_str().unwrap());
        assert_eq!(n.appearance.fill_paint(), before.node(ids[0]).unwrap().appearance.fill_paint());
        assert_eq!(n.appearance.items.len(), 1);
        assert!(n.appearance.items[0].overprint());
        assert_eq!(d.parent_of(id), Some(layer));
        assert_eq!(d.layers.last().unwrap().id, layer);
        assert_eq!(d.layers.len(), before.layers.len() + 1);
        for (old, current) in before.layers.iter().zip(d.layers.iter()) {
            assert!(Arc::ptr_eq(old, current));
        }
        for source in ids {
            assert_eq!(before.node(source), d.node(source));
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        assert_eq!(s.doc().unwrap().selection.objects, vec![id]);
        let info = s.execute("trapping.info", &json!({})).unwrap();
        assert_eq!(info["artwork"], 0);
        assert_eq!(info["localTraps"][0]["id"], r["id"]);
        let after = s.doc().unwrap().doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        assert_eq!(s.doc().unwrap().selection.objects, selected);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, after);
        reject(&mut s, &json!({"previewId":p["previewId"]}));
    }
}
#[test]
fn linked_spot_tint_geometry_and_overprint_survive_native_reopen_and_pdf() {
    let _serial = CHECKS.lock().unwrap();
    let (mut s, ids) = setup();
    for (id, name, tint, c) in [(ids[0], "KX Test Spot A", 40, 1), (ids[1], "KX Test Spot B", 75, 0)] {
        s.execute("swatch.new", &json!({"name":name,"spot":true,"color":{"model":"cmyk","c":c,"m":1-c,"y":0,"k":0}})).unwrap();
        s.execute("paint.setFill", &json!({"ids":[id],"swatch":name,"tint":tint})).unwrap();
    }
    let p = preview(&mut s, ids);
    let r = incorporate(&mut s, &p);
    let id = NodeId(r["id"].as_u64().unwrap());
    let doc = s.doc().unwrap().doc.clone();
    let bytes = vectorcraft_format::save(&doc, false);
    let reopened = vectorcraft_format::load(&bytes).unwrap();
    assert_eq!(reopened, *doc);
    let trap = reopened.node(id).unwrap();
    assert!(matches!(trap.appearance.fill_paint(),Paint::Solid{swatch:Some(ref name),tint,..} if name=="KX Test Spot A" && (tint-0.4).abs()<1e-6));
    assert!(trap.appearance.items[0].overprint());
    let settings = serde_json::from_value(json!({"compression":{"compressText":false}})).unwrap();
    let pdf = vectorcraft_pdf::export_with_report(&reopened, &vectorcraft_pdf::PdfOptions { settings, ..Default::default() }).unwrap();
    let text = String::from_utf8_lossy(&pdf.bytes);
    assert!(text.contains("/Separation"));
    assert!(text.contains("KX#20Test#20Spot#20A"));
    assert!(text.contains("KX#20Test#20Spot#20B"));
    assert!(text.contains("/OP true/op true/OPM 1"));
    assert!(pdf.warnings.iter().all(|w| !w.contains("overprint")));
    let mut second = Session::new();
    second.execute("document.open", &json!({"name":"reopened.vectorcraft","dataBase64":vectorcraft_format::base64_encode(&bytes)})).unwrap();
    second.execute("select.set", &json!({"ids":[id]})).unwrap();
    let info = second.execute("trapping.info", &json!({})).unwrap();
    assert_eq!(info["localTraps"][0]["record"]["source"], json!(ids[0]));
    assert_eq!(info["localTraps"][0]["record"]["widthMm"], 0.1);
    reject(&mut second, &json!({"previewId":p["previewId"]}));
}
#[test]
fn stale_unknown_replaced_and_failed_preview_tickets_cannot_modify_documents() {
    let _serial = CHECKS.lock().unwrap();
    let (mut s, ids) = setup();
    reject(&mut s, &json!({}));
    reject(&mut s, &json!({"previewId":1}));
    let p = preview(&mut s, ids);
    let next = preview(&mut s, ids);
    assert_ne!(p["previewId"], next["previewId"]);
    reject(&mut s, &json!({"previewId":p["previewId"]}));
    reject(&mut s, &json!({"previewId":next["previewId"],"bandPath":"M0 0H200V100Z"}));
    s.execute("shape.rectangle", &json!({"x":5,"y":5,"width":2,"height":2})).unwrap();
    reject(&mut s, &json!({"previewId":next["previewId"]}));
    let p = preview(&mut s, ids);
    assert!(s.execute("trapping.previewLocal", &json!({"ids":ids,"region":[99,10,99,90]})).is_err());
    reject(&mut s, &json!({"previewId":p["previewId"]}));
    let p = preview(&mut s, ids);
    s.execute("file.new", &json!({})).unwrap();
    reject(&mut s, &json!({"previewId":p["previewId"]}));
}
#[test]
fn selection_changes_are_allowed_but_color_settings_and_edit_groups_are_guarded() {
    let _serial = CHECKS.lock().unwrap();
    let (mut s, ids) = setup();
    let p = preview(&mut s, ids);
    s.execute("select.none", &json!({})).unwrap();
    incorporate(&mut s, &p);
    let (mut s, ids) = setup();
    let p = preview(&mut s, ids);
    s.execute("edit.undoGroup.begin", &json!({})).unwrap();
    reject(&mut s, &json!({"previewId":p["previewId"]}));
    s.execute("edit.undoGroup.end", &json!({})).unwrap();
    incorporate(&mut s, &p);
    let (mut s, ids) = setup();
    let p = preview(&mut s, ids);
    let settings = vectorcraft_color::cms::active_settings();
    let mut changed = settings.clone();
    changed.bpc = !settings.bpc;
    vectorcraft_color::cms::set_active(&changed).unwrap();
    reject(&mut s, &json!({"previewId":p["previewId"]}));
    vectorcraft_color::cms::set_active(&settings).unwrap();
}
#[test]
fn managed_layer_is_reused_for_other_pairs_and_overlap_guard_survives_save() {
    let _serial = CHECKS.lock().unwrap();
    let (mut s, ids) = setup();
    let c = rect(&mut s, Rect::new(0.0, 120.0, 100.0, 200.0), Color::cmyk(0.0, 0.0, 1.0, 0.0));
    let d = rect(&mut s, Rect::new(100.0, 120.0, 200.0, 200.0), Color::cmyk(0.5, 0.0, 0.0, 0.0));
    let p = preview(&mut s, ids);
    let first = incorporate(&mut s, &p);
    let p = s.execute("trapping.previewLocal", &json!({"ids":[c,d],"region":[99,130,101,190]})).unwrap();
    let second = incorporate(&mut s, &p);
    assert_eq!(first["layerId"], second["layerId"]);
    let bytes = vectorcraft_format::save(&s.doc().unwrap().doc, false);
    let mut reopened = Session::new();
    reopened.execute("document.open", &json!({"name":"local.vectorcraft","dataBase64":vectorcraft_format::base64_encode(&bytes)})).unwrap();
    // Hidden traps must still count for duplicate prevention after reopen; hiding the
    // managed layer permits a new underlying-contact preview, but not a duplicate commit.
    let layer = NodeId(first["layerId"].as_u64().unwrap());
    reopened
        .edit("Hide", |doc, _| {
            doc.node_mut(layer).unwrap().visible = false;
            Ok(())
        })
        .unwrap();
    let p = preview(&mut reopened, ids);
    reject(&mut reopened, &json!({"previewId":p["previewId"]}));
}
#[test]
fn unsuitable_managed_layer_and_legacy_recalculation_fail_without_edits() {
    let _serial = CHECKS.lock().unwrap();
    for locked in [true, false] {
        let (mut s, ids) = setup();
        let p = preview(&mut s, ids);
        let first = incorporate(&mut s, &p);
        let trap = NodeId(first["id"].as_u64().unwrap());
        let layer = NodeId(first["layerId"].as_u64().unwrap());
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("trapping.apply", &json!({"ids":[ids[0],ids[1],trap]})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        let c = rect(&mut s, Rect::new(0.0, 120.0, 100.0, 200.0), Color::cmyk(0.0, 0.0, 1.0, 0.0));
        let d = rect(&mut s, Rect::new(100.0, 120.0, 200.0, 200.0), Color::cmyk(0.5, 0.0, 0.0, 0.0));
        s.edit("Layer options", |doc, _| {
            if locked {
                doc.node_mut(layer).unwrap().locked = true;
            } else {
                doc.node_mut(layer).unwrap().opacity = 0.5;
            }
            Ok(())
        })
        .unwrap();
        let p = s.execute("trapping.previewLocal", &json!({"ids":[c,d],"region":[99,130,101,190]})).unwrap();
        reject(&mut s, &json!({"previewId":p["previewId"]}));
    }
}

#[test]
fn shared_ink_reductions_and_non_print_sources_are_rejected_without_changing_art() {
    let _serial = CHECKS.lock().unwrap();
    for (source, receiver) in [
        (Color::cmyk(0.2, 1.0, 0.0, 0.0), Color::cmyk(0.8, 0.0, 0.0, 0.0)),
        (Color::rgb(0.0, 1.0, 1.0), Color::cmyk(0.0, 1.0, 0.0, 0.0)),
        (Color::cmyk(0.0, 0.0, 0.0, 0.0), Color::cmyk(0.0, 1.0, 0.0, 0.0)),
    ] {
        let (mut s, ids) = setup();
        s.edit("Print source", |doc, _| {
            for (id, c) in [(ids[0], source), (ids[1], receiver)] {
                doc.node_mut(id).unwrap().appearance = Appearance::basic(Paint::solid(c), Paint::None, 0.0).into();
            }
            Ok(())
        })
        .unwrap();
        let p = preview(&mut s, ids);
        reject(&mut s, &json!({"previewId":p["previewId"]}));
    }
    // A source whose non-zero shared plates do not reduce the receiver is safe.
    let (mut s, ids) = setup();
    s.execute("paint.setFill", &json!({"ids":[ids[1]],"color":{"model":"cmyk","c":0.5,"m":1,"y":0,"k":0}})).unwrap();
    let p = preview(&mut s, ids);
    incorporate(&mut s, &p);
}
