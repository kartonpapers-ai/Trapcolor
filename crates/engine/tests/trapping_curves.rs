#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use serde_json::{Value, json};
use std::sync::Arc;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Node, NodeId};
use vectorcraft_engine::Session;
use vectorcraft_geom::{Point, Rect, shapes};

fn setup() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width":200,"height":100})).unwrap();
    s
}
fn rect(s: &mut Session, r: Rect, c: Color) -> NodeId {
    s.edit("Fixture", |d, _| {
        let id = d.alloc_id();
        let layer = d.default_layer();
        d.insert(layer, usize::MAX, Node::path(id, shapes::rectangle(r), Appearance::basic(Paint::solid(c), Paint::None, 0.0)))?;
        Ok(id)
    })
    .unwrap()
}
fn verify(s: &mut Session, a: NodeId, b: NodeId, region: Value) -> Value {
    let doc = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.objects.clone();
    let undo = s.doc().unwrap().history.undo.len();
    let redo = s.doc().unwrap().history.redo.len();
    let r = s.execute("trapping.verifyCurves", &json!({"ids":[a.0,b.0],"region":region})).unwrap();
    assert!(Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
    assert_eq!(selection, s.doc().unwrap().selection.objects);
    assert_eq!(undo, s.doc().unwrap().history.undo.len());
    assert_eq!(redo, s.doc().unwrap().history.redo.len());
    r
}
fn pair(s: &mut Session, gap: f64) -> (NodeId, NodeId) {
    let a = rect(s, Rect::new(0.0, 0.0, 100.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = rect(s, Rect::new(100.0 + gap, 0.0, 200.0, 100.0), Color::cmyk(0.0, 1.0, 0.0, 0.0));
    (a, b)
}
#[test]
fn shared_and_overlapping_edges_are_checked_on_original_curves_without_edits() {
    for gap in [0.0, -20.0] {
        let mut s = setup();
        let (a, b) = pair(&mut s, gap);
        let x = 100.0 + gap;
        let r = verify(&mut s, a, b, json!([x - 1.0, 10, x + 1.0, 90]));
        assert_eq!(r["status"], "visibleBoundaryFound");
        assert!(r["visibleSamples"].as_u64().unwrap() > 100);
        for fragment in r["fragments"].as_array().unwrap() {
            assert!((fragment[2].as_f64().unwrap() - x).abs() < 1e-9);
        }
        assert_eq!(r["probeMm"], 0.0001);
    }
}
#[test]
fn resolved_micro_gap_does_not_turn_into_a_contact() {
    let mut s = setup();
    let (a, b) = pair(&mut s, 0.01);
    let r = verify(&mut s, a, b, json!([99, 10, 101, 90]));
    assert_eq!(r["visibleSamples"], 0);
    assert!(r["gapSamples"].as_u64().unwrap() > 0);
    assert_eq!(r["status"], "noVerifiedBoundary");
}
#[test]
fn original_cubic_samples_follow_the_curve_instead_of_the_candidates_pixel_box() {
    let mut s = setup();
    let a = rect(&mut s, Rect::new(0.0, 0.0, 200.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let path = shapes::ellipse(Rect::new(60.0, 20.0, 140.0, 80.0));
    let bez = path.to_bezpath();
    let b = s
        .edit("Cubic ellipse", |d, _| {
            let id = d.alloc_id();
            let layer = d.default_layer();
            d.insert(layer, usize::MAX, Node::path(id, path, Appearance::basic(Paint::solid(Color::cmyk(0.0, 1.0, 0.0, 0.0)), Paint::None, 0.0)))?;
            Ok(id)
        })
        .unwrap();
    let r = verify(&mut s, a, b, json!([59, 19, 100, 81]));
    assert!(r["visibleSamples"].as_u64().unwrap() > 50);
    for f in r["fragments"].as_array().unwrap() {
        let p = Point::new(f[2].as_f64().unwrap(), f[3].as_f64().unwrap());
        assert!(vectorcraft_geom::hit::distance_to_outline(&bez, p) < 1e-6);
    }
}
#[test]
fn occlusion_and_unknown_compositing_are_reported_without_seeing_through_art() {
    for opacity in [1.0, 0.5] {
        let mut s = setup();
        let (a, b) = pair(&mut s, 0.0);
        let c = rect(&mut s, Rect::new(80.0, 0.0, 120.0, 100.0), Color::cmyk(0.0, 0.0, 1.0, 0.0));
        s.edit("Opacity", |d, _| {
            d.node_mut(c).unwrap().opacity = opacity;
            Ok(())
        })
        .unwrap();
        let r = verify(&mut s, a, b, json!([99, 10, 101, 90]));
        assert_eq!(r["visibleSamples"], 0);
        if opacity == 1.0 {
            assert!(r["occludedSamples"].as_u64().unwrap() > 0);
        } else {
            assert!(r["unresolvedSamples"].as_u64().unwrap() > 0);
            assert_eq!(r["status"], "incomplete");
        }
    }
}
#[test]
fn clip_edges_and_holes_use_original_fill_rules() {
    for svg in [
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><clipPath id="c"><rect x="100" width="100" height="100"/></clipPath></defs><rect width="200" height="100" fill="#00ffff"/><g clip-path="url(#c)"><rect width="200" height="100" fill="#ff00ff"/></g></svg>"##,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="200" height="100" fill="#ff00ff"/><path fill="#00ffff" fill-rule="evenodd" d="M0 0H200V100H0Z M100 10H150V90H100Z"/></svg>"##,
    ] {
        let mut s = setup();
        s.execute("document.open", &json!({"name":"original.svg","dataBase64":vectorcraft_format::base64_encode(svg.as_bytes())})).unwrap();
        let report = s.execute("trapping.boundaries", &json!({})).unwrap();
        let ids = &report["zones"][0]["attribution"]["pairs"][0]["ids"];
        let r = verify(&mut s, NodeId(ids[0].as_u64().unwrap()), NodeId(ids[1].as_u64().unwrap()), json!([99, 20, 101, 80]));
        assert!(r["visibleSamples"].as_u64().unwrap() > 50, "{r}");
    }
}
#[test]
fn spot_pairs_large_documents_and_bounded_incomplete_results_are_supported() {
    let mut s = setup();
    for _ in 0..100 {
        rect(&mut s, Rect::new(-10.0, -10.0, -5.0, -5.0), Color::WHITE);
    }
    let (a, b) = pair(&mut s, 0.0);
    for (id, name) in [(a, "Curve spot A"), (b, "Curve spot B")] {
        s.execute("swatch.new", &json!({"name":name,"spot":true,"color":{"model":"cmyk","c":1,"m":0,"y":0,"k":0}})).unwrap();
        s.execute("paint.setFill", &json!({"ids":[id.0],"swatch":name})).unwrap();
    }
    let r = verify(&mut s, a, b, json!([99, 10, 101, 90]));
    assert_eq!(r["localObjects"], 2);
    assert!(r["visibleSamples"].as_u64().unwrap() > 0);
    let mut s = setup();
    let a = rect(&mut s, Rect::new(0.0, 0.0, 100.0, 5000.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = rect(&mut s, Rect::new(100.0, 0.0, 200.0, 5000.0), Color::cmyk(0.0, 1.0, 0.0, 0.0));
    let r = verify(&mut s, a, b, json!([99, 0, 101, 5000]));
    assert_eq!(r["truncated"], true);
    assert_eq!(r["status"], "incomplete");
    assert!(r["testedSamples"].as_u64().unwrap() <= 8192);
}
#[test]
fn invalid_or_unsupported_requests_fail_without_document_edits() {
    let mut s = setup();
    let (a, b) = pair(&mut s, 0.0);
    let before = s.doc().unwrap().doc.clone();
    for p in [
        json!({}),
        json!({"ids":[a.0,b.0]}),
        json!({"ids":[a.0,a.0],"region":[99,10,101,90]}),
        json!({"ids":[a.0,b.0],"region":[99,10,99,90]}),
        json!({"ids":[-1,b.0],"region":[99,10,101,90]}),
        json!({"ids":[a.0,999999],"region":[99,10,101,90]}),
    ] {
        assert!(s.execute("trapping.verifyCurves", &p).is_err(), "{p}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
    s.edit("Unsupported source", |d, _| {
        d.node_mut(b).unwrap().opacity = 0.5;
        Ok(())
    })
    .unwrap();
    assert!(s.execute("trapping.verifyCurves", &json!({"ids":[a.0,b.0],"region":[99,10,101,90]})).is_err());
}

#[test]
fn implicit_fill_closure_and_dense_scene_limits_are_respected() {
    let mut s = setup();
    let a = rect(&mut s, Rect::new(0.0, 0.0, 200.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = s
        .edit("Open filled path", |d, _| {
            let id = d.alloc_id();
            let layer = d.default_layer();
            let path = vectorcraft_geom::PathData::single(vectorcraft_geom::SubPath::polyline(
                &[Point::new(100.0, 0.0), Point::new(200.0, 0.0), Point::new(200.0, 100.0)],
                false,
            ));
            d.insert(layer, usize::MAX, Node::path(id, path, Appearance::basic(Paint::solid(Color::cmyk(0.0, 1.0, 0.0, 0.0)), Paint::None, 0.0)))?;
            Ok(id)
        })
        .unwrap();
    assert!(verify(&mut s, a, b, json!([139, 39, 141, 41]))["visibleSamples"].as_u64().unwrap() > 0);
    s.edit("Dense local scene", |d, _| {
        let layer = d.default_layer();
        for _ in 0..520 {
            let id = d.alloc_id();
            d.insert(
                layer,
                usize::MAX,
                Node::path(
                    id,
                    shapes::rectangle(Rect::new(139.0, 39.0, 141.0, 41.0)),
                    Appearance::basic(Paint::solid(Color::WHITE), Paint::None, 0.0),
                ),
            )?;
        }
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    let e = s.execute("trapping.verifyCurves", &json!({"ids":[a.0,b.0],"region":[139,39,141,41]})).unwrap_err().to_string();
    assert!(e.contains("512"));
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}
