use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

pub(super) fn card_image(rank: Option<u8>) -> Image {
    const W: usize = 80;
    const H: usize = 112;
    const DIGITS: [[u8; 5]; 10] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
    ];
    let mut pixels = Vec::with_capacity(W * H * 4);
    for y in 0..H {
        for x in 0..W {
            let border = !(2..W - 2).contains(&x) || !(2..H - 2).contains(&y);
            let pixel = if border {
                [210, 183, 127, 255]
            } else if rank.is_some() {
                [246, 231, 199, 255]
            } else if (x + y) % 12 < 6 {
                [31, 72, 61, 255]
            } else {
                [43, 91, 73, 255]
            };
            pixels.extend(pixel);
        }
    }
    if let Some(rank) = rank {
        let digits = rank.to_string();
        let mut stamp = |left: usize, top: usize, scale: usize, reverse: bool| {
            let width = (digits.len() * 4 - 1) * scale;
            for (i, digit) in digits.bytes().enumerate() {
                for (y, bits) in DIGITS[(digit - b'0') as usize].iter().enumerate() {
                    for x in 0..3 {
                        if bits & (4 >> x) == 0 {
                            continue;
                        }
                        for dy in 0..scale {
                            for dx in 0..scale {
                                let px = (i * 4 + x) * scale + dx;
                                let py = y * scale + dy;
                                let px = left + if reverse { width - 1 - px } else { px };
                                let py = top + if reverse { 5 * scale - 1 - py } else { py };
                                let offset = (py * W + px) * 4;
                                pixels[offset..offset + 4].copy_from_slice(&[52, 37, 27, 255]);
                            }
                        }
                    }
                }
            }
        };
        stamp((W - (digits.len() * 4 - 1) * 7) / 2, (H - 35) / 2, 7, false);
        stamp(6, 7, 2, false);
        stamp(W - 6 - (digits.len() * 4 - 1) * 2, 7, 2, false);
        stamp(6, H - 17, 2, true);
        stamp(W - 6 - (digits.len() * 4 - 1) * 2, H - 17, 2, true);
    }
    Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
