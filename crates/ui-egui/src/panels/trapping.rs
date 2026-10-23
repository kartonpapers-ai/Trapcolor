//! Persistent Trap Color palette. No modal and no dependency on an installed WASM plug-in.
//! Geometry is only calculated on Apply; ordinary frames read a selection snapshot.

use egui::Ui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{VectorcraftApp, widgets};

pub const ID: &str = "trapping";
const DIRECTIONS: [&str; 3] = ["Claro hacia oscuro", "Invertida", "Centrada"];
const DIRECTION_IDS: [&str; 3] = ["auto", "reverse", "center"];
const BLACK: [&str; 2] = ["Simular sobreimpresión", "Aplicar trapping"];

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
}

impl Default for Settings {
    fn default() -> Self {
        Self { width_mm: 0.1, direction: 0, black: 0, difference: 10.0, message: String::new(), failed: false, document: None }
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
        settings.message.clear();
        settings.failed = false;
        settings.document = doc_uid;
    }
    let snapshot = vectorcraft_engine::cmd::trapping::snapshot(&app.session).ok();
    let artwork = snapshot.as_ref().and_then(|v| v["artwork"].as_u64()).unwrap_or(0);
    let pairs = snapshot.as_ref().and_then(|v| v["pairs"].as_array()).cloned().unwrap_or_default();
    let ids = snapshot.as_ref().map(|v| v["ids"].clone()).unwrap_or_else(|| json!([]));
    let can_apply = doc_uid.is_some() && artwork >= 2;

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
    ui.add_space(8.0);
    ui.add_enabled_ui(can_apply, |ui| {
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
            "Selecciona dos trazados de colores distintos. Para recalcular, incluye también sus traps."
        });
    } else {
        widgets::dim_label(ui, &format!("{artwork} trazados en la selección"));
    }
    if !settings.message.is_empty() {
        let color = if settings.failed { tokens.error } else { tokens.text };
        ui.label(egui::RichText::new(&settings.message).color(color));
    }
    widgets::divider(ui);
    widgets::section_header(ui, "Pares de la selección");
    if pairs.is_empty() {
        ui.label("Aplica trapping para ver los pares y ajustar cada encuentro.");
    }
    for pair in &pairs {
        let Some(id) = pair["id"].as_u64() else { continue };
        let name = pair["name"].as_str().unwrap_or("Trap");
        let key = ui.id().with(("trap-pair", doc_uid, id, name));
        let initial = PairEdit {
            width: pair["widthMm"].as_f64().filter(|v| *v >= 0.01).unwrap_or(settings.width_mm),
            mode: match pair["relation"].as_str() {
                Some("=") => 2,
                Some("-") => 3,
                Some("S") => 4,
                _ => 0,
            },
        };
        let mut edit = ui.ctx().data(|d| d.get_temp::<PairEdit>(key)).unwrap_or(initial);
        let left = pair["left"].as_str().unwrap_or("");
        let right = pair["right"].as_str().unwrap_or("");
        let has_black = pair["blackLeft"].as_bool().unwrap_or(false) || pair["blackRight"].as_bool().unwrap_or(false) || pair["relation"] == "S";
        let forward = format!("{left} hacia {right}");
        let reverse = format!("{right} hacia {left}");
        let modes = [forward.as_str(), reverse.as_str(), "Centrado", "Sin trap", "Simular sobreimpresión K"];
        let modes = if has_black { &modes[..] } else { &modes[..4] };
        edit.mode = edit.mode.min(modes.len().saturating_sub(1));
        ui.push_id(id, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("{left} / {right}"));
                if ui
                    .button("Localizar")
                    .on_hover_text("Selecciona el trap en el lienzo. Vuelve a seleccionar el arte y sus traps para recalcular.")
                    .clicked()
                {
                    if let Err(e) = app.run("select.set", json!({"ids": [id]})) {
                        settings.message = e;
                        settings.failed = true;
                    }
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
                    let mode = ["forward", "reverse", "center", "none", "overprint"].get(edit.mode).copied().unwrap_or("forward");
                    p["pair"] = json!({"id": id, "widthMm": edit.width, "mode": mode});
                    apply(app, &mut settings, p);
                }
            });
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, edit));
        widgets::divider(ui);
    }
    ui.collapsing("Alcance de esta versión", |ui| {
        ui.label("Motor steppa-trap 0.4: rellenos planos de proceso, sin contornos ni transparencias. RGB usa conversión simple a CMYK.");
        ui.label("La sobreimpresión se simula combinando CMYK. No admite tintas directas, textos ni imágenes. Las curvas se aproximan con segmentos.");
        ui.label("Los traps existentes conservan sus ajustes. Selecciona todo el arte que pueda tapar un encuentro. Máximo: 80 trazados y 12.000 nodos por operación.");
    });
    app.ui.trapping = settings;
}
