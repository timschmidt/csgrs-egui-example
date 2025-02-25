use eframe::{
    egui::{self, Color32, Pos2, Shape, Vec2},
};
use egui::epaint::{PathStroke, PathShape};
use egui::Stroke;

use nalgebra::{Matrix4, Vector3};

type CSG = csgrs::csg::CSG<()>;

/// A small struct holding your geometry plus camera controls.
struct MyApp {
    /// Triangles of the model (each triangle is [p0, p1, p2]), in `f32`.
    /// We store them here so we don’t have to rebuild them every frame.
    triangles: Vec<([f32; 3], [f32; 3], [f32; 3])>,

    /// Camera angles (yaw, pitch), in radians.
    yaw: f32,
    pitch: f32,
    /// Camera distance from origin
    dist: f32,

    /// Pan offset in the plane. Adjusts the final 2D position of the projection.
    pan_x: f32,
    pan_y: f32,
}

impl MyApp {
    fn new() -> Self {
        // 1) Build some geometry from csgrs
        let cube = CSG::cube(1.0, 1.0, 1.0, None);
        let sphere = CSG::sphere(1.0, 16, 8, None);

        // Union, then subdivide for more triangles
        let unioned = cube.union(&sphere);

        // 2) Gather triangle list in f32
        let mut triangles_f32 = Vec::new();
        for poly in &unioned.polygons {
            // Triangulate each polygon (most are already triangles after `subdivide_triangles`)
            let tri_list = poly.triangulate();
            for tri in tri_list {
                let p0 = tri[0].pos;
                let p1 = tri[1].pos;
                let p2 = tri[2].pos;
                triangles_f32.push((
                    [p0.x as f32, p0.y as f32, p0.z as f32],
                    [p1.x as f32, p1.y as f32, p1.z as f32],
                    [p2.x as f32, p2.y as f32, p2.z as f32],
                ));
            }
        }

        Self {
            triangles: triangles_f32,
            yaw: 0.0,
            pitch: 0.0,
            dist: 3.0,  // camera distance
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Respond to mouse input for rotation, panning, zooming:
        let input = ctx.input(|i| i.clone());

        if input.pointer.is_decidedly_dragging() {
            if let drag_delta = input.pointer.delta() {
                // Left click => rotate
                // Right click => pan
                if input.pointer.button_down(egui::PointerButton::Primary) {
                    self.yaw -= drag_delta.x * 0.01;   // turn left-right
                    self.pitch += drag_delta.y * 0.01; // turn up-down
                } else if input.pointer.button_down(egui::PointerButton::Secondary) {
                    self.pan_x += drag_delta.x * 0.5;
                    self.pan_y += drag_delta.y * 0.5;
                }
            }
        }
        // Scroll => zoom
        let scroll = input.raw_scroll_delta.y;
        if scroll.abs() > f32::EPSILON {
            self.dist *= (1.0 - scroll * 0.001).max(0.05);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Left-drag = rotate, Right-drag = pan, Scroll = zoom.");

            // Allocate a region to draw in
            let available_size = ui.available_size();
            let (response, painter) = ui.allocate_painter(available_size, egui::Sense::drag());

            // We'll do a "painter's algorithm" in the 2D space of this region.
            let rect = response.rect;
            let center_2d = rect.center();

            // Build a view transform from yaw, pitch, dist
            let cam = build_camera(self.yaw, self.pitch, self.dist);

            // We'll collect "renderable triangles" in a small vec
            let mut render_tris = Vec::new();
            for &(p0, p1, p2) in &self.triangles {
                // Transform each vertex by `cam`
                let v0 = transform(cam, p0);
                let v1 = transform(cam, p1);
                let v2 = transform(cam, p2);

                // We'll store the average depth, plus the 3 points in 2D
                let avg_z = (v0.z + v1.z + v2.z) / 3.0;
                // Some basic shading factor based on average Z (or normal, if we want).
                // For demonstration, let's darken further-away triangles:
                let shade = (1.0 - (avg_z / 10.0)).clamp(0.0, 1.0);

                // Projection: simple scale
                fn project(pos: Vector3<f32>) -> Option<[f32; 2]> {
                    // Choose a near plane (anything behind this is culled):
                    let near_z = 0.1;
                    let focal_length = 2.0;  // or whatever "camera" constant you want
                
                    // If it's behind the near plane, skip it (or clip partially).
                    if pos.z < near_z {
                        return None;
                    }
                
                    // Simple perspective scale:
                    let scale = focal_length / pos.z;
                    Some([pos.x * scale, -pos.y * scale])
                }
                let p0_2d = project(v0).unwrap();
                let p1_2d = project(v1).unwrap();
                let p2_2d = project(v2).unwrap();

                render_tris.push((avg_z, shade, p0_2d, p1_2d, p2_2d));
            }

            // Sort back-to-front by avg_z (descending)
            render_tris.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

            // Draw them
            for &(_z, shade, p0, p1, p2) in &render_tris {
                let color_val = (shade * 200.0) as u8;
                let col = Color32::from_rgb(color_val, color_val, color_val);

                // Convert 2D coords to egui::Pos2, with pan offset
                let c0 = Pos2::new(
                    center_2d.x + p0[0] * 100.0 + self.pan_x,
                    center_2d.y + p0[1] * 100.0 + self.pan_y,
                );
                let c1 = Pos2::new(
                    center_2d.x + p1[0] * 100.0 + self.pan_x,
                    center_2d.y + p1[1] * 100.0 + self.pan_y,
                );
                let c2 = Pos2::new(
                    center_2d.x + p2[0] * 100.0 + self.pan_x,
                    center_2d.y + p2[1] * 100.0 + self.pan_y,
                );

                // Fill the triangle
                let path = vec![c0, c1, c2, c0];
                //painter.add(Shape::closed_line(path, PathStroke::new(1.0, col)));
                //painter.add(Shape::convex_polygon(
                //    vec![c0, c1, c2],
                //    Color32::from_rgb(50, 100, 255), // fill color
                //    Stroke::NONE,
                //));
                painter.add(Shape::convex_polygon(
                    vec![c0, c1, c2],
                    Color32::from_rgb(50, 100, 255),
                    Stroke::new(1.0, Color32::from_rgb(255, 255, 255)),
                ));
            }
        });
    }
}

/// A helper: build a camera transform matrix (4x4) from yaw, pitch, distance.
/// For simplicity, we do just rotates and then translate along +Z.
fn build_camera(yaw: f32, pitch: f32, dist: f32) -> Matrix4<f32> {
    // Rotation around Y (yaw), then X (pitch).
    let rot_y = Matrix4::new_rotation(Vector3::y() * yaw);
    let rot_x = Matrix4::new_rotation(Vector3::x() * pitch);
    let translate = Matrix4::new_translation(&Vector3::new(0.0, 0.0, dist));
    translate * rot_x * rot_y
}

/// Multiply a 3D point by a 4x4 transform, returning a new 3D Vector.
fn transform(m: Matrix4<f32>, p: [f32; 3]) -> Vector3<f32> {
    let v = m * Vector4::new(p[0], p[1], p[2], 1.0);
    Vector3::new(v.x, v.y, v.z)
}

// We need Vector4 too
use nalgebra::Vector4;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "CSG Viewer (egui)",
        options,
        Box::new(|_cc| Ok(Box::new(MyApp::new()))),
    )
}
