use anyhow::*;
use image::{DynamicImage, GenericImage, GenericImageView, ImageBuffer};
use std::cmp;
use std::path::*;

use crate::app::thumbnail::{read, Options};

pub fn generate_thumbnail(image_path: &Path, options: &Options) -> Result<DynamicImage> {
	let source_image = DynamicImage::ImageRgb8(read(image_path)?.into_rgb8());
	let (source_width, source_height) = source_image.dimensions();
	let largest_dimension = cmp::max(source_width, source_height);
	let out_dimension = cmp::min(
		options.max_dimension.unwrap_or(largest_dimension),
		largest_dimension,
	);

	let source_aspect_ratio: f32 = source_width as f32 / source_height as f32;
	let is_almost_square = source_aspect_ratio > 0.8 && source_aspect_ratio < 1.2;

	let mut final_image;
	if is_almost_square && options.resize_if_almost_square {
		final_image = source_image.thumbnail_exact(out_dimension, out_dimension);
	} else if options.pad_to_square {
		let scaled_image = source_image.thumbnail(out_dimension, out_dimension);
		let (scaled_width, scaled_height) = scaled_image.dimensions();
		let background = image::Rgb([255, 255_u8, 255_u8]);
		final_image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(
			out_dimension,
			out_dimension,
			background,
		));
		final_image.copy_from(
			&scaled_image,
			(out_dimension - scaled_width) / 2,
			(out_dimension - scaled_height) / 2,
		)?;
	} else {
		final_image = source_image.thumbnail(out_dimension, out_dimension);
	}

	Ok(final_image)
}
