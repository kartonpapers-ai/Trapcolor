//! Experimental, read-only plate captures for KX27 surface research.
//!
//! This is a sampled separation preview, not a RIP or a trap generator. It uses the
//! existing renderer's clipping/transparency and approximate overprint compositing.
//! In particular, Multiply is not a full implementation of PDF overprint modes.

use std::io::Cursor;

use vectorcraft_color::Paint;
use vectorcraft_color::cms::{self, PROCESS_PLATES};
use vectorcraft_doc::{Document, ImageBlob, Node, NodeKind};
use vectorcraft_geom::Rect;

use crate::{RenderOptions, Rendered, Renderer, proof, raster_size};

/// Bound the diagnostic separately from full raster exports (especially on WASM).
pub const MAX_CAPTURE_PIXELS: u64 = 4_000_000;
const MAX_IMAGE_PIXELS: u64 = 32_000_000;
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

fn check_art(doc: &Document) -> Result<(), String> {
    fn visit(n: &Node, doc: &Document, mask: bool, depth: usize, count: &mut usize) -> Result<(), String> {
        *count += 1;
        if depth > 128 || *count > 20_000 {
            return Err("La captura admite hasta 20.000 nodos y 128 niveles de anidación.".into());
        }
        if !n.visible
            || matches!(
                n.kind,
                NodeKind::Layer { template: true, .. } | NodeKind::Layer { printable: false, .. } | NodeKind::Path { guide: true, .. }
            )
        {
            return Ok(());
        }
        if !n.appearance.effects.is_empty()
            || n.appearance.items.iter().any(|i| !i.effects().is_empty() || matches!(i.paint(), Paint::Pattern { .. }))
        {
            return Err("La captura inicial no admite efectos ni rellenos de patrón. Expándelos en una copia del trabajo.".into());
        }
        match &n.kind {
            NodeKind::Image(im) => {
                if mask {
                    return Err("La captura inicial no admite imágenes dentro de máscaras de opacidad.".into());
                }
                if !doc.images.contains_key(&im.key) {
                    return Err("Faltan los píxeles de una imagen del documento.".into());
                }
                if im.link.is_some() {
                    return Err("Incrusta las imágenes vinculadas antes de capturar sus tintas.".into());
                }
            }
            NodeKind::Layer { .. }
            | NodeKind::Group { .. }
            | NodeKind::Path { .. }
            | NodeKind::Compound { .. }
            | NodeKind::Text(_)
            | NodeKind::Mesh(_) => {}
            _ => {
                return Err(
                    "La captura inicial requiere expandir símbolos, documentos colocados y otros objetos vivos en una copia del trabajo.".into()
                );
            }
        }
        if let Some(m) = &n.extras().mask {
            visit(&m.art, doc, true, depth + 1, count)?;
        }
        for c in n.children().into_iter().flatten() {
            visit(c, doc, mask, depth + 1, count)?;
        }
        Ok(())
    }
    // `proof::prepare` visits hidden art and unused symbols too. Bound that walk as
    // well, rather than allowing a hidden subtree to bypass the recursion budget.
    fn budget(n: &Node, depth: usize, count: &mut usize) -> Result<(), String> {
        *count += 1;
        if depth > 128 || *count > 20_000 {
            return Err("La captura admite hasta 20.000 nodos y 128 niveles de anidación.".into());
        }
        for child in n.children().into_iter().flatten() {
            budget(child, depth + 1, count)?;
        }
        Ok(())
    }
    let mut count = 0;
    for n in doc.layers.iter().chain(doc.symbols.iter().map(|s| &s.art)) {
        budget(n, 0, &mut count)?;
    }
    let mut count = 0;
    for n in &doc.layers {
        visit(n, doc, false, 0, &mut count)?;
    }
    Ok(())
}

/// Unlike the canvas, a technical capture must ignore non-printing layers and their
/// screen-only outline/dim-image options. Work on the temporary clone only.
fn print_layers(n: &mut std::sync::Arc<Node>) {
    let n = std::sync::Arc::make_mut(n);
    if let NodeKind::Layer { printable, preview, dim_images, .. } = &mut n.kind {
        if !*printable {
            n.visible = false;
        }
        *preview = true;
        *dim_images = None;
    }
    for c in n.children_mut().into_iter().flatten() {
        print_layers(c);
    }
}

/// Validate dimensions without allocating or silently reducing the requested resolution.
pub fn capture_size(region: Rect, dpi: f64) -> Result<(u32, u32), String> {
    if !dpi.is_finite() || !(36.0..=600.0).contains(&dpi) {
        return Err("La resolución debe estar entre 36 y 600 ppp.".into());
    }
    if ![region.x0, region.y0, region.x1, region.y1].iter().all(|v| v.is_finite()) || region.width() <= 0.0 || region.height() <= 0.0 {
        return Err("El área de captura debe ser finita y tener tamaño positivo.".into());
    }
    let (w, h) = raster_size(region, dpi / 72.0)?;
    if u64::from(w) * u64::from(h) > MAX_CAPTURE_PIXELS {
        return Err("La captura supera 4 millones de píxeles. Baja los ppp o captura una región menor.".into());
    }
    Ok((w, h))
}

/// Images must be separated too: the ordinary Separations Preview only recolours vectors.
/// Conversion is temporary; encoded source bytes and alpha remain untouched in the document.
fn image_plate(blob: &ImageBlob, plate: &str, dimensions: (u32, u32)) -> Result<ImageBlob, String> {
    if blob.is_proxy() {
        return Err("La imagen solo tiene una previsualización vinculada. Incrusta el original para capturar sus tintas.".into());
    }
    let process = PROCESS_PLATES.iter().position(|p| *p == plate);
    let rgba = if blob.is_cmyk() {
        let inks = blob.cmyk().ok_or("No se pudieron leer las muestras CMYK de una imagen.")?;
        let mut rgba = image::RgbaImage::new(inks.width, inks.height);
        for (out, px) in rgba.pixels_mut().zip(inks.data.as_chunks::<4>().0) {
            let value = process.and_then(|i| px.get(i)).copied().unwrap_or(0);
            *out = image::Rgba([255 - value, 255 - value, 255 - value, 255]);
        }
        rgba
    } else {
        let mut reader = image::ImageReader::new(Cursor::new(blob.bytes.as_slice())).with_guessed_format().map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let mut rgba = reader.decode().map_err(|e| format!("No se pudo separar una imagen: {e}"))?.to_rgba8();
        let cms = cms::active();
        // A bounded LUT avoids an unbounded map of every distinct pixel colour in a photo.
        let lut = process.map(|i| {
            cms::ProofLut::build(|rgb| {
                let value = 1.0 - cms.srgb_to_cmyk(rgb, cms.settings().intent).get(i).copied().unwrap_or(0.0);
                [value; 3]
            })
        });
        for px in rgba.pixels_mut().filter(|p| p.0[3] > 0) {
            let value = lut.as_ref().map_or(255, |lut| lut.apply8([px.0[0], px.0[1], px.0[2]])[0]);
            px.0[..3].fill(value);
        }
        rgba
    };
    if rgba.dimensions() != dimensions {
        return Err("Las dimensiones de una imagen no coinciden con su cabecera.".into());
    }
    let mut bytes = Vec::new();
    rgba.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(ImageBlob::png(bytes))
}

/// One plate, black = 100% coverage and white = paper. Includes visible locked artwork,
/// all original occluders, clips and alpha. Selection and screen proof state are ignored.
/// Spot identity is read from linked swatches, never inferred from RGB appearance or names.
pub fn capture_plate(renderer: &mut Renderer, doc: &Document, region: Rect, dpi: f64, plate: &str) -> Result<Rendered, String> {
    capture_size(region, dpi)?;
    check_art(doc)?;
    if !proof::plates(doc).iter().any(|p| p.name == plate) {
        return Err("La tinta solicitada no existe en el documento.".into());
    }
    // Check every header and the total budget before decoding any asset. Conservatively
    // include definitions/unused assets too, so nested images can't bypass this budget.
    let (mut pixels, mut bytes) = (0u64, 0usize);
    let mut sizes = Vec::new();
    for (key, blob) in &doc.images {
        bytes = bytes.saturating_add(blob.bytes.len());
        let reader = image::ImageReader::new(Cursor::new(blob.bytes.as_slice())).with_guessed_format().map_err(|e| e.to_string())?;
        let (w, h) = reader.into_dimensions().map_err(|e| format!("No se pudo leer la cabecera de una imagen: {e}"))?;
        if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
            return Err("Una imagen supera 16.384 píxeles por lado o está vacía.".into());
        }
        pixels = pixels.saturating_add(u64::from(w) * u64::from(h));
        if pixels > MAX_IMAGE_PIXELS || bytes > MAX_IMAGE_BYTES {
            return Err("Las imágenes superan el presupuesto de captura: 32 MP decodificados o 64 MiB de archivos.".into());
        }
        sizes.push((key.clone(), (w, h)));
    }
    let mut separated = doc.clone();
    for n in &mut separated.layers {
        print_layers(n);
    }
    for (key, dimensions) in sizes {
        if let Some(blob) = doc.images.get(&key) {
            separated.images.insert(key, image_plate(blob, plate, dimensions)?);
        }
    }
    let opts = RenderOptions {
        background: Some([255; 4]),
        skip_templates: true,
        dim_templates: false,
        precise: true,
        proof: Some(proof::ProofSetup { separations: Some(vec![plate.to_string()]), ..Default::default() }),
        ..Default::default()
    };
    Ok(renderer.render_region_with(&separated, region, dpi / 72.0, &opts))
}

pub struct BoundaryCapture {
    pub composite: Rendered,
    pub analysis: crate::boundaries::Analysis,
    pub plates: Vec<proof::Plate>,
    pub attribution: crate::sources::Attribution,
}

/// Read-only, sampled visible-ink candidates. All plates use the same region and
/// resolution; no document objects, selection, proof view or Apply rules are changed.
pub fn capture_boundaries(
    renderer: &mut Renderer,
    doc: &Document,
    region: Rect,
    dpi: f64,
    contrast: f64,
    min_pixels: usize,
) -> Result<BoundaryCapture, String> {
    use crate::boundaries::{self, InkPlane};
    let (width, height) = capture_size(region, dpi)?;
    if u64::from(width) * u64::from(height) > boundaries::MAX_PIXELS as u64 {
        return Err("El detector admite hasta 1 millón de píxeles. Baja los ppp o analiza una región menor.".into());
    }
    if !contrast.is_finite() || !(5.0..=100.0).contains(&contrast) || !(2..=1000).contains(&min_pixels) {
        return Err("Contraste debe estar entre 5 y 100 %, y minPixels entre 2 y 1000.".into());
    }
    let plates = proof::plates(doc);
    if plates.len() > boundaries::MAX_PLATES {
        return Err("El detector admite hasta 12 tintas, incluidas CMYK y las muestras spot del documento.".into());
    }
    // Phase 2 repeats source decoding per plate. A smaller aggregate source-image
    // budget keeps this synchronous experiment bounded on the browser too.
    let mut source_pixels = 0u64;
    for blob in doc.images.values() {
        let reader = image::ImageReader::new(Cursor::new(blob.bytes.as_slice())).with_guessed_format().map_err(|e| e.to_string())?;
        let (w, h) = reader.into_dimensions().map_err(|e| format!("No se pudo leer una imagen: {e}"))?;
        source_pixels = source_pixels.saturating_add(u64::from(w) * u64::from(h));
        if source_pixels > 8_000_000 {
            return Err("El detector inicial admite hasta 8 MP de imágenes fuente acumulados.".into());
        }
    }
    let mut planes = Vec::new();
    for plate in &plates {
        let image = capture_plate(renderer, doc, region, dpi, &plate.name)?;
        let coverage = image.pixels.as_chunks::<4>().0.iter().map(|p| p[3].saturating_sub(p[0])).collect();
        planes.push(InkPlane { name: plate.name.clone(), coverage });
    }
    let analysis = boundaries::detect(width, height, &planes, contrast, min_pixels)?;
    let mut printable = doc.clone();
    for n in &mut printable.layers {
        print_layers(n);
    }
    let opts = RenderOptions {
        background: Some([255; 4]),
        skip_templates: true,
        dim_templates: false,
        precise: true,
        overprint_preview: true,
        ..Default::default()
    };
    let composite = renderer.render_region_with(&printable, region, dpi / 72.0, &opts);
    let attribution = crate::sources::attribute(renderer, doc, region, dpi, &analysis);
    Ok(BoundaryCapture { composite, analysis, plates, attribution })
}
