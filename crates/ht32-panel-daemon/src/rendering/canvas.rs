//! Canvas for rendering to framebuffer.

use anyhow::Result;
use ht32_panel_hw::lcd::framebuffer::{rgb888_to_rgb565, Framebuffer};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use super::text::TextRenderer;

/// Brightens a color by the given factor.
fn brighten_color(color: u32, factor: f32) -> u32 {
    let r = ((color >> 16) & 0xFF) as f32;
    let g = ((color >> 8) & 0xFF) as f32;
    let b = (color & 0xFF) as f32;

    let r = ((r * factor).min(255.0)) as u32;
    let g = ((g * factor).min(255.0)) as u32;
    let b = ((b * factor).min(255.0)) as u32;

    (r << 16) | (g << 8) | b
}

/// Canvas for rendering.
pub struct Canvas {
    width: u32,
    height: u32,
    pixmap: Pixmap,
    background_color: u32,
    text_renderer: TextRenderer,
}

impl Canvas {
    /// Creates a new canvas.
    pub fn new(width: u32, height: u32) -> Self {
        let pixmap = Pixmap::new(width, height).expect("Failed to create pixmap");

        Self {
            width,
            height,
            pixmap,
            background_color: 0x000000, // Black
            text_renderer: TextRenderer::new(),
        }
    }

    /// Returns the canvas dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Resizes the canvas to new dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.pixmap = Pixmap::new(width, height).expect("Failed to create pixmap");
        }
    }

    /// Sets the background color.
    pub fn set_background(&mut self, color: u32) {
        self.background_color = color;
    }

    /// Clears the canvas.
    pub fn clear(&mut self) {
        let r = ((self.background_color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((self.background_color >> 8) & 0xFF) as f32 / 255.0;
        let b = (self.background_color & 0xFF) as f32 / 255.0;
        self.pixmap.fill(Color::from_rgba(r, g, b, 1.0).unwrap());
    }

    /// Draws a filled rectangle.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: u32) {
        debug_assert!(
            x >= 0 && y >= 0,
            "fill_rect: negative coordinates ({}, {})",
            x,
            y
        );
        debug_assert!(
            x + width as i32 <= self.width as i32,
            "fill_rect: x overflow ({} + {} > {})",
            x,
            width,
            self.width
        );
        debug_assert!(
            y + height as i32 <= self.height as i32,
            "fill_rect: y overflow ({} + {} > {})",
            y,
            height,
            self.height
        );

        let r = ((color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((color >> 8) & 0xFF) as f32 / 255.0;
        let b = (color & 0xFF) as f32 / 255.0;

        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(r, g, b, 1.0).unwrap());

        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32) {
            self.pixmap
                .fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// Draws a filled circle.
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: u32, color: u32) {
        debug_assert!(
            cx - radius as i32 >= 0,
            "fill_circle: left edge off screen ({} - {} < 0)",
            cx,
            radius
        );
        debug_assert!(
            cy - radius as i32 >= 0,
            "fill_circle: top edge off screen ({} - {} < 0)",
            cy,
            radius
        );
        debug_assert!(
            cx + radius as i32 <= self.width as i32,
            "fill_circle: right edge off screen ({} + {} > {})",
            cx,
            radius,
            self.width
        );
        debug_assert!(
            cy + radius as i32 <= self.height as i32,
            "fill_circle: bottom edge off screen ({} + {} > {})",
            cy,
            radius,
            self.height
        );

        let r = ((color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((color >> 8) & 0xFF) as f32 / 255.0;
        let b = (color & 0xFF) as f32 / 255.0;

        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(r, g, b, 1.0).unwrap());
        paint.anti_alias = true;

        let mut pb = PathBuilder::new();
        pb.push_circle(cx as f32, cy as f32, radius as f32);
        if let Some(path) = pb.finish() {
            self.pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Draws a line between two points.
    ///
    /// # Arguments
    /// * `x1`, `y1` - Start point
    /// * `x2`, `y2` - End point
    /// * `stroke_width` - Width of the line
    /// * `color` - RGB888 color
    #[allow(clippy::too_many_arguments)]
    pub fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, stroke_width: f32, color: u32) {
        let r = ((color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((color >> 8) & 0xFF) as f32 / 255.0;
        let b = (color & 0xFF) as f32 / 255.0;

        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(r, g, b, 1.0).unwrap());
        paint.anti_alias = true;

        let stroke = Stroke {
            width: stroke_width,
            line_cap: tiny_skia::LineCap::Round,
            ..Default::default()
        };

        let mut pb = PathBuilder::new();
        pb.move_to(x1 as f32, y1 as f32);
        pb.line_to(x2 as f32, y2 as f32);

        if let Some(path) = pb.finish() {
            self.pixmap
                .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    /// Draws an arc (unfilled, stroke only).
    ///
    /// # Arguments
    /// * `cx`, `cy` - Center of the arc
    /// * `radius` - Radius of the arc
    /// * `start_angle` - Start angle in radians (0 = right, PI/2 = down)
    /// * `end_angle` - End angle in radians
    /// * `stroke_width` - Width of the arc stroke
    /// * `color` - RGB888 color
    #[allow(clippy::too_many_arguments)]
    pub fn draw_arc(
        &mut self,
        cx: i32,
        cy: i32,
        radius: u32,
        start_angle: f32,
        end_angle: f32,
        stroke_width: f32,
        color: u32,
    ) {
        let total_radius = radius as i32 + (stroke_width / 2.0).ceil() as i32;
        debug_assert!(
            cx - total_radius >= 0,
            "draw_arc: left edge off screen ({} - {} < 0)",
            cx,
            total_radius
        );
        debug_assert!(
            cy - total_radius >= 0,
            "draw_arc: top edge off screen ({} - {} < 0)",
            cy,
            total_radius
        );
        debug_assert!(
            cx + total_radius <= self.width as i32,
            "draw_arc: right edge off screen ({} + {} > {})",
            cx,
            total_radius,
            self.width
        );
        debug_assert!(
            cy + total_radius <= self.height as i32,
            "draw_arc: bottom edge off screen ({} + {} > {})",
            cy,
            total_radius,
            self.height
        );

        let r = ((color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((color >> 8) & 0xFF) as f32 / 255.0;
        let b = (color & 0xFF) as f32 / 255.0;

        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(r, g, b, 1.0).unwrap());
        paint.anti_alias = true;

        let stroke = Stroke {
            width: stroke_width,
            ..Default::default()
        };

        // Build arc path using line segments (tiny_skia doesn't have native arc)
        let mut pb = PathBuilder::new();
        let segments = 64;
        let angle_span = end_angle - start_angle;
        let cx_f = cx as f32;
        let cy_f = cy as f32;
        let radius_f = radius as f32;

        for i in 0..=segments {
            let t = i as f32 / segments as f32;
            let angle = start_angle + t * angle_span;
            let px = cx_f + radius_f * angle.cos();
            let py = cy_f + radius_f * angle.sin();
            if i == 0 {
                pb.move_to(px, py);
            } else {
                pb.line_to(px, py);
            }
        }

        if let Some(path) = pb.finish() {
            self.pixmap
                .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    /// Draws text at the specified position.
    ///
    /// # Arguments
    /// * `x` - X position (left edge of text)
    /// * `y` - Y position (top edge of text)
    /// * `text` - The text to render
    /// * `size` - Font size in pixels
    /// * `color` - RGB888 color (0xRRGGBB)
    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, size: f32, color: u32) {
        let text_width = self.text_renderer.text_width(text, size);
        let text_height = self.text_renderer.line_height(size);
        debug_assert!(
            x >= 0 && y >= 0,
            "draw_text: negative coordinates ({}, {}) for '{}'",
            x,
            y,
            text
        );
        debug_assert!(
            x + text_width <= self.width as i32,
            "draw_text: text extends past right edge ({} + {} > {}) for '{}'",
            x,
            text_width,
            self.width,
            text
        );
        debug_assert!(
            y + text_height <= self.height as i32,
            "draw_text: text extends past bottom edge ({} + {} > {}) for '{}'",
            y,
            text_height,
            self.height,
            text
        );

        self.text_renderer
            .draw_text(&mut self.pixmap, x, y, text, size, color);
    }

    /// Draws text with horizontal scaling.
    ///
    /// # Arguments
    /// * `x` - X position (left edge of text)
    /// * `y` - Y position (top edge of text)
    /// * `text` - The text to render
    /// * `size` - Font size in pixels
    /// * `color` - RGB888 color (0xRRGGBB)
    /// * `x_scale` - Horizontal scale factor (< 1.0 makes text narrower)
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_scaled(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        size: f32,
        color: u32,
        x_scale: f32,
    ) {
        let text_width = self.text_renderer.text_width_scaled(text, size, x_scale);
        let text_height = self.text_renderer.line_height(size);
        debug_assert!(
            x >= 0 && y >= 0,
            "draw_text_scaled: negative coordinates ({}, {}) for '{}'",
            x,
            y,
            text
        );
        debug_assert!(
            x + text_width <= self.width as i32,
            "draw_text_scaled: text extends past right edge ({} + {} > {}) for '{}'",
            x,
            text_width,
            self.width,
            text
        );
        debug_assert!(
            y + text_height <= self.height as i32,
            "draw_text_scaled: text extends past bottom edge ({} + {} > {}) for '{}'",
            y,
            text_height,
            self.height,
            text
        );

        self.text_renderer
            .draw_text_scaled(&mut self.pixmap, x, y, text, size, color, x_scale);
    }

    /// Returns the width of text when rendered at the specified size.
    pub fn text_width(&self, text: &str, size: f32) -> i32 {
        self.text_renderer.text_width(text, size)
    }

    /// Returns the width of text when rendered with horizontal scaling.
    pub fn text_width_scaled(&self, text: &str, size: f32, x_scale: f32) -> i32 {
        self.text_renderer.text_width_scaled(text, size, x_scale)
    }

    /// Returns the line height for the specified font size.
    pub fn line_height(&self, size: f32) -> i32 {
        self.text_renderer.line_height(size)
    }

    /// Draws a scrolling line graph from historical data.
    ///
    /// # Arguments
    /// * `x` - X position (left edge)
    /// * `y` - Y position (top edge)
    /// * `width` - Width of the graph area
    /// * `height` - Height of the graph area
    /// * `data` - Historical data points (oldest first, newest last)
    /// * `max_value` - Maximum value for scaling (values above this are clamped)
    /// * `line_color` - Color for the line/bars
    /// * `bg_color` - Background color for the graph area
    #[allow(clippy::too_many_arguments)]
    pub fn draw_graph(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        data: &[f64],
        max_value: f64,
        line_color: u32,
        bg_color: u32,
    ) {
        debug_assert!(
            x >= 0 && y >= 0,
            "draw_graph: negative coordinates ({}, {})",
            x,
            y
        );
        debug_assert!(
            x + width as i32 <= self.width as i32,
            "draw_graph: x overflow ({} + {} > {})",
            x,
            width,
            self.width
        );
        debug_assert!(
            y + height as i32 <= self.height as i32,
            "draw_graph: y overflow ({} + {} > {})",
            y,
            height,
            self.height
        );

        // Draw background - use internal fill to avoid duplicate bounds check
        let r = ((bg_color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((bg_color >> 8) & 0xFF) as f32 / 255.0;
        let b = (bg_color & 0xFF) as f32 / 255.0;
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(r, g, b, 1.0).unwrap());
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32) {
            self.pixmap
                .fill_rect(rect, &paint, Transform::identity(), None);
        }

        if data.is_empty() || max_value <= 0.0 {
            return;
        }

        // Compute highlight colors for high values
        let high_color = brighten_color(line_color, 1.4); // 95-99%: brighter
        let max_color = 0xFFFFFF; // 100%: white

        let num_points = data.len();
        let bar_width = (width as f64 / num_points as f64).max(1.0);

        // Draw bars from left to right (oldest to newest)
        for (i, &value) in data.iter().enumerate() {
            let normalized = (value / max_value).min(1.0);
            let bar_height = (normalized * height as f64) as u32;

            if bar_height > 0 {
                let bar_x = x + (i as f64 * bar_width) as i32;
                let bar_y = y + (height - bar_height) as i32;

                // Choose color based on how close to max
                let color = if normalized >= 1.0 {
                    max_color
                } else if normalized >= 0.95 {
                    high_color
                } else {
                    line_color
                };

                self.fill_rect(bar_x, bar_y, bar_width.ceil() as u32, bar_height, color);
            }
        }
    }

    /// Resolves the display color for a bar based on its normalized value.
    ///
    /// Returns white at `>= 1.0`, a brightened color at `>= 0.95`, or the base color otherwise.
    fn bar_color(normalized: f64, base_color: u32) -> u32 {
        if normalized >= 1.0 {
            0xFFFFFF
        } else if normalized >= 0.95 {
            brighten_color(base_color, 1.4)
        } else {
            base_color
        }
    }

    /// Draws a dual-series scrolling line graph from historical data.
    ///
    /// # Arguments
    /// * `x` - X position (left edge)
    /// * `y` - Y position (top edge)
    /// * `width` - Width of the graph area
    /// * `height` - Height of the graph area
    /// * `data1` - First data series (e.g., read/rx rates)
    /// * `data2` - Second data series (e.g., write/tx rates)
    /// * `max_value` - Maximum value for scaling (values above this are clamped)
    /// * `color1` - Color for first series
    /// * `color2` - Color for second series
    /// * `bg_color` - Background color for the graph area
    #[allow(clippy::too_many_arguments)]
    pub fn draw_dual_graph(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        data1: &[f64],
        data2: &[f64],
        max_value: f64,
        color1: u32,
        color2: u32,
        bg_color: u32,
    ) {
        debug_assert!(
            x >= 0 && y >= 0,
            "draw_dual_graph: negative coordinates ({}, {})",
            x,
            y
        );
        debug_assert!(
            x + width as i32 <= self.width as i32,
            "draw_dual_graph: x overflow ({} + {} > {})",
            x,
            width,
            self.width
        );
        debug_assert!(
            y + height as i32 <= self.height as i32,
            "draw_dual_graph: y overflow ({} + {} > {})",
            y,
            height,
            self.height
        );

        // Draw background
        let r = ((bg_color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((bg_color >> 8) & 0xFF) as f32 / 255.0;
        let b = (bg_color & 0xFF) as f32 / 255.0;
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(r, g, b, 1.0).unwrap());
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32) {
            self.pixmap
                .fill_rect(rect, &paint, Transform::identity(), None);
        }

        if (data1.is_empty() && data2.is_empty()) || max_value <= 0.0 {
            return;
        }

        // Use the longer of the two series for bar width calculation
        let num_points = data1.len().max(data2.len());
        if num_points == 0 {
            return;
        }
        let bar_width = (width as f64 / num_points as f64).max(1.0);

        // Draw first series (e.g., read/rx) - draw from bottom
        for (i, &value) in data1.iter().enumerate() {
            let normalized = (value / max_value).min(1.0);
            let bar_height = (normalized * height as f64) as u32;

            if bar_height > 0 {
                let bar_x = x + (i as f64 * bar_width) as i32;
                let bar_y = y + (height - bar_height) as i32;
                self.fill_rect(
                    bar_x,
                    bar_y,
                    bar_width.ceil() as u32,
                    bar_height,
                    Self::bar_color(normalized, color1),
                );
            }
        }

        // Draw second series (e.g., write/tx) - draw on top with some transparency effect
        // We draw slightly thinner bars offset by 1 pixel to create layered effect
        for (i, &value) in data2.iter().enumerate() {
            let normalized = (value / max_value).min(1.0);
            let bar_height = (normalized * height as f64) as u32;

            if bar_height > 0 {
                let bar_x = x + (i as f64 * bar_width) as i32;
                let bar_y = y + (height - bar_height) as i32;
                // Draw slightly narrower bars for the overlay effect
                let overlay_width = (bar_width * 0.6).ceil() as u32;
                self.fill_rect(
                    bar_x,
                    bar_y,
                    overlay_width,
                    bar_height,
                    Self::bar_color(normalized, color2),
                );
            }
        }
    }

    /// Draws a wrap-around (oscilloscope) dual-series graph.
    ///
    /// Unlike `draw_dual_graph` which scrolls, this places each sample at a fixed column
    /// determined by its absolute index modulo 60, creating an oscilloscope-style overwrite
    /// effect. `data1`/`data2` are the last `len` samples in natural scrolling order (oldest
    /// first, newest last). `count` is the total number of samples ever pushed (monotonic).
    ///
    /// Column formula for window-index `k`: `s = ((count - len + k) % 60) as i32`.
    /// Each retained sample keeps the same column as `count` and the window advance together.
    ///
    /// # Arguments
    /// * `x` - X position (left edge)
    /// * `y` - Y position (top edge)
    /// * `width` - Width of the graph area
    /// * `height` - Height of the graph area
    /// * `data1` - First data series in natural order (oldest first, newest last; len ≤ 60)
    /// * `data2` - Second data series in natural order (oldest first, newest last; len ≤ 60)
    /// * `count` - Total samples ever pushed; determines the write-head position
    /// * `max_value` - Maximum value for scaling (values above this are clamped)
    /// * `color1` - Color for first series
    /// * `color2` - Color for second series
    /// * `bg_color` - Background color for the graph area
    #[allow(clippy::too_many_arguments)]
    pub fn draw_dual_graph_wrap(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        data1: &[f64],
        data2: &[f64],
        count: u64,
        max_value: f64,
        color1: u32,
        color2: u32,
        bg_color: u32,
    ) {
        debug_assert!(
            x >= 0 && y >= 0,
            "draw_dual_graph_wrap: negative coordinates ({}, {})",
            x,
            y
        );
        debug_assert!(
            x + width as i32 <= self.width as i32,
            "draw_dual_graph_wrap: x overflow ({} + {} > {})",
            x,
            width,
            self.width
        );
        debug_assert!(
            y + height as i32 <= self.height as i32,
            "draw_dual_graph_wrap: y overflow ({} + {} > {})",
            y,
            height,
            self.height
        );

        // Fill background
        let r = ((bg_color >> 16) & 0xFF) as f32 / 255.0;
        let g = ((bg_color >> 8) & 0xFF) as f32 / 255.0;
        let b = (bg_color & 0xFF) as f32 / 255.0;
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(r, g, b, 1.0).unwrap());
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32) {
            self.pixmap
                .fill_rect(rect, &paint, Transform::identity(), None);
        }

        if (data1.is_empty() && data2.is_empty()) || max_value <= 0.0 {
            return;
        }

        // W = 60 columns; bar_width scales to fit
        let bar_width = (width as f64 / 60.0).max(1.0);

        // len is the window size (number of samples in the slice).
        // Absolute index of window-index k: abs_k = count - len + k
        // Column: s = abs_k % 60 = (count - len + k) % 60
        // Use saturating_sub to handle warm-up phase where count < len.
        let len = data1.len().max(data2.len()) as u64;
        let base = count.saturating_sub(len);

        // Draw first series
        for (k, &value) in data1.iter().enumerate() {
            let normalized = (value / max_value).min(1.0);
            let bar_height = (normalized * height as f64) as u32;

            if bar_height > 0 {
                let s = ((base + k as u64) % 60) as i32;
                let bar_x = x + (s as f64 * bar_width) as i32;
                let bar_y = y + (height - bar_height) as i32;
                self.fill_rect(
                    bar_x,
                    bar_y,
                    bar_width.ceil() as u32,
                    bar_height,
                    Self::bar_color(normalized, color1),
                );
            }
        }

        // Draw second series on top (narrower overlay, same placement)
        for (k, &value) in data2.iter().enumerate() {
            let normalized = (value / max_value).min(1.0);
            let bar_height = (normalized * height as f64) as u32;

            if bar_height > 0 {
                let s = ((base + k as u64) % 60) as i32;
                let bar_x = x + (s as f64 * bar_width) as i32;
                let bar_y = y + (height - bar_height) as i32;
                let overlay_width = (bar_width * 0.6).ceil() as u32;
                self.fill_rect(
                    bar_x,
                    bar_y,
                    overlay_width,
                    bar_height,
                    Self::bar_color(normalized, color2),
                );
            }
        }

        // Write-head gap: blank a 1-px strip at the next column to be written.
        // count % 60 is the ring slot about to be overwritten; clearing it creates
        // the visible "write cursor" on screen.
        let gap_x = x + (((count % 60) as f64) * bar_width) as i32;
        self.fill_rect(gap_x, y, 1, height, bg_color);
    }

    /// Renders the canvas to a framebuffer.
    pub fn render_to_framebuffer(&self, fb: &mut Framebuffer) -> Result<()> {
        let pixels = self.pixmap.pixels();
        let data = fb.data_mut();

        for (i, pixel) in pixels.iter().enumerate() {
            if i < data.len() {
                data[i] = rgb888_to_rgb565(pixel.red(), pixel.green(), pixel.blue());
            }
        }

        Ok(())
    }

    /// Returns the raw RGBA pixels.
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Returns the pixmap pixels as color values.
    pub fn pixmap_pixels(&self) -> &[tiny_skia::PremultipliedColorU8] {
        self.pixmap.pixels()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_creation() {
        let canvas = Canvas::new(320, 170);
        assert_eq!(canvas.dimensions(), (320, 170));
    }

    /// Verifies that a non-zero sample at window-index k produces a non-background bar
    /// at the expected pixel column `((count - len + k) % 60)`.
    ///
    /// With count=65, len=60, k=5: abs_index = 65-60+5 = 10; column = 10 % 60 = 10.
    #[test]
    fn wrap_graph_positive_placement() {
        let w = 60u32;
        let h = 20u32;
        let bg = 0x000000u32;
        let count: u64 = 65;
        let len: u64 = 60;
        // Non-zero sample at window-index k=5; all others zero.
        let mut d1 = vec![0.0f64; len as usize];
        d1[5] = 4.5; // 4.5 / 9.0 = 0.5 → bar_height = floor(0.5 * 20) = 10
        let d2 = vec![0.0f64; len as usize];

        let mut c = Canvas::new(w, h);
        c.set_background(bg);
        c.clear();
        c.draw_dual_graph_wrap(0, 0, w, h, &d1, &d2, count, 9.0, 0xFF0000, 0x00FF00, bg);

        let pixels = c.pixels();
        // bar_width = 60.0 / 60.0 = 1.0
        // expected column = (count - len + k) % 60 = (65 - 60 + 5) % 60 = 10
        let k = 5usize;
        let expected_col = ((count - len + k as u64) % 60) as usize;
        assert_eq!(expected_col, 10, "formula sanity check");

        // At least one pixel in the expected column must be non-background.
        let has_bar = (0..h as usize).any(|row| {
            let i = (row * w as usize + expected_col) * 4;
            let (r, g, b) = (pixels[i], pixels[i + 1], pixels[i + 2]);
            r != 0 || g != 0 || b != 0
        });
        assert!(
            has_bar,
            "expected a non-bg bar at column {expected_col} but found none"
        );

        // The adjacent column (expected_col - 1) should be background (no data there).
        let adj_col = expected_col - 1;
        let adj_is_bg = (0..h as usize).all(|row| {
            let i = (row * w as usize + adj_col) * 4;
            pixels[i] == 0 && pixels[i + 1] == 0 && pixels[i + 2] == 0
        });
        assert!(
            adj_is_bg,
            "column {adj_col} should be background but has non-zero pixels"
        );
    }

    /// Determinism: same (data, count) always produces identical pixels.
    #[test]
    fn wrap_graph_is_deterministic() {
        let w = 60u32;
        let h = 20u32;
        let d1: Vec<f64> = (0..60).map(|i| (i % 10) as f64).collect();
        let d2: Vec<f64> = vec![0.0; 60];
        let render = |count: u64| {
            let mut c = Canvas::new(w, h);
            c.set_background(0);
            c.clear();
            c.draw_dual_graph_wrap(
                0, 0, w, h, &d1, &d2, count, 9.0, 0xFF0000, 0x00FF00, 0x000000,
            );
            c.pixels().to_vec()
        };
        assert_eq!(render(100), render(100), "wrap graph not deterministic");
    }

    /// Head-locality: advancing window AND count together (as in production) changes only
    /// the two columns near the write head, not the whole display.
    ///
    /// In production a new sample BOTH appends to the window (dropping the oldest) AND
    /// increments count by 1.  Each retained sample keeps the same abs_index % 60 column.
    /// Only the newly written column and the head-gap column change.
    #[test]
    fn wrap_graph_is_head_local_when_window_and_count_advance_together() {
        let w = 60u32;
        let h = 20u32;
        let base: Vec<f64> = (0..60).map(|i| (i % 10) as f64).collect(); // full window
        let zero = vec![0.0f64; 60];
        let render = |data: &[f64], count: u64| {
            let mut c = Canvas::new(w, h);
            c.set_background(0);
            c.clear();
            c.draw_dual_graph_wrap(
                0, 0, w, h, data, &zero, count, 9.0, 0xFF0000, 0x00FF00, 0x000000,
            );
            c.pixels().to_vec()
        };
        let a = render(&base, 100);
        // One new sample: window drops oldest (front) + appends newest (back), count += 1.
        let mut adv = base.clone();
        adv.remove(0);
        adv.push(7.0);
        let b = render(&adv, 101);
        let differing = (0..w as usize)
            .filter(|&col| {
                (0..h as usize).any(|row| {
                    let i = (row * w as usize + col) * 4;
                    a[i..i + 3] != b[i..i + 3]
                })
            })
            .count();
        assert!(
            differing <= 3,
            "advancing window+count together changed {differing} cols; expected <= 3"
        );
    }
}
