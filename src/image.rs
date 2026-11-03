//! PNG encoding for rendered preview frames.

/// Encodes tight `F32 RGBA` pixels as an 8-bit sRGB-ish PNG.
///
/// The engine hands back float samples; clamping to `[0, 1]` and scaling to
/// 8 bits is the lossy step every preview path takes. Non-finite samples become
/// transparent black instead of poisoning the image.
pub fn encode_rgba_f32(width: u32, height: u32, pixels: &[f32]) -> Result<Vec<u8>, String> {
	let expected = (width as usize)
		.checked_mul(height as usize)
		.and_then(|value| value.checked_mul(4))
		.ok_or_else(|| "image dimensions overflow".to_string())?;
	if pixels.len() < expected {
		return Err(format!(
			"frame buffer too small: expected {expected} floats, got {}",
			pixels.len()
		));
	}
	let mut bytes = Vec::with_capacity(expected);
	for &sample in &pixels[..expected] {
		let value = if sample.is_finite() {
			(sample.clamp(0.0, 1.0) * 255.0).round() as u8
		} else {
			0
		};
		bytes.push(value);
	}

	let mut out = Vec::new();
	{
		let mut encoder = png::Encoder::new(&mut out, width, height);
		encoder.set_color(png::ColorType::Rgba);
		encoder.set_depth(png::BitDepth::Eight);
		let mut writer = encoder
			.write_header()
			.map_err(|error| format!("png header failed: {error}"))?;
		writer
			.write_image_data(&bytes)
			.map_err(|error| format!("png write failed: {error}"))?;
	}
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encodes_a_one_pixel_png() {
		let png = encode_rgba_f32(1, 1, &[1.0, 0.0, 0.0, 1.0]).unwrap();
		assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
	}

	#[test]
	fn rejects_short_buffers() {
		let error = encode_rgba_f32(2, 2, &[0.0, 0.0, 0.0, 0.0]).unwrap_err();
		assert!(error.contains("too small"), "{error}");
	}
}
