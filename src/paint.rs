//! One CPU rasterizer shared by native presentation and the isolated PNG path.
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
pub struct Paint {
    renderer: EguiSoftwareRender,
    pub pixels: Vec<[u8; 4]>,
}
impl Paint {
    pub fn new() -> Self {
        Self {
            renderer: EguiSoftwareRender::new(ColorFieldOrder::Rgba),
            pixels: vec![],
        }
    }
    pub fn raster(
        &mut self,
        ctx: &egui::Context,
        output: &egui::FullOutput,
        width: u32,
        height: u32,
    ) {
        self.pixels
            .resize(width as usize * height as usize, [19, 26, 34, 255]);
        // The rasterizer alpha-composites its cached canvas; transparent areas must
        // start from a clean backbuffer rather than the preceding frame's text.
        self.pixels.fill([19, 26, 34, 255]);
        let primitives = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
        self.renderer.render(
            &mut BufferMutRef::new(&mut self.pixels, width as usize, height as usize),
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_content_does_not_survive_in_the_next_frame() {
        let ctx = egui::Context::default();
        let mut paint = Paint::new();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(80.0, 80.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input(), |ctx| {
            ctx.layer_painter(egui::LayerId::background()).rect_filled(
                egui::Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(60.0, 60.0)),
                0.0,
                egui::Color32::WHITE,
            );
        });
        paint.raster(&ctx, &output, 80, 80);
        assert_eq!(paint.pixels[40 * 80 + 40], [255; 4]);
        let output = ctx.run(input(), |_| {});
        paint.raster(&ctx, &output, 80, 80);
        assert_eq!(paint.pixels[40 * 80 + 40], [19, 26, 34, 255]);
    }
}
