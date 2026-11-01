//! Original transfer-bar identity, rasterized at the native icon metric.
pub fn pixels(size: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = (x as f64 + 0.5) * 32.0 / size as f64;
            let v = (y as f64 + 0.5) * 32.0 / size as f64;
            if ((2.0..12.0).contains(&u) && (2.0..24.0).contains(&v))
                || ((20.0..30.0).contains(&u) && (8.0..30.0).contains(&v))
            {
                let i = ((y * size + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[0, 137, 119, 255]);
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_sizes_have_visible_bars_and_transparent_surroundings() {
        for n in [16, 20, 24, 32] {
            let rgba = pixels(n);
            assert_eq!(rgba.len(), (n * n * 4) as usize);
            let alpha = rgba.as_chunks::<4>().0.iter().filter(|p| p[3] != 0).count();
            assert!(alpha > (n * n / 6) as usize);
            assert!(alpha < (n * n / 2) as usize);
            assert_eq!(rgba[3], 0);
        }
    }
}
