/// Windows taskbar overlays use an RGBA icon instead of a numeric badge API.
pub fn pixels(count: u32) -> Vec<u8> {
    let mut pixels = vec![0; 32 * 32 * 4];
    if count == 0 {
        return pixels;
    }
    for y in 0..32 {
        for x in 0..32 {
            if (x as f32 - 15.5).powi(2) + (y as f32 - 15.5).powi(2) <= 15.5_f32.powi(2) {
                let offset = (y * 32 + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[220, 65, 77, 255]);
            }
        }
    }
    let label = if count > 99 {
        "99+".into()
    } else {
        count.to_string()
    };
    let scale = if label.len() > 2 { 2 } else { 3 };
    let width = (label.len() * 4 - 1) * scale;
    let left = (32 - width) / 2;
    let top = (32 - 5 * scale) / 2;
    for (index, digit) in label.chars().enumerate() {
        let rows = match digit {
            '0' => [7, 5, 5, 5, 7],
            '1' => [2, 6, 2, 2, 7],
            '2' => [7, 1, 7, 4, 7],
            '3' => [7, 1, 7, 1, 7],
            '4' => [5, 5, 7, 1, 1],
            '5' => [7, 4, 7, 1, 7],
            '6' => [7, 4, 7, 5, 7],
            '7' => [7, 1, 1, 1, 1],
            '8' => [7, 5, 7, 5, 7],
            '9' => [7, 5, 7, 1, 7],
            _ => [0, 2, 7, 2, 0],
        };
        for (row, bits) in rows.into_iter().enumerate() {
            for col in 0..3 {
                if bits & (1 << (2 - col)) == 0 {
                    continue;
                }
                for y in 0..scale {
                    for x in 0..scale {
                        let offset =
                            ((top + row * scale + y) * 32 + left + (index * 4 + col) * scale + x)
                                * 4;
                        pixels[offset..offset + 4].copy_from_slice(&[255; 4]);
                    }
                }
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taskbar_badge_clears_and_caps_large_counts_without_overflow() {
        assert!(pixels(0).iter().all(|byte| *byte == 0));
        for count in [1, 9, 10, 99, 100, u32::MAX] {
            let icon = pixels(count);
            assert_eq!(icon.len(), 32 * 32 * 4);
            assert!(icon.chunks_exact(4).any(|pixel| pixel == [255; 4]));
            assert!(
                icon.chunks_exact(4)
                    .any(|pixel| pixel == [220, 65, 77, 255])
            );
        }
        assert_ne!(pixels(1), pixels(2));
        assert_eq!(pixels(100), pixels(u32::MAX));
    }
}
