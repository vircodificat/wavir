use std::path::Path;

use ab_glyph::{Font as _, FontArc, Glyph, GlyphId, Point as FPoint, PxScale, ScaleFont};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::{Point, Rect};
use sdl2::render::{BlendMode, Canvas, TextureCreator};
use sdl2::surface::Surface;
use sdl2::video::{Window, WindowContext};

/// High-level 2D renderer wrapping the SDL2 canvas.
pub struct Renderer {
    pub canvas: Canvas<Window>,
    texture_creator: TextureCreator<WindowContext>,
    font: FontArc,
}

impl Renderer {
    pub fn new(window: Window, font_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let canvas = window
            .into_canvas()
            .accelerated()
            .present_vsync()
            .build()
            .map_err(|e| e.to_string())?;
        let texture_creator = canvas.texture_creator();
        let font = FontArc::try_from_vec(std::fs::read(font_path)?)?;
        Ok(Self {
            canvas,
            texture_creator,
            font,
        })
    }

    pub fn set_draw_color(&mut self, color: Color) {
        self.canvas.set_draw_color(color);
    }

    pub fn clear(&mut self) {
        self.canvas.clear();
    }

    pub fn present(&mut self) {
        self.canvas.present();
    }

    /// Size of the drawable area in pixels.
    pub fn output_size(&self) -> Result<(u32, u32), String> {
        self.canvas.output_size().map_err(|e| e.to_string())
    }

    /// Draw a straight line from `(x1, y1)` to `(x2, y2)`.
    pub fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<(), String> {
        self.canvas
            .draw_line(Point::new(x1, y1), Point::new(x2, y2))
    }

    /// Draw a dashed line from `(x1, y1)` to `(x2, y2)`:
    /// `dash` pixels on, `gap` pixels off, along the whole segment.
    pub fn dashed_line(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        dash: u32,
        gap: u32,
    ) -> Result<(), String> {
        let dx = (x2 - x1) as f32;
        let dy = (y2 - y1) as f32;
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.0 {
            return Ok(());
        }
        let (ux, uy) = (dx / len, dy / len);
        let (dash, gap) = (dash as f32, gap as f32);
        let mut t = 0.0;
        while t < len {
            let t_end = (t + dash).min(len);
            self.line(
                (x1 as f32 + ux * t).round() as i32,
                (y1 as f32 + uy * t).round() as i32,
                (x1 as f32 + ux * t_end).round() as i32,
                (y1 as f32 + uy * t_end).round() as i32,
            )?;
            t = t_end + gap;
        }
        Ok(())
    }

    /// Measure `text`; returns its size in pixels at the given font size.
    pub fn text_size(&self, text: &str, px_size: f32) -> (u32, u32) {
        let (width, height, _) = rasterize_text(&self.font, text, px_size);
        (width, height)
    }

    /// Draw `text` with its bounding box top-left at `(x, y)`.
    /// RGB is taken from `color`; the glyph coverage supplies the alpha.
    /// Returns the drawn text size in pixels.
    pub fn text(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        px_size: f32,
        color: Color,
    ) -> Result<(u32, u32), String> {
        let (width, height, pixels) = rasterize_text(&self.font, text, px_size);
        let mut surface =
            Surface::new(width, height, PixelFormatEnum::RGBA32).map_err(|e: String| e)?;
        surface.with_lock_mut(|buf: &mut [u8]| buf.copy_from_slice(&pixels));
        let mut texture = self
            .texture_creator
            .create_texture_from_surface(&surface)
            .map_err(|e: sdl2::render::TextureValueError| e.to_string())?;
        texture.set_blend_mode(BlendMode::Blend);
        texture.set_color_mod(color.r, color.g, color.b);
        texture.set_alpha_mod(color.a);
        let dest = Rect::new(x, y, width, height);
        self.canvas.copy(&texture, None, Some(dest))?;
        Ok((width, height))
    }
}

/// Rasterize `text` at the given pixel size into an RGBA8 buffer
/// (white RGB + alpha coverage) using the `ab_glyph` outline rasterizer.
fn rasterize_text(font: &FontArc, text: &str, px_size: f32) -> (u32, u32, Vec<u8>) {
    let scale = PxScale::from(px_size);
    let scaled = font.as_scaled(scale);
    let baseline = scaled.ascent();

    // Lay out glyphs left to right (advances + kerning), baseline at `ascent`.
    let mut pen_x = 0.0f32;
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut prev: Option<GlyphId> = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(prev_id) = prev {
            pen_x += scaled.kern(prev_id, id);
        }
        glyphs.push(id.with_scale_and_position(
            scale,
            FPoint { x: pen_x, y: baseline },
        ));
        pen_x += scaled.h_advance(id);
        prev = Some(id);
    }

    // Bounding box over all glyph outlines.
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for g in &glyphs {
        if let Some(og) = font.outline_glyph(g.clone()) {
            let b = og.px_bounds();
            min_x = min_x.min(b.min.x);
            min_y = min_y.min(b.min.y);
            max_x = max_x.max(b.max.x);
            max_y = max_y.max(b.max.y);
        }
    }

    if min_x > max_x {
        // No outlined glyphs (e.g. whitespace only): empty buffer.
        let width = (pen_x.round() as u32).max(1);
        let height = ((scaled.ascent() - scaled.descent()).round() as u32).max(1);
        return (width, height, vec![0u8; (width * height * 4) as usize]);
    }

    let x0 = min_x.floor() as i32;
    let y0 = min_y.floor() as i32;
    let width = ((max_x.ceil() as i32 - x0) as u32).max(1);
    let height = ((max_y.ceil() as i32 - y0) as u32).max(1);
    let mut pixels = vec![0u8; (width * height * 4) as usize];

    for g in glyphs {
        let Some(og) = font.outline_glyph(g) else {
            continue;
        };
        // Per-glyph pixel offsets are relative to the glyph's own bounds,
        // so rebase them onto the global bounding box.
        let b = og.px_bounds();
        let gx0 = b.min.x as i32 - x0;
        let gy0 = b.min.y as i32 - y0;
        og.draw(|gx, gy, c| {
            let px = gx0 + gx as i32;
            let py = gy0 + gy as i32;
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                return;
            }
            let idx = (py as u32 * width + px as u32) as usize * 4;
            pixels[idx] = 255;
            pixels[idx + 1] = 255;
            pixels[idx + 2] = 255;
            pixels[idx + 3] = (c * 255.0).round().min(255.0) as u8;
        });
    }

    (width, height, pixels)
}
