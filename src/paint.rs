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
        let primitives = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
        self.renderer.render(
            &mut BufferMutRef::new(&mut self.pixels, width as usize, height as usize),
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
        );
    }
}
