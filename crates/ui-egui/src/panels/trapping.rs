//! Persistent Trap Color palette. No modal and no dependency on an installed WASM plug-in.
//! Geometry is calculated only on Analyze or Apply; ordinary frames read a snapshot.

use egui::Ui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use vectorcraft_doc::Document;

use crate::theme::Tokens;
use crate::{VectorcraftApp, widgets};

pub const ID: &str = "trapping";
const DIRECTIONS: [&str; 3] = ["Claro hacia oscuro", "Invertida", "Centrada"];
const DIRECTION_IDS: [&str; 3] = ["auto", "reverse", "center"];
const BLACK: [&str; 2] = ["Sobreimprimir negro", "Aplicar trapping"];

#[derive(Debug)]
struct Analysis {
    document: Arc<Document>,
    params: Value,
    report: Value,
}

struct Capture {
    document: Arc<Document>,
    colors: vectorcraft_color::cms::ColorSettings,
    plate: String,
    dpi: f64,
    artboard: usize,
    report: Value,
    texture: egui::TextureHandle,
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture").field("plate", &self.plate).field("dpi", &self.dpi).field("artboard", &self.artboard).finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub width_mm: f64,
    pub direction: usize,
    pub black: usize,
    pub difference: f64,
    #[serde(skip)]
    pub message: String,
    #[serde(skip)]
    pub failed: bool,
    #[serde(skip)]
    document: Option<u64>,
    #[serde(skip)]
    analysis: Option<Arc<Analysis>>,
    #[serde(skip)]
    analysis_page: usize,
    pub capture_dpi: f64,
    pub capture_plate: usize,
    pub capture_zoom: f32,
    #[serde(skip)]
    capture: Option<Arc<Capture>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            width_mm: 0.1,
            direction: 0,
            black: 0,
            difference: 10.0,
            message: String::new(),
            failed: false,
            document: None,
            analysis: None,
            analysis_page: 0,
            capture_dpi: 150.0,
            capture_plate: 0,
            capture_zoom: 1.0,
            capture: None,
        }
    }
}

impl Settings {
    fn sanitize(&mut self) {
        if !self.width_mm.is_finite() {
            self.width_mm = 0.1;
        }
        if !self.difference.is_finite() {
            self.difference = 10.0;
        }
        self.width_mm = self.width_mm.clamp(0.01, 2.0);
        self.difference = self.difference.clamp(0.0, 100.0);
        self.direction = self.direction.min(2);
        self.black = self.black.min(1);
        if !self.capture_dpi.is_finite() {
            self.capture_dpi = 150.0;
        }
        self.capture_dpi = self.capture_dpi.clamp(36.0, 600.0);
        if !self.capture_zoom.is_finite() {
            self.capture_zoom = 1.0;
        }
        self.capture_zoom = self.capture_zoom.clamp(1.0, 8.0);
    }

    fn params(&self) -> Value {
        json!({
            "widthMm": self.width_mm,
            "direction": DIRECTION_IDS.get(self.direction).unwrap_or(&"auto"),
            "black": if self.black == 1 { "trap" } else { "overprint" },
            "difference": self.difference,
        })
    }
}

#[derive(Clone)]
struct PairEdit {
    width: f64,
    mode: usize,
}

fn number(ui: &mut Ui, label: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        let label = ui.label(label);
        ui.add(egui::DragValue::new(value).speed(0.01).range(range).suffix(suffix).max_decimals(2)).labelled_by(label.id);
    });
}

fn apply(app: &mut VectorcraftApp, settings: &mut Settings, params: Value) {
    settings.analysis = None;
    match app.run("trapping.apply", params) {
        Ok(v) => {
            let count = v.get("pairs").and_then(Value::as_array).map_or(0, Vec::len);
            settings.message = format!("{count} pares actualizados. Puedes deshacer la operación.");
            settings.failed = false;
        }
        Err(e) => {
            settings.message = e;
            settings.failed = true;
        }
    }
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    let mut settings = app.ui.trapping.clone();
    settings.sanitize();
    let doc_uid = app.session.active().map(|st| st.uid);
    if settings.document != doc_uid {
        clear_capture(ui, &mut settings);
        settings.analysis = None;
        settings.message.clear();
        settings.failed = false;
        settings.document = doc_uid;
    }
    let snapshot = vectorcraft_engine::cmd::trapping::snapshot(&app.session).ok();
    let artwork = snapshot.as_ref().and_then(|v| v["artwork"].as_u64()).unwrap_or(0);
    let pairs = snapshot.as_ref().and_then(|v| v["pairs"].as_array()).cloned().unwrap_or_default();
    let ids = snapshot.as_ref().map(|v| v["ids"].clone()).unwrap_or_else(|| json!([]));
    let can_apply = doc_uid.is_some() && artwork >= 2;

    capture_section(app, ui, &mut settings);

    widgets::section_header(ui, "Reglas para pares nuevos");
    ui.add_enabled_ui(doc_uid.is_some(), |ui| {
        number(ui, "Ancho", &mut settings.width_mm, 0.01..=2.0, " mm");
        ui.horizontal(|ui| {
            ui.label("Dirección");
            if let Some(i) = widgets::dropdown(
                ui,
                "trap-direction",
                DIRECTIONS.get(settings.direction).unwrap_or(&DIRECTIONS[0]),
                &DIRECTIONS,
                ui.available_width().clamp(90.0, 165.0),
            ) {
                settings.direction = i;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Negro K");
            if let Some(i) =
                widgets::dropdown(ui, "trap-black", BLACK.get(settings.black).unwrap_or(&BLACK[0]), &BLACK, ui.available_width().clamp(90.0, 165.0))
            {
                settings.black = i;
            }
        });
        number(ui, "Diferencia mínima", &mut settings.difference, 0.0..=100.0, " %");
    });
    if snapshot.as_ref().is_some_and(|v| v["gradients"].as_u64().unwrap_or(0) > 0) {
        ui.label("Sólido + degradado lineal: dirección fija del sólido hacia el degradado, en sobreimpresión.");
    }
    if snapshot.as_ref().is_some_and(|v| v["images"].as_u64().unwrap_or(0) > 0) {
        ui.label("Sólido + imagen opaca: sólido hacia el marco de la imagen, sin analizar los colores de sus píxeles.");
    }
    let stale = settings
        .analysis
        .as_ref()
        .is_some_and(|a| a.params != settings.params() || app.session.active().is_none_or(|st| !Arc::ptr_eq(&a.document, &st.doc)));
    if stale {
        settings.analysis = None;
        settings.message = "El documento o las reglas cambiaron. Analiza de nuevo.".into();
        settings.failed = false;
    }
    ui.add_space(8.0);
    if ui.add_enabled(can_apply, egui::Button::new("Analizar contactos").min_size(egui::vec2(ui.available_width(), 28.0))).clicked() {
        let params = settings.params();
        let mut p = params.clone();
        p["ids"] = ids.clone();
        settings.analysis = None;
        match app.run("trapping.analyze", p) {
            Ok(report) => {
                if let Some(st) = app.session.active() {
                    settings.analysis = Some(Arc::new(Analysis { document: st.doc.clone(), params, report }));
                    settings.analysis_page = 0;
                    settings.message = "Análisis listo. El documento no se modificó.".into();
                    settings.failed = false;
                }
            }
            Err(e) => {
                settings.message = e;
                settings.failed = true;
            }
        }
    }
    let reviewed_scope = settings.analysis.as_ref().is_none_or(|a| a.report["ids"] == ids);
    let has_contacts = settings.analysis.as_ref().is_none_or(|a| a.report["encounters"].as_array().is_some_and(|v| !v.is_empty()));
    if !reviewed_scope {
        ui.label("Recupera la selección analizada para aplicar sus reglas.");
        if ui.button("Recuperar selección analizada").clicked()
            && let Some(a) = &settings.analysis
            && let Err(e) = app.run("select.set", json!({"ids": a.report["ids"]}))
        {
            settings.message = e;
            settings.failed = true;
        }
    }
    ui.add_enabled_ui(can_apply && reviewed_scope && has_contacts, |ui| {
        if ui
            .add_sized(
                [ui.available_width(), 28.0],
                egui::Button::new(egui::RichText::new("Aplicar a la selección").strong().color(tokens.text_strong))
                    .fill(tokens.input)
                    .stroke(egui::Stroke::new(1.0, tokens.accent)),
            )
            .clicked()
        {
            let mut p = settings.params();
            p["ids"] = ids.clone();
            apply(app, &mut settings, p);
        }
        if !pairs.is_empty()
            && ui.button("Restablecer pares").on_hover_text("Recalcula los pares seleccionados y reemplaza sus ajustes individuales.").clicked()
        {
            let mut p = settings.params();
            p["ids"] = ids.clone();
            p["resetExisting"] = json!(true);
            apply(app, &mut settings, p);
        }
    });
    if !can_apply {
        ui.label(if doc_uid.is_none() {
            "Abre un documento para comenzar."
        } else {
            "Selecciona dos trazados compatibles. Para recalcular, incluye también sus traps."
        });
    } else {
        widgets::dim_label(ui, &format!("{artwork} objetos en la selección"));
    }
    if !settings.message.is_empty() {
        let color = if settings.failed { tokens.error } else { tokens.text };
        ui.label(egui::RichText::new(&settings.message).color(color));
    }
    widgets::divider(ui);
    if let Some(a) = settings.analysis.clone() {
        widgets::section_header(ui, "Análisis de contactos");
        let encounters = a.report["encounters"].as_array().map(Vec::as_slice).unwrap_or_default();
        ui.label(if encounters.len() == 1 { "1 encuentro previsto".into() } else { format!("{} encuentros previstos", encounters.len()) });
        if encounters.is_empty() {
            ui.label("No se prevén encuentros con este ancho y selección.");
        } else {
            let pages = encounters.len().div_ceil(20);
            settings.analysis_page = settings.analysis_page.min(pages.saturating_sub(1));
            if pages > 1 {
                ui.horizontal(|ui| {
                    if ui.add_enabled(settings.analysis_page > 0, egui::Button::new("Anterior")).clicked() {
                        settings.analysis_page = settings.analysis_page.saturating_sub(1);
                    }
                    ui.label(format!("{} / {pages}", settings.analysis_page + 1));
                    if ui.add_enabled(settings.analysis_page + 1 < pages, egui::Button::new("Siguiente")).clicked() {
                        settings.analysis_page += 1;
                    }
                });
            }
            egui::ScrollArea::vertical().id_salt("trap-analysis").max_height(230.0).show(ui, |ui| {
                for (index, encounter) in encounters.iter().enumerate().skip(settings.analysis_page * 20).take(20) {
                    ui.push_id(("encounter", index), |ui| {
                        let left = encounter["left"].as_str().unwrap_or_default();
                        let right = encounter["right"].as_str().unwrap_or_default();
                        let relation = encounter["relation"].as_str().unwrap_or_default();
                        let action = match relation {
                            "-" => "Sin trap".into(),
                            "=" => format!("Centrado: {left} / {right}"),
                            "S" => format!("Sobreimprimir: {left} sobre {right}"),
                            _ => format!("{left} hacia {right}"),
                        };
                        ui.strong(action);
                        ui.label(format!(
                            "{} / {}",
                            encounter["bottomName"].as_str().unwrap_or("Trazado"),
                            encounter["topName"].as_str().unwrap_or("Trazado")
                        ));
                        let reason = match encounter["reason"].as_str() {
                            Some("solidIntoImage") => "Regla fija: sólido hacia imagen opaca",
                            Some("solidIntoGradient") => "Regla fija: sólido hacia degradado",
                            Some("savedRule") => "Ajuste del par existente",
                            Some("sharedInk") => "Diferencia de tinta inferior al mínimo",
                            Some("blackOverprint") => "Regla de sobreimpresión de negro K",
                            Some("center") => "Dirección centrada",
                            Some("reverse") => "Dirección invertida",
                            _ => "Color claro hacia oscuro",
                        };
                        ui.label(format!("{:.2} mm · {reason}", encounter["widthMm"].as_f64().unwrap_or_default()));
                        if ui.button("Seleccionar pareja").clicked()
                            && let Err(e) = app.run("select.set", json!({"ids": [encounter["bottomId"], encounter["topId"]]}))
                        {
                            settings.message = e;
                            settings.failed = true;
                        }
                        widgets::divider(ui);
                    });
                }
            });
        }
        widgets::dim_label(ui, "Previsión según el ancho, las reglas y el arte seleccionado. Cada ajuste afecta a los encuentros del par mostrado.");
        widgets::divider(ui);
    }
    widgets::section_header(ui, "Traps de la selección");
    if pairs.is_empty() {
        ui.label("Aplica trapping para ver y ajustar las parejas de tintas.");
    }
    for pair in &pairs {
        let Some(id) = pair["id"].as_u64() else { continue };
        let name = pair["name"].as_str().unwrap_or("Trap");
        let key = ui.id().with(("trap-pair", doc_uid, id, name, pair["widthMm"].as_f64().unwrap_or_default().to_bits(), pair["relation"].as_str()));
        let gradient = pair["solidIntoGradient"].as_bool().unwrap_or(false) || pair["solidIntoImage"].as_bool().unwrap_or(false);
        let initial = PairEdit {
            width: pair["widthMm"].as_f64().filter(|v| *v >= 0.01).unwrap_or(settings.width_mm),
            mode: if gradient {
                usize::from(pair["relation"] == "-")
            } else {
                match pair["relation"].as_str() {
                    Some("=") => 2,
                    Some("-") => 3,
                    Some("S") => 4,
                    _ => 0,
                }
            },
        };
        let mut edit = ui.ctx().data(|d| d.get_temp::<PairEdit>(key)).unwrap_or(initial);
        let left = pair["left"].as_str().unwrap_or("");
        let right = pair["right"].as_str().unwrap_or("");
        let has_black = pair["blackLeft"].as_bool().unwrap_or(false) || pair["blackRight"].as_bool().unwrap_or(false) || pair["relation"] == "S";
        let forward = format!("{left} hacia {right}");
        let reverse = format!("{right} hacia {left}");
        let modes = [forward.as_str(), reverse.as_str(), "Centrado", "Sin trap", "Sobreimprimir negro K"];
        let gradient_modes = [forward.as_str(), "Sin trap"];
        let modes = if gradient {
            &gradient_modes[..]
        } else if has_black {
            &modes[..]
        } else {
            &modes[..4]
        };
        edit.mode = edit.mode.min(modes.len().saturating_sub(1));
        ui.push_id(id, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("{left} / {right}"));
                if ui
                    .button("Localizar")
                    .on_hover_text("Selecciona el trap en el lienzo. Vuelve a seleccionar el arte y sus traps para recalcular.")
                    .clicked()
                    && let Err(e) = app.run("select.set", json!({"ids": [id]}))
                {
                    settings.message = e;
                    settings.failed = true;
                }
            });
            number(ui, "Ancho del par", &mut edit.width, 0.01..=2.0, " mm");
            if let Some(i) =
                widgets::dropdown(ui, ("trap-mode", id), modes.get(edit.mode).unwrap_or(&modes[0]), modes, ui.available_width().min(245.0))
            {
                edit.mode = i;
            }
            ui.add_enabled_ui(can_apply, |ui| {
                if ui.button("Actualizar par").clicked() {
                    let mut p = settings.params();
                    p["ids"] = ids.clone();
                    let modes = if gradient { &["forward", "none"][..] } else { &["forward", "reverse", "center", "none", "overprint"][..] };
                    let mode = modes.get(edit.mode).copied().unwrap_or("forward");
                    p["pair"] = json!({"id": id, "widthMm": edit.width, "mode": mode});
                    apply(app, &mut settings, p);
                }
            });
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, edit));
        widgets::divider(ui);
    }
    ui.collapsing("Alcance de esta versión", |ui| {
        ui.label("Sólidos CMYK y spot; también sólido contra degradado lineal opaco de proceso. RGB usa conversión simple a CMYK.");
        ui.label("Los traps conservan las tintas y sobreimprimen. Para ver la aproximación en pantalla, activa View → Overprint Preview. Admite sólido hacia el marco de PNG/JPEG opacos incrustados. No admite imágenes vinculadas, transparencias, recortes, radiales, imagen contra imagen/degradado ni tintas técnicas.");
        ui.label("Los traps existentes conservan sus ajustes. Selecciona todo el arte que pueda tapar un encuentro. Máximo: 80 objetos y 12.000 nodos por operación.");
    });
    app.ui.trapping = settings;
}

fn clear_capture(_ui: &Ui, settings: &mut Settings) {
    // Dropping the last handle frees the old texture, with no image-loader cache.
    settings.capture = None;
}

fn capture_section(app: &mut VectorcraftApp, ui: &mut Ui, settings: &mut Settings) {
    let artboard = app.session.active_index().and_then(|i| app.views.get(i)).map_or(0, |v| v.artboard);
    let plates = app.session.active().map(|st| vectorcraft_doc::inks::plates(&st.doc)).unwrap_or_default();
    settings.capture_plate = settings.capture_plate.min(plates.len().saturating_sub(1));
    let plate = plates.get(settings.capture_plate).map(|p| p.name.clone()).unwrap_or_default();
    let stale = settings.capture.as_ref().is_some_and(|c| {
        c.plate != plate
            || c.dpi != settings.capture_dpi
            || c.artboard != artboard
            || c.colors != vectorcraft_color::cms::active_settings()
            || app.session.active().is_none_or(|st| !Arc::ptr_eq(&c.document, &st.doc))
    });
    if stale {
        clear_capture(ui, settings);
    }
    egui::CollapsingHeader::new("Captura por tintas · Fase 1").show(ui, |ui| {
        ui.label("Mesa completa, sin marcas de selección. No modifica el documento ni aplica trapping.");
        ui.add_enabled_ui(!plates.is_empty(), |ui| {
            let labels: Vec<&str> = plates.iter().map(|p| p.name.as_str()).collect();
            if let Some(i) = widgets::dropdown_names(ui, "capture-plate", &plate, &labels, ui.available_width().min(220.0)) {
                settings.capture_plate = i;
                clear_capture(ui, settings);
            }
            let old_dpi = settings.capture_dpi;
            number(ui, "Resolución", &mut settings.capture_dpi, 36.0..=600.0, " ppp");
            if old_dpi != settings.capture_dpi {
                clear_capture(ui, settings);
            }
            if ui.button("Capturar tinta").clicked() {
                clear_capture(ui, settings);
                let plate = plates.get(settings.capture_plate).map(|p| p.name.clone()).unwrap_or_default();
                match app.run("trapping.capture", json!({"plate":plate,"dpi":settings.capture_dpi,"artboard":artboard})) {
                    Ok(mut report) => {
                        let pixels = report["dataBase64"]
                            .as_str()
                            .and_then(vectorcraft_format::base64_decode)
                            .and_then(|bytes| image::load_from_memory(&bytes).ok())
                            .map(|image| image.to_rgba8());
                        if let (Some(pixels), Some(st)) = (pixels, app.session.active()) {
                            // Upload once on explicit capture. This build installs SVG loaders
                            // only, so PNGs use the same direct texture path as other panels.
                            let color =
                                egui::ColorImage::from_rgba_unmultiplied([pixels.width() as usize, pixels.height() as usize], pixels.as_raw());
                            let texture = ui.ctx().load_texture("kx27-technical-capture", color, egui::TextureOptions::NEAREST);
                            if let Some(obj) = report.as_object_mut() {
                                obj.remove("dataBase64");
                            }
                            settings.capture = Some(Arc::new(Capture {
                                document: st.doc.clone(),
                                colors: vectorcraft_color::cms::active_settings(),
                                plate: plate.clone(),
                                dpi: settings.capture_dpi,
                                artboard,
                                report,
                                texture,
                            }));
                            settings.message = "Captura experimental lista. El documento no se modificó.".into();
                            settings.failed = false;
                        } else {
                            settings.message = "No se pudo leer la captura.".into();
                            settings.failed = true;
                        }
                    }
                    Err(e) => {
                        settings.message = e;
                        settings.failed = true;
                    }
                }
            }
        });
        widgets::dim_label(
            ui,
            "Experimental · Negro = tinta; blanco = sin tinta. Sobreimpresión aproximada. No es una prueba RIP ni detecta conflictos.",
        );
        if let Some(c) = &settings.capture {
            ui.label(format!(
                "{} · {} × {} px · {:.3} mm/píxel",
                c.plate,
                c.report["width"],
                c.report["height"],
                c.report["pixelMm"].as_f64().unwrap_or(0.0)
            ));
            ui.add(egui::Slider::new(&mut settings.capture_zoom, 1.0..=8.0).text("Ampliación"));
            let width = ui.available_width().max(1.0) * settings.capture_zoom;
            let ratio = c.report["height"].as_f64().unwrap_or(1.0) / c.report["width"].as_f64().unwrap_or(1.0).max(1.0);
            egui::ScrollArea::both().id_salt("capture-image").max_height(280.0).show(ui, |ui| {
                ui.add(egui::Image::new(&c.texture).fit_to_exact_size(egui::vec2(width, width * ratio as f32)));
            });
            ui.label("Los detalles menores que un píxel pueden perderse. Ampliar esta imagen no aumenta su resolución.");
            if !plates.iter().any(|p| p.spot) {
                ui.label("Sin muestras spot: esta captura no puede recuperar tintas planas convertidas a RGB o CMYK.");
            }
        }
    });
    widgets::divider(ui);
}
