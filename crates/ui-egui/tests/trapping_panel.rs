#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;
use vectorcraft_engine::Session;
use vectorcraft_ui_egui::{
    VectorcraftApp,
    panels::trapping::{ID, show},
};

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Default::default());
    app.run("file.new", json!({"width":200, "height":150})).unwrap();
    for (x, size, color) in [(0, 100, "#00ffff"), (30, 40, "#ffff00")] {
        app.run("shape.rectangle", json!({"x":x, "y":x, "width":size, "height":size})).unwrap();
        app.run("paint.setFill", json!({"color":color})).unwrap();
        app.run("paint.setStroke", json!({"none":true})).unwrap();
    }
    app.run("select.all", json!({})).unwrap();
    app.run("window.panel", json!({"panel":ID})).unwrap();
    app
}

#[test]
fn trapping_panel_stays_open_after_apply_and_settings_and_docking_roundtrip() {
    let mut app = app();
    app.ui.trapping.width_mm = 0.25;
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 950.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                vectorcraft_ui_egui::theme::apply(ui.ctx(), vectorcraft_ui_egui::theme::Brightness::MediumDark);
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Aplicar a la selección").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(h.state().ui.open_panel.as_deref(), Some(ID));
    let snapshot = vectorcraft_engine::cmd::trapping::snapshot(&h.state().session).unwrap();
    assert_eq!(snapshot["pairs"].as_array().unwrap().len(), 1);
    h.get_by_label("Actualizar par");
    let app = h.state_mut();
    app.run("window.panel.float", json!({"panel":ID, "x":40,"y":60,"width":340,"height":650})).unwrap();
    let saved = serde_json::to_value(&app.ui).unwrap();
    let restored: vectorcraft_ui_egui::state::UiState = serde_json::from_value(saved).unwrap();
    assert_eq!(restored.trapping.width_mm, 0.25);
    assert_eq!(restored.docking, app.ui.docking);
    assert!(restored.trapping.message.is_empty());
}

#[test]
fn trapping_panel_reports_empty_selection_without_calculating() {
    let mut app = VectorcraftApp::new(Session::new(), Default::default());
    let ctx = egui::Context::default();
    vectorcraft_ui_egui::theme::install_fonts(&ctx);
    egui_extras::install_image_loaders(&ctx);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui));
    output.textures_delta.clear();
    assert!(app.session.active().is_none());
    assert!(app.ui.dialog.is_none());
}

/// Optional captures of the actual egui panel, including its populated per-pair controls.
#[test]
fn trapping_panel_visual_capture() {
    let Ok(dir) = std::env::var("TRAPCOLOR_SCREENSHOTS") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, width) in [(vectorcraft_ui_egui::theme::Brightness::MediumDark, 340.0), (vectorcraft_ui_egui::theme::Brightness::Light, 280.0)] {
        let mut app = if std::env::var("KX27_IMAGE_CAPTURE").is_ok() { image_app().0 } else { app() };
        if std::env::var("KX27_GRADIENT_CAPTURE").is_ok() {
            let id = app.session.doc().unwrap().selection.objects[0].0;
            app.run(
                "paint.setFill",
                json!({"ids":[id],"gradient":{"kind":"linear","stops":[
                {"offset":0,"color":{"model":"cmyk","c":1,"m":0,"y":0,"k":0}},
                {"offset":1,"color":{"model":"cmyk","c":0,"m":0,"y":0,"k":0}}]}}),
            )
            .unwrap();
            app.run("select.all", json!({})).unwrap();
        }
        if std::env::var("KX27_CURVE_CAPTURE").is_ok() {
            let id = app.session.doc().unwrap().selection.objects[1];
            app.session
                .edit("Synthetic original curve", |d, _| {
                    let n = d.node_mut(id).unwrap();
                    *n = vectorcraft_doc::Node::path(
                        id,
                        vectorcraft_geom::shapes::ellipse(vectorcraft_geom::Rect::new(30.0, 30.0, 70.0, 70.0)),
                        (*n.appearance).clone(),
                    );
                    Ok(())
                })
                .unwrap();
        }
        if std::env::var("KX27_BOUNDARY_CAPTURE").is_ok() {
            if let Ok(path) = std::env::var("KX27_BOUNDARY_CAPTURE_SVG") {
                let bytes = std::fs::read(path).unwrap();
                app.run("document.open", json!({"name":"user-job.svg","dataBase64":vectorcraft_format::base64_encode(&bytes)})).unwrap();
            }
        } else {
            app.run("trapping.apply", json!({})).unwrap();
        }
        let mut ready = false;
        let mut h = Harness::builder()
            .with_size(egui::vec2(
                width,
                if std::env::var("KX27_PREVIEW_CAPTURE").is_ok() {
                    2400.0
                } else if std::env::var("KX27_SOURCE_CAPTURE").is_ok() {
                    1600.0
                } else {
                    1050.0
                },
            ))
            .wgpu()
            .build_ui_state(
                move |ui, app: &mut VectorcraftApp| {
                    if !ready {
                        vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                        egui_extras::install_image_loaders(ui.ctx());
                        vectorcraft_ui_egui::theme::apply(ui.ctx(), theme);
                        ready = true;
                        return;
                    }
                    show(app, ui);
                },
                app,
            );
        h.run_steps(4);
        if std::env::var("KX27_BOUNDARY_CAPTURE").is_ok() {
            h.get_by_label("Fronteras visibles · Fase 2").click();
            h.run_steps(4);
            h.get_by_label("Detectar fronteras").click();
        } else if std::env::var("KX27_TECHNICAL_CAPTURE").is_ok() {
            h.get_by_label("Captura por tintas · Fase 1").click();
            h.run_steps(4);
            h.get_by_label("Capturar tinta").click();
        } else {
            h.get_by_label("Analizar contactos").click();
        }
        h.run_steps(4);
        if std::env::var("KX27_SOURCE_CAPTURE").is_ok() {
            h.get_by_label_contains("Zona 1 ·").click();
            h.run_steps(4);
        }
        if std::env::var("KX27_CURVE_CAPTURE").is_ok() {
            h.get_by_label("Verificar curvas del par 1").click();
            h.run_steps(4);
        }
        if std::env::var("KX27_PREVIEW_CAPTURE").is_ok() {
            h.get_by_label("Previsualizar franja").click();
            h.run_steps(4);
            assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
        }
        let rendered = h.render().unwrap();
        if std::env::var("KX27_CURVE_CAPTURE").is_ok() {
            let color = vectorcraft_ui_egui::theme::Tokens::for_brightness(theme).axis_y.to_array();
            let count =
                rendered.pixels().filter(|p| p.0[0].abs_diff(color[0]) < 4 && p.0[1].abs_diff(color[1]) < 4 && p.0[2].abs_diff(color[2]) < 4).count();
            assert!(count > 10, "verified samples must remain visibly green in both themes: {count}");
        }
        rendered.save(std::path::Path::new(&dir).join(format!("trapping-{}-{width}.png", theme.id()))).unwrap();
    }
}

#[test]
fn visible_boundary_map_can_be_reviewed_without_editing_and_invalidates_when_needed() {
    let app = app();
    let before = app.session.doc().unwrap().doc.clone();
    let selection = app.session.doc().unwrap().selection.objects.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 1600.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Fronteras visibles · Fase 2").click();
    h.run_steps(4);
    h.get_by_label("Detectar fronteras").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    h.get_by_label("Mostrar marcas rojas").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.boundary_show);
    h.get_by_label_contains("Zona 1 ·").click();
    h.run_steps(4);
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.doc().unwrap().doc));
    assert_eq!(selection, h.state().session.doc().unwrap().selection.objects);
    assert_eq!(undo, h.state().session.doc().unwrap().history.undo.len());
    let saved = serde_json::to_value(&h.state().ui.trapping).unwrap();
    assert!(saved.get("boundaries").is_none());
    h.state_mut().ui.trapping.boundary_contrast = 50.0;
    h.run_steps(4);
    assert!(h.query_by_label("Mostrar marcas rojas").is_none(), "contrast changes invalidate the map");
    h.get_by_label("Detectar fronteras").click();
    h.run_steps(4);
    h.get_by_label("Mostrar marcas rojas");
    h.state_mut().run("shape.rectangle", json!({"x":0,"y":0,"width":5,"height":5})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("Mostrar marcas rojas").is_none(), "document edits invalidate the map");
}

#[test]
fn experimental_capture_is_read_only_and_the_panel_stays_available() {
    let mut app = app();
    app.ui.trapping.capture_dpi = 72.0;
    let before = app.session.doc().unwrap().doc.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 1400.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Captura por tintas · Fase 1").click();
    h.run_steps(4);
    h.get_by_label("Capturar tinta").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    h.get_by_label("Cyan · 200 × 150 px · 0.353 mm/píxel");
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.doc().unwrap().doc));
    assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo);
    h.state_mut().run("shape.rectangle", json!({"x":0,"y":0,"width":5,"height":5})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("Cyan · 200 × 150 px · 0.353 mm/píxel").is_none(), "edited document invalidates capture");
    h.get_by_label("Capturar tinta");
    h.state_mut().ui.trapping.capture_dpi = 600.0;
    h.input_mut().max_texture_side = Some(1024);
    h.get_by_label("Capturar tinta").click();
    h.run_steps(4);
    assert!(h.state().ui.trapping.failed);
    assert!(h.state().ui.trapping.message.contains("límite gráfico de 1024"));
}

type DiagnosticFiles = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<u8>)>>>;

fn diagnostic_download(app: &mut VectorcraftApp) -> DiagnosticFiles {
    let files = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let copy = files.clone();
    app.services.download = Some(Box::new(move |name, bytes| copy.borrow_mut().push((name.to_string(), bytes.to_vec()))));
    files
}

#[test]
fn diagnostic_json_download_preserves_document_selection_and_original_error() {
    let mut app = app();
    let files = diagnostic_download(&mut app);
    let before = app.session.doc().unwrap().doc.clone();
    let selection = app.session.doc().unwrap().selection.objects.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    app.ui.trapping.message = "Prueba de error original".into();
    app.ui.trapping.failed = true;
    app.run("trapping.exportDiagnostic", json!({})).unwrap();
    let files = files.borrow();
    assert_eq!(files.len(), 1);
    assert!(files[0].0.ends_with("-trapping.json"));
    let r: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    assert_eq!(r["format"], "kx27-trapping-diagnostic");
    assert_eq!(r["schemaVersion"], 1);
    assert_eq!(r["panelStatus"]["failed"], true);
    assert_eq!(r["panelStatus"]["message"], "Prueba de error original");
    assert_eq!(r["objects"]["samples"].as_array().unwrap().len(), 2);
    assert_eq!(r["objects"]["samples"][0]["paints"][0]["paint"]["color"]["model"], "rgb");
    assert_eq!(r["analyses"]["boundaries"]["status"], "notAvailable");
    assert_eq!(r["fullGeometryIncluded"], false);
    assert_eq!(r["imageBytesIncluded"], false);
    assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
    assert_eq!(selection, app.session.doc().unwrap().selection.objects);
    assert_eq!(undo, app.session.doc().unwrap().history.undo.len());
    assert!(app.ui.trapping.failed);
    assert_eq!(app.ui.trapping.message, "Prueba de error original");
}

#[test]
fn diagnostic_json_includes_linked_spots_and_handles_file_extensions_and_cancel() {
    let mut app = app();
    let id = app.session.doc().unwrap().selection.objects[0].0;
    app.run("swatch.new", json!({"name":"Spot Test","spot":true,"color":{"model":"lab","l":55,"a":20,"b":30}})).unwrap();
    app.run("paint.setFill", json!({"ids":[id],"swatch":"Spot Test","tint":40})).unwrap();
    let files = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Vec<u8>)>::new()));
    let copy = files.clone();
    app.services.write = Some(Box::new(move |path, bytes| {
        copy.borrow_mut().push((path.to_string(), bytes.to_vec()));
        Ok(())
    }));
    app.services.pick_save = Some(Box::new(|pick| {
        assert!(pick.name.ends_with("-trapping.json"));
        Some("/tmp/art.svg".into())
    }));
    let before = app.session.doc().unwrap().doc.clone();
    app.run("trapping.exportDiagnostic", json!({})).unwrap();
    assert_eq!(files.borrow()[0].0, "/tmp/art.svg.json", "never overwrite a picked source document");
    let r: serde_json::Value = serde_json::from_slice(&files.borrow()[0].1).unwrap();
    assert!(r["plates"].as_array().unwrap().iter().any(|p| p["name"] == "Spot Test" && p["spot"] == true));
    assert!(r["objects"]["samples"].as_array().unwrap().iter().any(|o| o["paints"][0]["paint"]["swatch"] == "Spot Test"));
    app.run("trapping.exportDiagnostic", json!({"path":"/tmp/check"})).unwrap();
    assert_eq!(files.borrow()[1].0, "/tmp/check.json");
    app.services.pick_save = Some(Box::new(|_| None));
    assert!(app.run("trapping.exportDiagnostic", json!({})).is_err());
    assert_eq!(files.borrow().len(), 2);
    assert!(app.run("trapping.exportDiagnostic", json!({"path":123})).is_err());
    assert!(app.run("trapping.exportDiagnostic", json!({"path":""})).is_err());
    assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
}

#[test]
fn diagnostic_exports_current_reports_and_excludes_stale_reports_and_image_payloads() {
    let mut app = app();
    let files = diagnostic_download(&mut app);
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 2000.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Analizar contactos").click();
    h.run_steps(4);
    h.get_by_label("Captura por tintas · Fase 1").click();
    h.run_steps(4);
    h.get_by_label("Capturar tinta").click();
    h.run_steps(4);
    h.get_by_label("Fronteras visibles · Fase 2").click();
    h.run_steps(4);
    h.get_by_label("Detectar fronteras").click();
    h.run_steps(4);
    let before = h.state().session.doc().unwrap().doc.clone();
    let undo = h.state().session.doc().unwrap().history.undo.len();
    h.get_by_label("Exportar diagnóstico (.json)").click();
    h.run_steps(4);
    let r: serde_json::Value = serde_json::from_slice(&files.borrow()[0].1).unwrap();
    for kind in ["contacts", "capture", "boundaries"] {
        assert_eq!(r["analyses"][kind]["status"], "current", "{kind}");
    }
    assert!(r["analyses"]["boundaries"]["report"]["zoneCount"].as_u64().unwrap() > 0);
    let raw = String::from_utf8(files.borrow()[0].1.clone()).unwrap();
    assert!(!raw.contains("dataBase64"));
    assert!(!raw.contains("overlayBase64"));
    assert!(!raw.contains("zoneMapBase64"));
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.doc().unwrap().doc));
    assert_eq!(undo, h.state().session.doc().unwrap().history.undo.len());
    // API invocation before another UI frame must re-check all cache identities itself.
    h.state_mut().run("shape.rectangle", json!({"x":0,"y":0,"width":5,"height":5})).unwrap();
    h.state_mut().run("trapping.exportDiagnostic", json!({})).unwrap();
    let r: serde_json::Value = serde_json::from_slice(&files.borrow()[1].1).unwrap();
    for kind in ["contacts", "capture", "boundaries"] {
        assert_eq!(r["analyses"][kind]["status"], "stale", "{kind}");
        assert!(r["analyses"][kind]["report"].is_null());
    }
}

#[test]
fn diagnostic_export_rejects_writer_failure_and_missing_document() {
    let mut app = app();
    let before = app.session.doc().unwrap().doc.clone();
    app.services.write = Some(Box::new(|_, _| Err("writer test failure".into())));
    assert!(app.run("trapping.exportDiagnostic", json!({"path":"/tmp/diagnostic.json"})).unwrap_err().contains("writer test failure"));
    assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
    let mut empty = VectorcraftApp::new(Session::new(), Default::default());
    assert!(empty.run("trapping.exportDiagnostic", json!({"path":"/tmp/none.json"})).is_err());
}

#[test]
fn diagnostic_stays_available_after_a_real_eighty_object_rejection_and_bounds_samples() {
    let mut app = app();
    let files = diagnostic_download(&mut app);
    app.session
        .edit("Diagnostic fixture", |d, _| {
            for i in 0..220 {
                let id = d.alloc_id();
                let x = f64::from(i);
                let path = vectorcraft_geom::shapes::rectangle(vectorcraft_geom::Rect::new(x, 0.0, x + 1.0, 10.0));
                let appearance = vectorcraft_doc::Appearance::basic(
                    vectorcraft_color::Paint::solid(vectorcraft_color::Color::cmyk(1.0, 0.0, 0.0, 0.0)),
                    vectorcraft_color::Paint::None,
                    0.0,
                );
                d.insert(d.default_layer(), usize::MAX, vectorcraft_doc::Node::path(id, path, appearance))?;
            }
            Ok(())
        })
        .unwrap();
    app.run("select.all", json!({})).unwrap();
    let error = app.run("trapping.apply", json!({})).unwrap_err();
    assert!(error.contains("80"));
    app.ui.trapping.message = error;
    app.ui.trapping.failed = true;
    let before = app.session.doc().unwrap().doc.clone();
    app.run("trapping.exportDiagnostic", json!({})).unwrap();
    let r: serde_json::Value = serde_json::from_slice(&files.borrow()[0].1).unwrap();
    assert_eq!(r["objects"]["samples"].as_array().unwrap().len(), 200);
    assert_eq!(r["objects"]["samplesTruncated"], true);
    assert_eq!(r["panelStatus"]["failed"], true);
    assert!(r["panelStatus"]["message"].as_str().unwrap().contains("80"));
    assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
}

#[test]
fn spot_names_remain_readable_with_only_the_trap_selected() {
    let mut app = app();
    let id = app.session.doc().unwrap().selection.objects[0].0;
    let name = "Tinta prueba /青 con nombre largo";
    app.run("swatch.new", json!({"name":name,"spot":true,"color":{"model":"lab","l":55,"a":40,"b":20}})).unwrap();
    app.run("paint.setFill", json!({"ids":[id],"swatch":name,"tint":40})).unwrap();
    let r = app.run("trapping.apply", json!({"difference":0})).unwrap();
    app.run("select.set", json!({"ids":[r["pairs"][0]["id"]]})).unwrap();
    let snapshot = vectorcraft_engine::cmd::trapping::snapshot(&app.session).unwrap();
    let pair = &snapshot["pairs"][0];
    let label = format!("{} / {}", pair["left"].as_str().unwrap(), pair["right"].as_str().unwrap());
    assert!(label.contains(name));
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(280.0, 600.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label(&label);
    h.get_by_label("Localizar");
    assert!(!h.state().ui.trapping.failed);
}

#[test]
fn analyze_preserves_art_and_invalidates_after_rule_or_document_changes() {
    let mut app = app();
    app.run("shape.rectangle", json!({"x":160,"y":120,"width":10,"height":10})).unwrap();
    app.run("paint.setFill", json!({"color":"#ff00ff"})).unwrap();
    app.run("paint.setStroke", json!({"none":true})).unwrap();
    app.run("select.all", json!({})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 1100.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Analizar contactos").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    assert!(std::sync::Arc::ptr_eq(&doc, &h.state().session.doc().unwrap().doc));
    assert_eq!(undo, h.state().session.doc().unwrap().history.undo.len());
    h.get_by_label("Seleccionar pareja").click();
    h.run_steps(4);
    h.get_by_label("Análisis de contactos");
    h.get_by_label("Recuperar selección analizada").click();
    h.run_steps(4);
    assert_eq!(h.state().session.doc().unwrap().selection.objects.len(), 3);
    let serialized = serde_json::to_value(&h.state().ui.trapping).unwrap();
    assert!(serialized.get("analysis").is_none());
    h.state_mut().ui.trapping.width_mm = 0.3;
    h.run_steps(4);
    assert!(h.state().ui.trapping.message.contains("cambiaron"));
    h.get_by_label("Analizar contactos").click();
    h.run_steps(4);
    h.state_mut().run("object.move", json!({"dx":1,"dy":0})).unwrap();
    h.run_steps(4);
    assert!(h.state().ui.trapping.message.contains("cambiaron"));
}

#[test]
fn gradient_analysis_and_apply_show_fixed_rule_without_internal_keys() {
    let mut app = app();
    let ids = app.session.doc().unwrap().selection.objects.clone();
    app.run(
        "paint.setFill",
        json!({"ids":[ids[0].0],"gradient":{"kind":"linear","stops":[
        {"offset":0,"color":{"model":"cmyk","c":1,"m":0,"y":0,"k":0}},
        {"offset":1,"color":{"model":"cmyk","c":0,"m":0,"y":0,"k":0}}]}}),
    )
    .unwrap();
    app.run("select.all", json!({})).unwrap();
    app.ui.trapping.direction = 2;
    app.ui.trapping.difference = 100.0;
    let before = app.session.doc().unwrap().doc.clone();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 1250.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Analizar contactos").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.doc().unwrap().doc));
    h.get_by_label("Aplicar a la selección").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    let report = vectorcraft_engine::cmd::trapping::snapshot(&h.state().session).unwrap();
    let p = &report["pairs"][0];
    assert_eq!(p["solidIntoGradient"], true);
    assert_eq!(p["relation"], ">");
    assert!(p["right"].as_str().unwrap().contains("Degradado"));
    assert!(!p["name"].as_str().unwrap().contains("GL"));
    h.get_by_label("Actualizar par").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed);
}

fn image_app() -> (VectorcraftApp, vectorcraft_doc::NodeId) {
    let mut app = app();
    app.run("select.all", json!({})).unwrap();
    app.run("edit.clear", json!({})).unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([0, 180, 220]))).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let r = app
        .run(
            "file.place",
            json!({"name":"Foto.png","dataBase64":vectorcraft_format::base64_encode(&png.into_inner()),"link":false,"rect":[0,0,100,100]}),
        )
        .unwrap();
    let id = vectorcraft_doc::NodeId(r["ids"][0].as_u64().unwrap());
    app.run("shape.rectangle", json!({"x":80,"y":0,"width":100,"height":100})).unwrap();
    app.run("paint.setFill", json!({"color":{"model":"cmyk","c":0,"m":0,"y":1,"k":0}})).unwrap();
    app.run("paint.setStroke", json!({"none":true})).unwrap();
    app.run("select.all", json!({})).unwrap();
    (app, id)
}

#[test]
fn image_frame_analysis_and_apply_keep_the_image_and_explain_the_rule() {
    let (app, id) = image_app();
    let original = app.session.doc().unwrap().doc.node(id).unwrap().clone();
    let before = app.session.doc().unwrap().doc.clone();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 1250.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Analizar contactos").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.doc().unwrap().doc));
    h.get_by_label("Aplicar a la selección").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    let r = vectorcraft_engine::cmd::trapping::snapshot(&h.state().session).unwrap();
    assert_eq!(r["pairs"][0]["solidIntoImage"], true);
    assert!(r["pairs"][0]["right"].as_str().unwrap().contains("Imagen"));
    assert_eq!(h.state().session.doc().unwrap().doc.node(id), Some(&original));
    h.get_by_label("Actualizar par").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed);
}

#[test]
fn boundary_zone_selects_original_sources_and_pair_button_keeps_document_and_history() {
    let mut app = app();
    app.run("select.none", json!({})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    let redo = app.session.doc().unwrap().history.redo.len();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 1900.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Fronteras visibles · Fase 2").click();
    h.run_steps(4);
    h.get_by_label("Detectar fronteras").click();
    h.run_steps(4);
    assert!(h.state().session.doc().unwrap().selection.objects.is_empty());
    let map = h.get_by_label("Mapa de fronteras visibles").rect();
    let pos = map.min + egui::vec2(map.width() * 30.5 / 200.0, map.height() * 50.5 / 150.0);
    h.event(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
    }
    h.run_steps(4);
    assert_eq!(h.state().session.doc().unwrap().selection.objects.len(), 2, "a marked map pixel selects both originals");
    h.state_mut().run("select.none", json!({})).unwrap();
    h.run_steps(4);
    h.get_by_label_contains("Zona 1 ·").click();
    h.run_steps(4);
    assert_eq!(h.state().session.doc().unwrap().selection.objects.len(), 2);
    h.get_by_label("Resaltar par 1").click();
    h.run_steps(4);
    assert_eq!(h.state().session.doc().unwrap().selection.objects.len(), 2);
    assert!(std::sync::Arc::ptr_eq(&doc, &h.state().session.doc().unwrap().doc));
    assert_eq!(undo, h.state().session.doc().unwrap().history.undo.len());
    assert_eq!(redo, h.state().session.doc().unwrap().history.redo.len());
    h.state_mut().run("shape.rectangle", json!({"x":110,"y":0,"width":10,"height":10})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("Resaltar par 1").is_none(), "source references must disappear when art changes");
}

#[test]
fn local_curve_button_exports_fresh_samples_without_edits_and_invalidates_immediately() {
    let mut app = app();
    let files = diagnostic_download(&mut app);
    let doc = app.session.doc().unwrap().doc.clone();
    let selected = app.session.doc().unwrap().selection.objects.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    let redo = app.session.doc().unwrap().history.redo.len();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 2200.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Fronteras visibles · Fase 2").click();
    h.run_steps(4);
    h.get_by_label("Detectar fronteras").click();
    h.run_steps(4);
    h.get_by_label_contains("Zona 1 ·").click();
    h.run_steps(4);
    h.get_by_label("Verificar curvas del par 1").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    h.get_by_label("Curvas del par 1");
    h.get_by_label("Exportar diagnóstico (.json)").click();
    h.run_steps(4);
    let r: serde_json::Value = serde_json::from_slice(&files.borrow()[0].1).unwrap();
    assert_eq!(r["analyses"]["curves"]["status"], "current");
    assert!(r["analyses"]["curves"]["report"]["visibleSamples"].as_u64().unwrap() > 0);
    assert!(!r["analyses"]["curves"]["report"]["fragments"].as_array().unwrap().is_empty());
    assert_eq!(r["localCurveSamplesIncluded"], true);
    assert_eq!(r["fullGeometryIncluded"], false);
    assert!(std::sync::Arc::ptr_eq(&doc, &h.state().session.doc().unwrap().doc));
    assert_eq!(selected, h.state().session.doc().unwrap().selection.objects);
    assert_eq!(undo, h.state().session.doc().unwrap().history.undo.len());
    assert_eq!(redo, h.state().session.doc().unwrap().history.redo.len());
    h.state_mut().run("shape.rectangle", json!({"x":5,"y":5,"width":10,"height":10})).unwrap();
    h.state_mut().run("trapping.exportDiagnostic", json!({})).unwrap();
    let r: serde_json::Value = serde_json::from_slice(&files.borrow()[1].1).unwrap();
    assert_eq!(r["analyses"]["curves"]["status"], "stale");
    assert!(r["analyses"]["curves"]["report"].is_null());
    h.run_steps(4);
    assert!(h.query_by_label("Curvas del par 1").is_none());
}

#[test]
fn local_band_preview_is_read_only_directional_and_exports_only_fresh_geometry() {
    let mut app = app();
    let files = diagnostic_download(&mut app);
    let doc = app.session.doc().unwrap().doc.clone();
    let selected = app.session.doc().unwrap().selection.objects.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    let redo = app.session.doc().unwrap().history.redo.len();
    let mut ready = false;
    let mut h = Harness::builder().with_size(egui::vec2(340.0, 3000.0)).build_ui_state(
        move |ui, app: &mut VectorcraftApp| {
            if !ready {
                vectorcraft_ui_egui::theme::install_fonts(ui.ctx());
                egui_extras::install_image_loaders(ui.ctx());
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Fronteras visibles · Fase 2").click();
    h.run_steps(4);
    h.get_by_label("Detectar fronteras").click();
    h.run_steps(4);
    h.get_by_label_contains("Zona 1 ·").click();
    h.run_steps(4);
    h.get_by_label("Verificar curvas del par 1").click();
    h.run_steps(4);
    h.get_by_label("Previsualizar franja").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    h.get_by_label("Franja temporal · 0.10 mm");
    h.get_by_label("Exportar diagnóstico (.json)").click();
    h.run_steps(4);
    let first: serde_json::Value = serde_json::from_slice(&files.borrow()[0].1).unwrap();
    assert_eq!(first["analyses"]["preview"]["status"], "current");
    assert_eq!(first["localPreviewGeometryIncluded"], true);
    let report = &first["analyses"]["preview"]["report"];
    assert!(report["bandPath"].as_str().unwrap().len() > 10);
    assert!(report.get("dataBase64").is_none());
    assert!(report.get("overlayBase64").is_none());
    let before_ids = report["ids"].as_array().unwrap().clone();
    h.get_by_label("Invertir origen y receptor").click();
    h.run_steps(4);
    assert!(h.query_by_label("Franja temporal · 0.10 mm").is_none());
    h.get_by_label("Previsualizar franja").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.failed, "{}", h.state().ui.trapping.message);
    h.get_by_label("Mostrar franja resaltada").click();
    h.run_steps(4);
    assert!(!h.state().ui.trapping.preview_show);
    h.get_by_label("Exportar diagnóstico (.json)").click();
    h.run_steps(4);
    let second: serde_json::Value = serde_json::from_slice(&files.borrow()[1].1).unwrap();
    let after_ids = second["analyses"]["preview"]["report"]["ids"].as_array().unwrap();
    assert_eq!(after_ids, &before_ids.into_iter().rev().collect::<Vec<_>>());
    assert!(std::sync::Arc::ptr_eq(&doc, &h.state().session.doc().unwrap().doc));
    assert_eq!(selected, h.state().session.doc().unwrap().selection.objects);
    assert_eq!(undo, h.state().session.doc().unwrap().history.undo.len());
    assert_eq!(redo, h.state().session.doc().unwrap().history.redo.len());
    // Direct export must detect stale controls before any UI frame runs.
    h.state_mut().ui.trapping.preview_width_mm = 0.2;
    h.state_mut().run("trapping.exportDiagnostic", json!({})).unwrap();
    let stale: serde_json::Value = serde_json::from_slice(&files.borrow()[2].1).unwrap();
    assert_eq!(stale["analyses"]["preview"]["status"], "stale");
    assert!(stale["analyses"]["preview"]["report"].is_null());
    h.run_steps(4);
    h.get_by_label("Previsualizar franja").click();
    h.run_steps(4);
    h.get_by_label("Franja temporal · 0.20 mm");
    h.state_mut().run("shape.rectangle", json!({"x":5,"y":5,"width":5,"height":5})).unwrap();
    h.state_mut().run("trapping.exportDiagnostic", json!({})).unwrap();
    let stale: serde_json::Value = serde_json::from_slice(&files.borrow()[3].1).unwrap();
    assert_eq!(stale["analyses"]["preview"]["status"], "stale");
    h.run_steps(4);
    assert!(h.query_by_label("Previsualizar franja").is_none());
}
