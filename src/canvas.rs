use std::path::{Path, PathBuf};

use ab_glyph::{Font as _, FontArc, Glyph, GlyphId, Point as FPoint, PxScale, ScaleFont};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::{Point, Rect};
use sdl2::render::{BlendMode, Canvas, TextureCreator};
use sdl2::surface::Surface;
use sdl2::video::{Window, WindowContext};

/// Backend-agnostic drawing interface.
///
/// [`Renderer`] draws onto an SDL2 window; [`SvgRenderer`] records the same
/// calls into an SVG document, so frames can be captured to a file without
/// opening a window.
pub trait Render {
    /// Set the color used by subsequent `line`/`dashed_line` calls (and,
    /// for `clear`, the background).
    fn set_draw_color(&mut self, color: Color);

    /// Fill the whole output area with the current draw color.
    fn clear(&mut self);

    /// Flush the frame: swap buffers on-screen, or write the file on disk.
    fn present(&mut self) -> Result<(), String>;

    /// Size of the drawable area in pixels.
    fn output_size(&self) -> Result<(u32, u32), String>;

    /// Draw a straight line from `(x1, y1)` to `(x2, y2)`.
    fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<(), String>;

    /// Draw a rectangle outline with top-left `(x, y)` and the given size.
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<(), String> {
        self.line(x, y, x + w, y)?;
        self.line(x + w, y, x + w, y + h)?;
        self.line(x + w, y + h, x, y + h)?;
        self.line(x, y + h, x, y)
    }

    /// Draw a dashed line from `(x1, y1)` to `(x2, y2)`:
    /// `dash` pixels on, `gap` pixels off, along the whole segment.
    fn dashed_line(
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

    /// Fill a rectangle with the current draw color.
    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32) -> Result<(), String>;

    /// Measure `text`; returns its size in pixels at the given font size.
    fn text_size(&self, text: &str, px_size: f32) -> (u32, u32);

    /// Draw `text` with its bounding box top-left at `(x, y)`.
    /// Returns the drawn text size in pixels.
    fn text(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        px_size: f32,
        color: Color,
    ) -> Result<(u32, u32), String>;
}

/// High-level SDL2 window renderer.
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
}

impl Render for Renderer {
    fn set_draw_color(&mut self, color: Color) {
        self.canvas.set_draw_color(color);
    }

    fn clear(&mut self) {
        self.canvas.clear();
    }

    fn present(&mut self) -> Result<(), String> {
        self.canvas.present();
        Ok(())
    }

    fn output_size(&self) -> Result<(u32, u32), String> {
        self.canvas.output_size().map_err(|e| e.to_string())
    }

    fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<(), String> {
        self.canvas
            .draw_line(Point::new(x1, y1), Point::new(x2, y2))
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32) -> Result<(), String> {
        self.canvas
            .fill_rect(Rect::new(x, y, w, h))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn text_size(&self, text: &str, px_size: f32) -> (u32, u32) {
        let (width, height, _) = rasterize_text(&self.font, text, px_size);
        (width, height)
    }

    fn text(
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

/// Records draw calls into an SVG document, written to `path` on `present`.
///
/// Meant for capturing screenshots headlessly (see the `--svg` flag in
/// `main.rs`); geometry matches the SDL renderer up to rounding, and text
/// uses a monospace font with an approximate advance width.
pub struct SvgRenderer {
    path: PathBuf,
    width: u32,
    height: u32,
    color: Color,
    background: Color,
    body: Vec<String>,
}

impl SvgRenderer {
    pub fn new(path: impl Into<PathBuf>, width: u32, height: u32) -> Self {
        Self {
            path: path.into(),
            width,
            height,
            color: Color::RGB(0, 0, 0),
            background: Color::RGB(0, 0, 0),
            body: Vec::new(),
        }
    }

    fn push(&mut self, element: String) {
        self.body.push(element);
    }
}

impl Render for SvgRenderer {
    fn set_draw_color(&mut self, color: Color) {
        self.color = color;
    }

    fn clear(&mut self) {
        self.background = self.color;
    }

    fn present(&mut self) -> Result<(), String> {
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
            self.width, self.height, self.width, self.height
        );
        svg.push_str(&format!(
            "<rect width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
            self.width,
            self.height,
            rgb_hex(self.background)
        ));
        for element in &self.body {
            svg.push_str(element);
            svg.push('\n');
        }
        svg.push_str("</svg>\n");
        std::fs::write(&self.path, svg).map_err(|e| e.to_string())
    }

    fn output_size(&self) -> Result<(u32, u32), String> {
        Ok((self.width, self.height))
    }

    fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<(), String> {
        self.push(format!(
            "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{}\" stroke-width=\"1\"/>",
            rgb_hex(self.color)
        ));
        Ok(())
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32) -> Result<(), String> {
        self.push(format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{}\"/>",
            rgb_hex(self.color)
        ));
        Ok(())
    }

    fn text_size(&self, text: &str, px_size: f32) -> (u32, u32) {
        // DejaVuSansMono (the font used by `Renderer`) has an advance of
        // roughly 0.6em; approximate for measurement purposes.
        let width = (px_size * 0.6 * text.chars().count() as f32).round() as u32;
        (width.max(1), (px_size as u32).max(1))
    }

    fn text(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        px_size: f32,
        color: Color,
    ) -> Result<(u32, u32), String> {
        // SVG positions text by its baseline; approximate it from the top of
        // the bounding box the way `Renderer` lays text out.
        let baseline = y as f32 + px_size * 0.8;
        self.push(format!(
            "<text x=\"{x}\" y=\"{}\" font-family=\"monospace\" font-size=\"{}\" fill=\"{}\">{}</text>",
            baseline,
            px_size,
            rgb_hex(color),
            escape_xml(text)
        ));
        let (width, height) = self.text_size(text, px_size);
        Ok((width, height))
    }
}

/// Format an RGB color as `#rrggbb`.
fn rgb_hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// Escape the characters that are special in XML text content.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
