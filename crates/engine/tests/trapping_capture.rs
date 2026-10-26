#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::sync::Arc;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Document, ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_engine::Session;
use vectorcraft_geom::{Affine, Rect, shapes};

fn setup() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width":200,"height":100})).unwrap();
    s
}

fn rect(s: &mut Session, r: Rect, color: Color) -> NodeId {
    s.edit("Fixture", |d, _| {
        let id = d.alloc_id();
        let parent = d.default_layer();
        d.insert(parent, usize::MAX, Node::path(id, shapes::rectangle(r), Appearance::basic(Paint::solid(color), Paint::None, 0.0)))?;
        Ok(id)
    })
    .unwrap()
}

fn capture(s: &mut Session, plate: &str) -> (Value, image::RgbaImage) {
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.objects.clone();
    let undo = s.doc().unwrap().history.undo.len();
    let proof = vectorcraft_render::proof::view();
    let report = s.execute("trapping.capture", &json!({"plate":plate,"dpi":72})).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(s.doc().unwrap().selection.objects, selection);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    assert_eq!(vectorcraft_render::proof::view(), proof);
    assert_eq!(report["experimental"], true);
    let png = vectorcraft_format::base64_decode(report["dataBase64"].as_str().unwrap()).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (report["width"].as_u64().unwrap() as u32, report["height"].as_u64().unwrap() as u32));
    assert!(img.pixels().all(|p| p.0[0].abs_diff(p.0[1]) <= 1 && p.0[1].abs_diff(p.0[2]) <= 1 && p.0[3] == 255));
    (report, img)
}

#[test]
fn capture_composes_visible_ink_and_respects_knockout_and_overprint() {
    let mut s = setup();
    rect(&mut s, Rect::new(0.0, 0.0, 100.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let yellow = rect(&mut s, Rect::new(30.0, 20.0, 80.0, 80.0), Color::cmyk(0.0, 0.0, 1.0, 0.0));
    let (_, cyan) = capture(&mut s, "Cyan");
    assert!(cyan.get_pixel(10, 50).0[0] < 3);
    assert!(cyan.get_pixel(50, 50).0[0] > 252, "yellow knocks out cyan");
    assert!(cyan.get_pixel(150, 50).0[0] > 252);
    s.edit("Overprint", |d, _| {
        d.node_mut(yellow).unwrap().appearance.fill_mut().unwrap().overprint = true;
        Ok(())
    })
    .unwrap();
    let (_, cyan) = capture(&mut s, "Cyan");
    assert!(cyan.get_pixel(50, 50).0[0] < 3, "zero cyan in OP yellow preserves underlying cyan");
    let (_, yellow) = capture(&mut s, "Yellow");
    assert!(yellow.get_pixel(50, 50).0[0] < 3);
    assert!(yellow.get_pixel(10, 50).0[0] > 252);
}

#[test]
fn linked_spot_identity_survives_identical_display_alternates() {
    let mut s = setup();
    let a = rect(&mut s, Rect::new(0.0, 0.0, 80.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = rect(&mut s, Rect::new(100.0, 0.0, 180.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    for (id, name) in [(a, "Ink A"), (b, "Ink B")] {
        s.execute("swatch.new", &json!({"name":name,"spot":true,"color":{"model":"cmyk","c":1,"m":0,"y":0,"k":0}})).unwrap();
        s.execute("paint.setFill", &json!({"ids":[id.0],"swatch":name,"tint":50})).unwrap();
    }
    let (report, ink) = capture(&mut s, "Ink A");
    assert!(ink.get_pixel(30, 50).0[0].abs_diff(128) < 3);
    assert!(ink.get_pixel(130, 50).0[0] > 252);
    assert!(report["plates"].as_array().unwrap().iter().any(|p| p["name"] == "Ink B" && p["spot"] == true));
    let (_, cyan) = capture(&mut s, "Cyan");
    assert!(cyan.get_pixel(30, 50).0[0] > 252, "spot alternate is not a process plate");
}

#[test]
fn capture_handles_more_than_eighty_objects_without_changing_apply_limits() {
    let mut s = setup();
    s.edit("Tessellated fixture", |d, _| {
        let parent = d.default_layer();
        for x in 0..200 {
            let id = d.alloc_id();
            d.insert(
                parent,
                usize::MAX,
                Node::path(
                    id,
                    shapes::rectangle(Rect::new(x as f64, 0.0, x as f64 + 1.0, 100.0)),
                    Appearance::basic(Paint::solid(Color::cmyk(1.0, 0.0, 0.0, 0.0)), Paint::None, 0.0),
                ),
            )?;
        }
        Ok(())
    })
    .unwrap();
    s.execute("select.all", &json!({})).unwrap();
    assert!(s.execute("trapping.apply", &json!({})).is_err());
    let (_, ink) = capture(&mut s, "Cyan");
    for x in 0..200 {
        assert!(ink.get_pixel(x, 50).0[0] < 3, "internal tessellation has no ink boundary at {x}");
    }
    s.execute("select.none", &json!({})).unwrap();
    let (_, no_selection) = capture(&mut s, "Cyan");
    assert_eq!(ink, no_selection);
}

fn put_image(s: &mut Session, blob: ImageBlob, width: u32, height: u32) -> NodeId {
    s.edit("Image fixture", |d, _| {
        let id = d.alloc_id();
        d.images.insert("fixture".into(), blob);
        let im = ImageObject {
            key: "fixture".into(),
            width,
            height,
            xf: Affine::scale_non_uniform(100.0 / f64::from(width), 100.0 / f64::from(height)),
            link: None,
            placement: Default::default(),
        };
        d.insert(d.default_layer(), usize::MAX, Node::new(id, NodeKind::Image(Box::new(im))))?;
        Ok(id)
    })
    .unwrap()
}

#[test]
fn transparent_raster_pixels_knock_out_only_where_the_image_paints_and_cmyk_samples_survive() {
    let mut s = setup();
    rect(&mut s, Rect::new(0.0, 0.0, 200.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let rgba = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 0, 255, 0, 0, 255]).unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    put_image(&mut s, ImageBlob::png(bytes.into_inner()), 2, 1);
    let original = s.doc().unwrap().doc.images["fixture"].bytes.clone();
    let (_, ink) = capture(&mut s, "Cyan");
    assert!(ink.get_pixel(15, 50).0[0] < 3, "transparent image leaves underlying cyan");
    assert!(ink.get_pixel(85, 50).0[0] > 200, "opaque red image replaces underlying cyan");
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().doc.images["fixture"].bytes));
    // CMYK images must not take a lossy RGB round trip.
    let mut s = setup();
    let samples = vectorcraft_doc::cmyk::Inks::new(1, 1, vec![51, 102, 153, 204]).unwrap();
    put_image(&mut s, ImageBlob::cmyk_tiff(&samples).unwrap(), 1, 1);
    for (plate, amount) in [("Cyan", 51u8), ("Magenta", 102), ("Yellow", 153), ("Black", 204)] {
        let (_, ink) = capture(&mut s, plate);
        assert!(ink.get_pixel(50, 50).0[0].abs_diff(255 - amount) <= 2, "{plate}");
    }
}

#[test]
fn clip_groups_keep_original_occluders_and_remove_hidden_art() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><clipPath id="c"><rect width="50" height="100"/></clipPath></defs><g clip-path="url(#c)"><rect width="100" height="100" fill="#000000"/></g><rect x="100" width="80" height="100" fill="#000000" display="none"/></svg>"##;
    let mut s = setup();
    s.execute("document.open", &json!({"name":"clip.svg","dataBase64":vectorcraft_format::base64_encode(svg.as_bytes())})).unwrap();
    let (_, ink) = capture(&mut s, "Black");
    assert!(ink.get_pixel(20, 40).0[0] < 10);
    assert!(ink.get_pixel(70, 40).0[0] > 252);
    assert!(ink.get_pixel(130, 40).0[0] > 252);
}

#[test]
fn invalid_capture_requests_fail_without_document_or_history_edits() {
    let mut s = setup();
    for p in [
        json!({"dpi":0}),
        json!({"dpi":601}),
        json!({"dpi":"bad"}),
        json!({"artboard":-1}),
        json!({"artboard":999}),
        json!({"plate":"missing"}),
        json!({"region":[0,0,1]}),
        json!({"region":[0,0,0,10]}),
        json!({"region":[0,0,10000,10000],"dpi":600}),
    ] {
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(s.execute("trapping.capture", &p).is_err(), "{p}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }
    assert!(vectorcraft_render::technical::capture_size(Rect::new(f64::NAN, 0.0, 10.0, 10.0), 72.0).is_err());
    assert!(vectorcraft_render::technical::capture_size(Rect::new(0.0, 0.0, 10.0, 10.0), f64::INFINITY).is_err());
    put_image(&mut s, ImageBlob::png(vec![1, 2, 3]), 1, 1);
    assert!(s.execute("trapping.capture", &json!({})).is_err());
}

/// User-owned artwork stays local, optional and never becomes a committed fixture.
#[test]
fn optional_real_job_plate_capture() {
    let Ok(path) = std::env::var("KX27_CAPTURE_SVG") else { return };
    let output = std::env::var("KX27_CAPTURE_OUT").unwrap_or_else(|_| "/tmp/kx27-phase1-job".into());
    std::fs::create_dir_all(&output).unwrap();
    let mut s = Session::new();
    let bytes = std::fs::read(path).unwrap();
    s.execute("document.open", &json!({"name":"user-job.svg","dataBase64":vectorcraft_format::base64_encode(&bytes)})).unwrap();
    let original = s.doc().unwrap().doc.clone();
    let region = original.artboards[0].rect;
    let mut renderer = vectorcraft_render::Renderer::new();
    let composite = renderer.render_region(&original, region, 1.0, true);
    std::fs::write(format!("{output}/composite.png"), composite.to_png().unwrap()).unwrap();
    for plate in ["Cyan", "Magenta", "Yellow", "Black"] {
        let start = std::time::Instant::now();
        let mut report = s.execute("trapping.capture", &json!({"plate":plate,"dpi":150})).unwrap();
        let png = vectorcraft_format::base64_decode(report["dataBase64"].as_str().unwrap()).unwrap();
        std::fs::write(format!("{output}/{plate}.png"), png).unwrap();
        report.as_object_mut().unwrap().remove("dataBase64");
        println!("{plate}: {:?}; {} × {} pixels", start.elapsed(), report["width"], report["height"]);
        std::fs::write(format!("{output}/{plate}.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    assert!(Arc::ptr_eq(&original, &s.doc().unwrap().doc));
    // The encoded document is unchanged; this feature creates no traps.
    assert_eq!(vectorcraft_format::save_file(&original), vectorcraft_format::save_file(&s.doc().unwrap().doc));
    let _doc: &Document = &original;
}

fn boundary_report(s: &mut Session, p: Value) -> Value {
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    let selected = s.doc().unwrap().selection.objects.clone();
    let proof = vectorcraft_render::proof::view();
    let r = s.execute("trapping.boundaries", &p).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(undo, s.doc().unwrap().history.undo.len());
    assert_eq!(selected, s.doc().unwrap().selection.objects);
    assert_eq!(proof, vectorcraft_render::proof::view());
    for key in ["dataBase64", "overlayBase64", "zoneMapBase64"] {
        let png = vectorcraft_format::base64_decode(r[key].as_str().unwrap()).unwrap();
        let im = image::load_from_memory(&png).unwrap();
        assert_eq!(u64::from(im.width()), r["width"].as_u64().unwrap());
        assert_eq!(u64::from(im.height()), r["height"].as_u64().unwrap());
    }
    r
}

fn two_inks() -> Session {
    let mut s = setup();
    rect(&mut s, Rect::new(0.0, 0.0, 100.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    rect(&mut s, Rect::new(100.0, 0.0, 200.0, 100.0), Color::cmyk(0.0, 1.0, 0.0, 0.0));
    s
}

#[test]
fn boundary_query_marks_a_visible_exchange_and_returns_document_space_bounds() {
    let mut s = two_inks();
    let r = boundary_report(&mut s, json!({}));
    assert_eq!(r["zoneCount"], 1);
    assert_eq!(r["zones"][0]["bounds"], json!([99.0, 2.0, 101.0, 98.0]));
    assert_eq!(r["zones"][0]["plates"], json!(["Cyan", "Magenta"]));
    let cropped = boundary_report(&mut s, json!({"region":[80,20,120,80]}));
    assert_eq!(cropped["width"], 40);
    assert_eq!(cropped["height"], 60);
    assert_eq!(cropped["zones"][0]["bounds"], json!([99.0, 22.0, 101.0, 78.0]));
    // A covered colour boundary is not a visible boundary.
    rect(&mut s, Rect::new(0.0, 0.0, 200.0, 100.0), Color::WHITE);
    assert_eq!(boundary_report(&mut s, json!({}))["zoneCount"], 0);
}

#[test]
fn boundary_query_distinguishes_spots_even_when_the_composite_colours_match() {
    let mut s = two_inks();
    s.execute("select.all", &json!({})).unwrap();
    let ids = s.doc().unwrap().selection.objects.clone();
    for (id, name) in ids.iter().zip(["Boundary Spot A", "Boundary Spot B"]) {
        s.execute("swatch.new", &json!({"name":name,"spot":true,"color":{"model":"cmyk","c":1,"m":0,"y":0,"k":0}})).unwrap();
        s.execute("paint.setFill", &json!({"ids":[id.0],"swatch":name})).unwrap();
    }
    let r = boundary_report(&mut s, json!({}));
    assert_eq!(r["zoneCount"], 1);
    assert_eq!(r["zones"][0]["plates"], json!(["Boundary Spot A", "Boundary Spot B"]));
    let bytes = vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap();
    let im = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!(im.get_pixel(20, 50), im.get_pixel(180, 50), "RGB alone cannot distinguish these inks");
}

#[test]
fn boundary_query_ignores_internal_same_colour_segments_and_resolved_white_gaps() {
    let mut s = setup();
    s.edit("Many fixture paths", |d, _| {
        for x in 0..200 {
            let id = d.alloc_id();
            let node = Node::path(
                id,
                shapes::rectangle(Rect::new(x as f64, 0.0, x as f64 + 1.0, 100.0)),
                Appearance::basic(Paint::solid(Color::cmyk(1.0, 0.0, 0.0, 0.0)), Paint::None, 0.0),
            );
            d.insert(d.default_layer(), usize::MAX, node)?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(boundary_report(&mut s, json!({}))["zoneCount"], 0);
    rect(&mut s, Rect::new(100.0, 0.0, 200.0, 100.0), Color::cmyk(0.0, 1.0, 0.0, 0.0));
    assert_eq!(boundary_report(&mut s, json!({}))["zoneCount"], 1);
    rect(&mut s, Rect::new(100.0, 0.0, 101.0, 100.0), Color::WHITE);
    assert_eq!(boundary_report(&mut s, json!({}))["zoneCount"], 0, "a resolved paper gap is not a contact");
}

#[test]
fn boundary_query_reads_image_inks_and_clips_before_detecting() {
    let mut s = setup();
    let samples: Vec<u8> = (0..200).flat_map(|x| if x < 100 { [255, 0, 0, 0] } else { [0, 255, 0, 0] }).collect();
    let inks = vectorcraft_doc::cmyk::Inks::new(200, 1, samples).unwrap();
    put_image(&mut s, ImageBlob::cmyk_tiff(&inks).unwrap(), 200, 1);
    let r = boundary_report(&mut s, json!({}));
    assert_eq!(r["zoneCount"], 1);
    assert_eq!(r["zones"][0]["plates"], json!(["Cyan", "Magenta"]));
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><clipPath id="c"><rect width="60" height="100"/></clipPath></defs><g clip-path="url(#c)"><rect width="100" height="100" fill="#00ffff"/><rect x="100" width="100" height="100" fill="#ff00ff"/></g></svg>"##;
    s.execute("document.open", &json!({"name":"clip.svg","dataBase64":vectorcraft_format::base64_encode(svg.as_bytes())})).unwrap();
    assert_eq!(boundary_report(&mut s, json!({}))["zoneCount"], 0, "the clipped-away encounter must disappear");
}

#[test]
fn boundary_query_rejects_invalid_parameters_and_budgets_without_edits() {
    let mut s = two_inks();
    for p in [
        json!({"contrast":0}),
        json!({"contrast":101}),
        json!({"dpi":"bad"}),
        json!({"dpi":0}),
        json!({"minPixels":1}),
        json!({"minPixels":1001}),
        json!({"minPixels":2.5}),
        json!({"region":[0,0,2000,2000]}),
        json!({"region":[0,0,5]}),
    ] {
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(s.execute("trapping.boundaries", &p).is_err(), "{p}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(undo, s.doc().unwrap().history.undo.len());
    }
    for i in 0..9 {
        s.execute("swatch.new", &json!({"name":format!("Many spots {i}"),"spot":true,"color":"#00ffff"})).unwrap();
    }
    assert!(s.execute("trapping.boundaries", &json!({})).unwrap_err().to_string().contains("12 tintas"));
}

#[test]
fn optional_real_job_boundary_capture() {
    let Ok(path) = std::env::var("KX27_BOUNDARY_SVG") else { return };
    let output = std::env::var("KX27_BOUNDARY_OUT").unwrap_or_else(|_| "/tmp/kx27-phase2-job".into());
    std::fs::create_dir_all(&output).unwrap();
    let mut s = Session::new();
    let bytes = std::fs::read(path).unwrap();
    s.execute("document.open", &json!({"name":"user-job.svg","dataBase64":vectorcraft_format::base64_encode(&bytes)})).unwrap();
    let started = std::time::Instant::now();
    let mut r = boundary_report(&mut s, json!({}));
    for (key, name) in [("dataBase64", "composite.png"), ("overlayBase64", "overlay.png")] {
        let bytes = vectorcraft_format::base64_decode(r[key].as_str().unwrap()).unwrap();
        std::fs::write(format!("{output}/{name}"), bytes).unwrap();
        r.as_object_mut().unwrap().remove(key);
    }
    println!("{} zones; {} marked pixels; {} × {} px; {:?}", r["zoneCount"], r["markedPixels"], r["width"], r["height"], started.elapsed());
    std::fs::write(format!("{output}/report.json"), serde_json::to_vec_pretty(&r).unwrap()).unwrap();
    assert!(r["zoneCount"].as_u64().unwrap() > 0);
}

fn attributed_pairs(r: &Value) -> std::collections::BTreeSet<Vec<u64>> {
    r["zones"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|z| z["attribution"]["pairs"].as_array().unwrap())
        .map(|p| p["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect())
        .collect()
}

#[test]
fn source_map_attributes_only_visible_objects_and_preserves_spot_identity() {
    let mut s = setup();
    let hidden = rect(&mut s, Rect::new(0.0, 0.0, 200.0, 100.0), Color::cmyk(0.0, 0.0, 1.0, 0.0));
    let a = rect(&mut s, Rect::new(0.0, 0.0, 100.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = rect(&mut s, Rect::new(100.0, 0.0, 200.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    for (id, name) in [(a, "Spot A"), (b, "Spot B")] {
        s.execute("swatch.new", &json!({"name":name,"spot":true,"color":{"model":"cmyk","c":1,"m":0,"y":0,"k":0}})).unwrap();
        s.execute("paint.setFill", &json!({"ids":[id.0],"swatch":name})).unwrap();
    }
    let r = boundary_report(&mut s, json!({}));
    assert_eq!(attributed_pairs(&r), [vec![a.0, b.0]].into());
    assert_eq!(r["zones"][0]["attribution"]["status"], "resolved");
    assert!(r["sourceObjects"].as_array().unwrap().iter().all(|o| o["id"] != hidden.0));
    assert_eq!(r["sourceObjects"][0]["plates"], json!(["Spot A"]));
    assert_eq!(r["sourceObjects"][1]["plates"], json!(["Spot B"]));
    let cropped = boundary_report(&mut s, json!({"region":[80,20,120,80],"dpi":144}));
    assert_eq!(attributed_pairs(&cropped), [vec![a.0, b.0]].into());
    let bytes = vectorcraft_format::base64_decode(cropped["zoneMapBase64"].as_str().unwrap()).unwrap();
    let map = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!(map.get_pixel(39, 30).0[0], 1);
    assert_eq!(map.get_pixel(10, 30).0[0], 0);
}

#[test]
fn source_map_splits_a_connected_zone_into_multiple_object_pairs() {
    let mut s = setup();
    let a = rect(&mut s, Rect::new(0.0, 0.0, 100.0, 50.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = rect(&mut s, Rect::new(100.0, 0.0, 200.0, 100.0), Color::cmyk(0.0, 1.0, 0.0, 0.0));
    let c = rect(&mut s, Rect::new(0.0, 50.0, 100.0, 100.0), Color::cmyk(0.0, 0.0, 1.0, 0.0));
    let r = boundary_report(&mut s, json!({}));
    assert_eq!(r["zoneCount"], 1);
    assert_eq!(attributed_pairs(&r), [vec![a.0, b.0], vec![a.0, c.0], vec![b.0, c.0]].into());
    assert!(r["zones"][0]["attribution"]["sampledProbes"].as_u64().unwrap() <= 256);
}

#[test]
fn source_map_blocks_unsupported_occluders_instead_of_attributing_hidden_solids() {
    for mode in ["opacity", "overprint", "gradient", "stroke", "mask"] {
        let mut s = two_inks();
        let id = rect(&mut s, Rect::new(0.0, 0.0, 200.0, 100.0), Color::cmyk(0.0, 0.0, 1.0, 0.0));
        s.edit("Unsupported occluder", |d, _| {
            let n = d.node_mut(id).unwrap();
            match mode {
                "opacity" => n.opacity = 0.3,
                "overprint" => n.appearance.fill_mut().unwrap().overprint = true,
                "stroke" => {
                    n.appearance.stroke_mut().unwrap().paint = Paint::solid(Color::BLACK);
                    n.appearance.stroke_mut().unwrap().width = 1.0;
                    n.opacity = 0.3;
                }
                "mask" => {
                    // A vector opacity mask contributes uncertain compositing.
                    let mask = Node::path(
                        NodeId(9999),
                        shapes::rectangle(Rect::new(0.0, 0.0, 200.0, 100.0)),
                        Appearance::basic(Paint::solid(Color::rgb(0.5, 0.5, 0.5)), Paint::None, 0.0),
                    );
                    n.extras_mut().mask = Some(Box::new(vectorcraft_doc::OpacityMask::new(mask, true)));
                }
                _ => n.opacity = 0.3,
            }
            Ok(())
        })
        .unwrap();
        if mode == "gradient" {
            s.execute(
                "paint.setFill",
                &json!({"ids":[id.0],"gradient":{"kind":"linear","stops":[{"offset":0,"color":"#ffff00"},{"offset":1,"color":"#ffffff"}]}}),
            )
            .unwrap();
        }
        let r = boundary_report(&mut s, json!({}));
        assert!(r["zoneCount"].as_u64().unwrap() > 0, "{mode}");
        assert!(attributed_pairs(&r).is_empty(), "{mode}: {r}");
    }
}

#[test]
fn source_map_respects_clips_and_more_than_eighty_objects() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><clipPath id="c"><rect x="0" y="0" width="200" height="50"/></clipPath></defs><g clip-path="url(#c)"><rect width="100" height="100" fill="#00ffff"/><rect x="100" width="100" height="100" fill="#ff00ff"/></g><rect width="200" height="100" fill="#ffff00" display="none"/></svg>"##;
    let mut s = Session::new();
    s.execute("document.open", &json!({"name":"original.svg","dataBase64":vectorcraft_format::base64_encode(svg.as_bytes())})).unwrap();
    for _ in 0..100 {
        rect(&mut s, Rect::new(-20.0, -20.0, -10.0, -10.0), Color::WHITE);
    }
    let r = boundary_report(&mut s, json!({}));
    assert_eq!(r["zoneCount"], 1);
    assert_eq!(attributed_pairs(&r).len(), 1);
    assert_eq!(r["sourceObjects"].as_array().unwrap().len(), 2);
    assert!(r["zones"][0]["bounds"][3].as_f64().unwrap() <= 50.0);
    // The clip path remains geometry, never an inferred source on the clipped edge.
    assert!(r["sourceObjects"][0]["plates"].as_array().unwrap().contains(&json!("Cyan")));
    assert!(r["sourceObjects"][1]["plates"].as_array().unwrap().contains(&json!("Magenta")));
}

#[test]
fn source_ids_above_one_byte_and_compound_holes_keep_original_identity() {
    let mut s = setup();
    s.edit("Many originals", |d, _| {
        let layer = d.default_layer();
        for _ in 0..300 {
            let id = d.alloc_id();
            d.insert(
                layer,
                usize::MAX,
                Node::path(
                    id,
                    shapes::rectangle(Rect::new(-10.0, -10.0, -5.0, -5.0)),
                    Appearance::basic(Paint::solid(Color::WHITE), Paint::None, 0.0),
                ),
            )?;
        }
        Ok(())
    })
    .unwrap();
    let a = rect(&mut s, Rect::new(0.0, 0.0, 100.0, 100.0), Color::cmyk(1.0, 0.0, 0.0, 0.0));
    let b = rect(&mut s, Rect::new(100.0, 0.0, 200.0, 100.0), Color::cmyk(0.0, 1.0, 0.0, 0.0));
    assert_eq!(attributed_pairs(&boundary_report(&mut s, json!({}))), [vec![a.0, b.0]].into());
    // A filled hole shows its lower source; it must not be attributed to the ring
    // merely because it lies inside the ring's bounding rectangle.
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="200" height="100" fill="#ff00ff"/><path fill="#00ffff" fill-rule="evenodd" d="M10 10H190V90H10Z M60 30H140V70H60Z"/></svg>"##;
    s.execute("document.open", &json!({"name":"hole.svg","dataBase64":vectorcraft_format::base64_encode(svg.as_bytes())})).unwrap();
    let r = boundary_report(&mut s, json!({}));
    assert_eq!(r["zoneCount"], 2);
    assert_eq!(r["sourceObjects"].as_array().unwrap().len(), 2);
    assert_eq!(attributed_pairs(&r).len(), 1);
    assert!(r["zones"].as_array().unwrap().iter().all(|z| !z["attribution"]["pairs"].as_array().unwrap().is_empty()));
}
