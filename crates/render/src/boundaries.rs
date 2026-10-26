//! Experimental visible-ink boundary candidates, not trap decisions or vector outlines.
//! Compare aligned plate samples, never RGB display colours or hidden source paths.

use crate::Rendered;

pub const MAX_PIXELS: usize = 1_000_000;
pub const MAX_PLATES: usize = 12;
const MAX_ZONES: usize = 200;

pub struct InkPlane {
    pub name: String,
    /// Ink coverage, one byte per pixel (0 = paper).
    pub coverage: Vec<u8>,
}

#[derive(Debug)]
pub struct Zone {
    pub pixels: usize,
    /// Pixel rectangle, right and bottom exclusive. Not a source object or a trap width.
    pub bounds: [u32; 4],
    pub plates: Vec<String>,
}

pub struct Analysis {
    pub overlay: Rendered,
    pub zones: Vec<Zone>,
    pub zone_count: usize,
    pub marked_pixels: usize,
    pub discarded_pixels: usize,
}

/// A sharp transition must trade inks: at least one plate increases and another
/// decreases by `contrast`. Two central differences must dominate the two outer
/// differences, rejecting a smooth ramp. Three inked inner samples prevent joining
/// two colours across a resolved white gap. Antialiasing and subpixel gaps remain
/// sampling limitations, not evidence of a physical contact.
fn exchange(planes: &[InkPlane], positions: [usize; 5], contrast: i16) -> u16 {
    let has_ink = |pos| planes.iter().any(|p| p.coverage.get(pos).is_some_and(|v| *v >= 4));
    if !has_ink(positions[1]) || !has_ink(positions[2]) || !has_ink(positions[3]) {
        return 0;
    }
    let (mut up, mut down, mut bits) = (false, false, 0u16);
    for (i, plane) in planes.iter().enumerate() {
        let [a, b, c, d, e] = positions.map(|pos| i16::from(plane.coverage.get(pos).copied().unwrap_or(0)));
        let delta = e - a;
        let inner = (c - b).abs() + (d - c).abs();
        let outer = (b - a).abs() + (e - d).abs();
        if delta.abs() >= contrast && inner >= 2 * outer {
            up |= delta > 0;
            down |= delta < 0;
            bits |= 1u16 << i;
        }
    }
    if up && down { bits } else { 0 }
}

/// Detect connected candidate pixels. The overlay includes all retained candidates;
/// only the first 200 connected zones, in scan order, have a detailed report.
pub fn detect(width: u32, height: u32, planes: &[InkPlane], contrast: f64, min_pixels: usize) -> Result<Analysis, String> {
    let count = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || count > MAX_PIXELS as u64 {
        return Err("El detector admite hasta 1 millón de píxeles. Baja los ppp o analiza una región menor.".into());
    }
    if !(2..=MAX_PLATES).contains(&planes.len()) {
        return Err("El detector requiere entre 2 y 12 planchas alineadas.".into());
    }
    if !contrast.is_finite() || !(5.0..=100.0).contains(&contrast) || !(2..=1000).contains(&min_pixels) {
        return Err("Contraste debe estar entre 5 y 100 %, y minPixels entre 2 y 1000.".into());
    }
    let n = count as usize;
    for (i, p) in planes.iter().enumerate() {
        if p.coverage.len() != n || p.name.is_empty() || planes.iter().take(i).any(|q| q.name == p.name) {
            return Err("Cada plancha debe tener un nombre único y exactamente un valor por píxel.".into());
        }
    }
    let (w, h) = (width as usize, height as usize);
    let threshold = (contrast * 255.0 / 100.0).round() as i16;
    let mut changes = vec![0u16; n];
    let mut mask = vec![0u8; n];
    for y in 2..h.saturating_sub(2) {
        for x in 2..w.saturating_sub(2) {
            let i = y * w + x;
            let horizontal = exchange(planes, [i - 2, i - 1, i, i + 1, i + 2], threshold);
            let vertical = exchange(planes, [i - 2 * w, i - w, i, i + w, i + 2 * w], threshold);
            let bits = horizontal | vertical;
            if let (Some(m), Some(c)) = (mask.get_mut(i), changes.get_mut(i)) {
                *m = u8::from(bits != 0);
                *c = bits;
            }
        }
    }
    let (mut zones, mut zone_count, mut marked_pixels, mut discarded_pixels) = (Vec::new(), 0, 0, 0);
    let mut component = Vec::new();
    for start in 0..n {
        if mask.get(start) != Some(&1) {
            continue;
        }
        component.clear();
        component.push(start);
        if let Some(m) = mask.get_mut(start) {
            *m = 2;
        }
        let (mut cursor, mut bits) = (0, 0u16);
        let mut bounds = [width, height, 0, 0];
        while let Some(&i) = component.get(cursor) {
            cursor += 1;
            let (x, y) = (i % w, i / w);
            bounds[0] = bounds[0].min(x as u32);
            bounds[1] = bounds[1].min(y as u32);
            bounds[2] = bounds[2].max(x as u32 + 1);
            bounds[3] = bounds[3].max(y as u32 + 1);
            bits |= changes.get(i).copied().unwrap_or(0);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && nx < w as i64 && ny >= 0 && ny < h as i64 {
                        let next = ny as usize * w + nx as usize;
                        if let Some(m) = mask.get_mut(next)
                            && *m == 1
                        {
                            *m = 2;
                            component.push(next);
                        }
                    }
                }
            }
        }
        if component.len() < min_pixels {
            discarded_pixels += component.len();
            continue;
        }
        zone_count += 1;
        marked_pixels += component.len();
        for &i in &component {
            if let Some(m) = mask.get_mut(i) {
                *m = 3;
            }
        }
        if zones.len() < MAX_ZONES {
            let plates = planes.iter().enumerate().filter(|(i, _)| bits & (1u16 << i) != 0).map(|(_, p)| p.name.clone()).collect();
            zones.push(Zone { pixels: component.len(), bounds, plates });
        }
    }
    let mut pixels = vec![0; n * 4];
    for i in 0..n {
        let marked = mask.get(i) == Some(&3);
        let (x, y) = (i % w, i / w);
        let halo = !marked
            && [
                (x.checked_sub(1), Some(y)),
                (Some(x + 1).filter(|x| *x < w), Some(y)),
                (Some(x), y.checked_sub(1)),
                (Some(x), Some(y + 1).filter(|y| *y < h)),
            ]
            .iter()
            .any(|(x, y)| x.zip(*y).is_some_and(|(x, y)| mask.get(y * w + x) == Some(&3)));
        if let Some(px) = pixels.get_mut(i * 4..i * 4 + 4) {
            // Premultiplied RGBA; white halo keeps red marks visible over dark inks.
            px.copy_from_slice(if marked {
                &[255, 32, 32, 255]
            } else if halo {
                &[180, 180, 180, 180]
            } else {
                &[0; 4]
            });
        }
    }
    Ok(Analysis { overlay: Rendered { width, height, pixels }, zones, zone_count, marked_pixels, discarded_pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planes(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 2]) -> Vec<InkPlane> {
        let mut out = vec![InkPlane { name: "Ink A".into(), coverage: vec![] }, InkPlane { name: "Ink B".into(), coverage: vec![] }];
        for y in 0..h {
            for x in 0..w {
                let values = f(x, y);
                for (p, v) in out.iter_mut().zip(values) {
                    p.coverage.push(v);
                }
            }
        }
        out
    }

    #[test]
    fn a_sharp_exchange_is_one_zone_and_preserves_ink_identity() {
        let p = planes(40, 24, |x, _| if x < 20 { [255, 0] } else { [0, 255] });
        let a = detect(40, 24, &p, 15.0, 4).unwrap();
        assert_eq!(a.zone_count, 1);
        assert_eq!(a.zones[0].plates, vec!["Ink A", "Ink B"]);
        assert_eq!(a.zones[0].bounds, [19, 2, 21, 22]);
        assert_eq!(a.marked_pixels, 40);
    }

    #[test]
    fn same_ink_ramps_white_edges_and_resolved_gaps_are_not_candidates() {
        for p in [
            planes(40, 24, |_, _| [180, 0]),
            planes(40, 24, |x, _| [255 - x as u8 * 6, x as u8 * 6]),
            planes(40, 24, |x, _| if x < 20 { [255, 0] } else { [0, 0] }),
            planes(40, 24, |x, _| {
                if x < 19 {
                    [255, 0]
                } else if x == 19 {
                    [0, 0]
                } else {
                    [0, 255]
                }
            }),
            planes(40, 24, |x, _| if x < 20 { [255, 0] } else { [255, 255] }),
        ] {
            assert_eq!(detect(40, 24, &p, 15.0, 4).unwrap().zone_count, 0);
        }
    }

    #[test]
    fn antialiased_exchange_is_retained_but_small_fragments_are_filtered() {
        let p = planes(40, 24, |x, _| {
            if x < 20 {
                [255, 0]
            } else if x == 20 {
                [128, 128]
            } else {
                [0, 255]
            }
        });
        let a = detect(40, 24, &p, 15.0, 4).unwrap();
        assert_eq!(a.zone_count, 1);
        assert!(a.marked_pixels >= 20);
        let b = detect(40, 24, &p, 15.0, 1000).unwrap();
        assert_eq!(b.zone_count, 0);
        assert!(b.discarded_pixels > 0);
    }

    #[test]
    fn bad_dimensions_lengths_duplicates_and_settings_fail() {
        let p = planes(10, 10, |_, _| [255, 0]);
        assert!(detect(0, 10, &p, 15.0, 4).is_err());
        assert!(detect(2000, 2000, &p, 15.0, 4).is_err());
        assert!(detect(9, 10, &p, 15.0, 4).is_err());
        assert!(detect(10, 10, &p, f64::NAN, 4).is_err());
        assert!(detect(10, 10, &p, 15.0, 0).is_err());
        let mut duplicate = p;
        duplicate[1].name = "Ink A".into();
        assert!(detect(10, 10, &duplicate, 15.0, 4).is_err());
    }

    #[test]
    fn all_marks_survive_a_bounded_zone_report() {
        let p = planes(400, 400, |x, y| {
            let (x, y) = (x % 20, y % 20);
            if x >= 14 || y >= 14 {
                [0, 0]
            } else if x < 7 {
                [255, 0]
            } else {
                [0, 255]
            }
        });
        let a = detect(400, 400, &p, 15.0, 4).unwrap();
        assert_eq!(a.zone_count, 400);
        assert_eq!(a.zones.len(), 200);
        assert!(a.marked_pixels > a.zones.iter().map(|z| z.pixels).sum::<usize>());
        let bottom = (386 * 400 + 386) * 4;
        assert_eq!(&a.overlay.pixels[bottom..bottom + 4], &[255, 32, 32, 255]);
    }
}
