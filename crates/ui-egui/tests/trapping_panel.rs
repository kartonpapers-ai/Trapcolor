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
        if std::env::var("KX27_BOUNDARY_CAPTURE").is_ok() {
            if let Ok(path) = std::env::var("KX27_BOUNDARY_CAPTURE_SVG") {
                let bytes = std::fs::read(path).unwrap();
                app.run("document.open", json!({"name":"user-job.svg","dataBase64":vectorcraft_format::base64_encode(&bytes)})).unwrap();
            }
        } else {
            app.run("trapping.apply", json!({})).unwrap();
        }
        let mut ready = false;
        let mut h = Harness::builder().with_size(egui::vec2(width, 1050.0)).wgpu().build_ui_state(
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
        h.render().unwrap().save(std::path::Path::new(&dir).join(format!("trapping-{}-{width}.png", theme.id()))).unwrap();
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
