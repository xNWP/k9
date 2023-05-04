use std::path::PathBuf;

use uuid::Uuid;

use crate::{
    camera::ScreenCamera,
    graphics::{
        system::{BuiltInShader, GraphicsCommandInterface},
        Vertex,
    },
};

use super::{GraphicsComponentImpl, RenderLocation};

pub struct TexQuadBase {
    vdimensions: (f32, f32),
    location: RenderLocation,
    texture_path: PathBuf,
    core: Option<TexQuadCore>,
}
struct TexQuadCore {
    vert_src: Uuid,
    tex: Uuid,
    sh_vert: Uuid,
    sh_frag: Uuid,
    program: Uuid,
    u_transform: Uuid,
}
impl TexQuadBase {
    pub fn new() -> Self {
        Self {
            vdimensions: (200.0, 200.0),
            location: RenderLocation::Screen(0.0, 0.0, 0.0),
            texture_path: PathBuf::from("assets/textures/test_squeezel.png"),
            core: None,
        }
    }
}
impl GraphicsComponentImpl for TexQuadBase {
    fn create(&mut self, k9cmd: &mut GraphicsCommandInterface, _screen_camera: &ScreenCamera) {
        let w2 = self.vdimensions.0 / 2.0;
        let h2 = self.vdimensions.1 / 2.0;
        let vertices: Vec<Vertex> = [
            Vertex {
                x: -w2,
                y: h2,
                z: 0.0,
                u: 0.0,
                v: 0.0,
            }, // tl
            Vertex {
                x: w2,
                y: h2,
                z: 0.0,
                u: 1.0,
                v: 0.0,
            }, // tr
            Vertex {
                x: w2,
                y: -h2,
                z: 0.0,
                u: 1.0,
                v: 1.0,
            }, // br
            Vertex {
                x: -w2,
                y: -h2,
                z: 0.0,
                u: 0.0,
                v: 1.0,
            }, // bl
        ]
        .into_iter()
        .collect();
        let indices: Vec<u16> = [0, 1, 2, 0, 2, 3].into_iter().collect();

        let vert_src = k9cmd.create_vertex_source(vertices, indices);
        let tex = k9cmd.create_texture_rgb8(self.texture_path.clone());
        let sh_vert = k9cmd.create_shader_builtin(BuiltInShader::TexQuadVert);
        let sh_frag = k9cmd.create_shader_builtin(BuiltInShader::TexQuadFrag);
        let program = k9cmd.create_shader_program([sh_vert, sh_frag].to_vec());
        let u_transform = k9cmd.create_uniform_link(program, "transform");

        self.core = Some(TexQuadCore {
            vert_src,
            tex,
            sh_vert,
            sh_frag,
            program,
            u_transform,
        })
    }

    fn render(&mut self, k9cmd: &mut GraphicsCommandInterface, screen_camera: &ScreenCamera) {
        if let Some(core) = &self.core {
            k9cmd.use_shader_program(core.program);
            k9cmd.bind_vertex_source(core.vert_src);
            k9cmd.bind_texture(core.tex, 0);

            k9cmd.upload_uniform_mat4(core.u_transform, screen_camera.view_proj_matrix());

            k9cmd.draw_elements(6);
        }
    }

    fn delete(&mut self, k9cmd: &mut GraphicsCommandInterface, _screen_camera: &ScreenCamera) {
        if let Some(core) = &self.core {
            k9cmd.delete_shader_program(core.program);
            k9cmd.delete_shader(core.sh_frag);
            k9cmd.delete_shader(core.sh_vert);
            k9cmd.delete_texture(core.tex);
            k9cmd.delete_vertex_source(core.vert_src);
        }
    }
}
