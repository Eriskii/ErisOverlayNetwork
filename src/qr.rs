//! Presenting a ticket to a human and a camera.
//!
//! A ticket is long, so the QR code is the thing meant to be used, and the
//! text underneath is the escape hatch for when a camera is not available or
//! a terminal renders blocks badly.

use std::path::{Path, PathBuf};

use qrcode::QrCode;
use qrcode::render::svg;
use qrcode::render::unicode::Dense1x2;

use crate::error::{Error, Result};

fn code(data: &str) -> Result<QrCode> {
    QrCode::new(data).map_err(|e| Error::BadRequest(format!("does not fit in a QR code: {e}")))
}

/// A QR code as text, two module rows per line of output.
///
/// Half-block characters give square modules in a terminal, where cells are
/// about twice as tall as they are wide. A quiet zone of four modules is
/// part of the spec, not decoration: scanners need it to find the symbol.
pub fn to_blocks(data: &str) -> Result<String> {
    let mut out = code(data)?.render::<Dense1x2>().dark_color(Dense1x2::Light).light_color(Dense1x2::Dark).build();
    out.push('\n');
    Ok(out)
}

/// A QR code as a standalone SVG document, dark on light, quiet zone included.
pub fn svg(data: &str) -> Result<String> {
    Ok(code(data)?
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000"))
        .light_color(svg::Color("#fff"))
        .build())
}

/// The QR as a square greyscale raster: dark modules black, everything else
/// white, quiet zone included. `scale` is pixels per module. Returns the
/// side length and the pixels, one byte each.
pub fn raster(data: &str, scale: u32) -> Result<(u32, Vec<u8>)> {
    let code = code(data)?;
    let width = code.width();
    let modules = code.to_colors();
    const QUIET: u32 = 4;
    let side = (width as u32 + QUIET * 2) * scale;

    let mut pixels = vec![255u8; (side * side) as usize];
    for (i, color) in modules.iter().enumerate() {
        if *color != qrcode::Color::Dark {
            continue;
        }
        let mx = (i % width) as u32 + QUIET;
        let my = (i / width) as u32 + QUIET;
        for dy in 0..scale {
            let start = ((my * scale + dy) * side + mx * scale) as usize;
            pixels[start..start + scale as usize].fill(0);
        }
    }
    Ok((side, pixels))
}

/// Write the QR as a greyscale PNG. `scale` is pixels per module; 8 is
/// comfortable on a phone screen held at arm's length.
pub fn write_png(data: &str, path: &Path, scale: u32) -> Result<()> {
    let (side, pixels) = raster(data, scale)?;
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), side, side);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let png = |e: png::EncodingError| Error::Internal(format!("writing {}: {e}", path.display()));
    let mut writer = encoder.write_header().map_err(png)?;
    writer.write_image_data(&pixels).map_err(png)?;
    writer.finish().map_err(png)?;
    Ok(())
}

/// Where a saved QR lands: the user's home directory, named for the device.
pub fn png_path(name: Option<&str>) -> PathBuf {
    let label = name.unwrap_or("device");
    let safe: String =
        label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(format!("eris-pair-{safe}.png"))
}
