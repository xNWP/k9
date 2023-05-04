use std::collections::BTreeMap;

use bytemuck::{offset_of, Pod, Zeroable};
use egui::{epaint::Primitive, PaintCallbackInfo, Painter};
use glow::HasContext;
use sdl2::{
    video::{GLContext, Window},
    EventPump, Sdl, VideoSubsystem,
};
use std::fmt::Display;
use uuid::Uuid;

use super::{system::ShaderType, Vertex};

pub struct K9Renderer {
    vertex_sources: BTreeMap<Uuid, VertexSource>,
    texture_sources: BTreeMap<Uuid, glow::NativeTexture>,
    shader_sources: BTreeMap<Uuid, glow::NativeShader>,
    shader_program_sources: BTreeMap<Uuid, glow::NativeProgram>,
    uniform_links: BTreeMap<Uuid, glow::NativeUniformLocation>,
    exit_called: bool,
    sdl_events: Vec<sdl2::event::Event>,
}

impl K9Renderer {
    pub fn new(glow: &glow::Context) -> Result<Self, String> {
        Ok(Self {
            vertex_sources: BTreeMap::new(),
            texture_sources: BTreeMap::new(),
            shader_sources: BTreeMap::new(),
            shader_program_sources: BTreeMap::new(),
            uniform_links: BTreeMap::new(),
            exit_called: false,
            sdl_events: Vec::new(),
        })
    }

    pub fn exit_called(&self) -> bool {
        self.exit_called
    }

    pub fn render(&mut self, glow: &glow::Context, cmds: Vec<RenderCommand>) {
        // draw code
        unsafe {
            glow.clear_color(0.2, 0.3, 0.3, 1.0);
            glow.clear(glow::COLOR_BUFFER_BIT);

            'render_command_loop: for cmd in cmds {
                match cmd {
                    RenderCommand::CreateVertexSource {
                        id,
                        vertices,
                        indices,
                    } => {
                        if self.vertex_sources.contains_key(&id) {
                            log::error!("request for unique vao with duplicate id: {id}");
                            continue;
                        }

                        let vao = match glow.create_vertex_array() {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("error creating vertex array object: {e}");
                                continue;
                            }
                        };
                        glow.bind_vertex_array(Some(vao));

                        let vbo = match glow.create_buffer() {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("error creating vertex array buffer object: {e}");
                                continue;
                            }
                        };
                        glow.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                        glow.buffer_data_u8_slice(
                            glow::ARRAY_BUFFER,
                            bytemuck::cast_slice(vertices.as_slice()),
                            glow::STATIC_DRAW,
                        );

                        let ebo = match glow.create_buffer() {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("error creating element array buffer object: {e}");
                                continue;
                            }
                        };
                        glow.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
                        glow.buffer_data_u8_slice(
                            glow::ELEMENT_ARRAY_BUFFER,
                            bytemuck::cast_slice(indices.as_slice()),
                            glow::STATIC_DRAW,
                        );

                        glow.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 20, 0);
                        glow.enable_vertex_attrib_array(0);

                        glow.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 20, 12);
                        glow.enable_vertex_attrib_array(1);

                        let vert_src = VertexSource { ebo, vao, vbo };
                        self.vertex_sources.insert(id, vert_src);
                    }
                    RenderCommand::BindVertexSource { id } => {
                        if let Some(vert_src) = self.vertex_sources.get(&id) {
                            glow.bind_vertex_array(Some(vert_src.vao));
                        } else {
                            log::error!("bind couldn't find vertex source with id: {id}");
                        }
                    }
                    RenderCommand::DeleteVertexSource { id } => {
                        if let Some(vert_src) = self.vertex_sources.remove(&id) {
                            glow.delete_vertex_array(vert_src.vao);
                            glow.delete_buffer(vert_src.vbo);
                            glow.delete_buffer(vert_src.ebo);
                        } else {
                            log::error!("delete couldn't find vertex source with id: {id}");
                        }
                    }
                    RenderCommand::CreateTextureRGB8 {
                        id,
                        pixels,
                        dimensions,
                    } => {
                        if self.texture_sources.contains_key(&id) {
                            log::error!(
                                "request for unique texture source with duplicate id: {id}"
                            );
                            continue;
                        }

                        let tex = match glow.create_texture() {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("couldn't create texture: {e}");
                                continue;
                            }
                        };
                        glow.bind_texture(glow::TEXTURE_2D, Some(tex));
                        glow.tex_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            glow::RGB8 as i32,
                            dimensions.0,
                            dimensions.1,
                            0,
                            glow::RGB,
                            glow::UNSIGNED_BYTE,
                            Some(pixels.as_slice()),
                        );
                        self.texture_sources.insert(id, tex);
                    }
                    RenderCommand::BindTexture { id, texture_slot } => {
                        if let Some(tex) = self.texture_sources.get(&id) {
                            glow.active_texture(glow::TEXTURE0 + texture_slot as u32);
                            glow.bind_texture(glow::TEXTURE_2D, Some(*tex));
                        } else {
                            log::error!("couldn't find texture to bind with id: {id}");
                            continue;
                        }
                    }
                    RenderCommand::DeleteTexture { id } => {
                        if let Some(tex) = self.texture_sources.remove(&id) {
                            glow.delete_texture(tex);
                        } else {
                            log::error!("couldn't find texture to delete with id: {id}");
                        }
                    }
                    RenderCommand::CreateShader {
                        id,
                        source,
                        sh_type,
                    } => {
                        let shader = match glow.create_shader(sh_type.into()) {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("failed to create shader: {e}");
                                continue;
                            }
                        };

                        glow.shader_source(shader, &source);
                        glow.compile_shader(shader);

                        if !glow.get_shader_compile_status(shader) {
                            let err = glow.get_shader_info_log(shader);
                            glow.delete_shader(shader);
                            log::error!("shader compile error: {err}");
                            continue;
                        }

                        self.shader_sources.insert(id, shader);
                    }
                    RenderCommand::DeleteShader { id } => {
                        if let Some(shader) = self.shader_sources.remove(&id) {
                            glow.delete_shader(shader);
                        } else {
                            log::error!("couldn't find shader to delete with id: {id}");
                        }
                    }
                    RenderCommand::CreateShaderProgram { id, shader_ids } => {
                        let program = match glow.create_program() {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("couldn't create shader program: {e}");
                                continue;
                            }
                        };

                        let mut shaders = Vec::new();
                        for sh_id in shader_ids {
                            let shader = match self.shader_sources.get(&sh_id) {
                                Some(x) => x,
                                None => {
                                    log::error!("couldn't get shader: {id}");
                                    continue 'render_command_loop;
                                }
                            };
                            glow.attach_shader(program, *shader);
                            shaders.push(shader);
                        }

                        glow.link_program(program);

                        for shader in shaders {
                            glow.detach_shader(program, *shader)
                        }

                        if !glow.get_program_link_status(program) {
                            let err = glow.get_program_info_log(program);
                            log::error!("couldn't link program with id '{id}': {err}");
                            continue;
                        }

                        self.shader_program_sources.insert(id, program);
                    }
                    RenderCommand::DeleteShaderProgram { id } => {
                        if let Some(program) = self.shader_program_sources.remove(&id) {
                            glow.delete_program(program);
                        } else {
                            log::error!("couldn't find shader program to delete with id: {id}");
                            continue;
                        }
                    }
                    RenderCommand::UseShaderProgram { id } => {
                        if let Some(program) = self.shader_program_sources.get(&id) {
                            glow.use_program(Some(*program));
                        }
                    }
                    RenderCommand::DrawElements { count } => {
                        glow.draw_elements(glow::TRIANGLES, count as i32, glow::UNSIGNED_SHORT, 0);
                    }
                    RenderCommand::CreateUniformLink {
                        new_uniform_id,
                        existing_program_id,
                        uniform_name,
                    } => {
                        if let Some(program) = self.shader_program_sources.get(&existing_program_id)
                        {
                            let loc = match glow.get_uniform_location(*program, &uniform_name) {
                                Some(x) => x,
                                None => {
                                    log::error!("couldn't find uniform with name '{uniform_name}' on program '{existing_program_id}'");
                                    continue;
                                }
                            };

                            self.uniform_links.insert(new_uniform_id, loc);
                        } else {
                            log::error!("couldn't find shader program for CreateUniformLink with id: {existing_program_id}");
                            continue;
                        }
                    }
                    RenderCommand::UploadUniformMat4 { id, data } => {
                        if let Some(loc) = self.uniform_links.get(&id) {
                            glow.uniform_matrix_4_f32_slice(
                                Some(loc),
                                false,
                                &data.to_cols_array(),
                            );
                        } else {
                            log::error!("couldn't find uniform location by id: {id}");
                            continue;
                        }
                    }
                }
            }
        }
    }
}

pub enum RenderCommand {
    CreateVertexSource {
        id: Uuid,
        vertices: Vec<Vertex>,
        indices: Vec<u16>,
    },
    BindVertexSource {
        id: Uuid,
    },
    DeleteVertexSource {
        id: Uuid,
    },
    CreateTextureRGB8 {
        id: Uuid,
        dimensions: (i32, i32),
        pixels: Vec<u8>,
    },
    BindTexture {
        id: Uuid,
        texture_slot: u8,
    },
    DeleteTexture {
        id: Uuid,
    },
    CreateShader {
        id: Uuid,
        sh_type: ShaderType,
        source: String,
    },
    DeleteShader {
        id: Uuid,
    },
    CreateShaderProgram {
        id: Uuid,
        shader_ids: Vec<Uuid>,
    },
    DeleteShaderProgram {
        id: Uuid,
    },
    UseShaderProgram {
        id: Uuid,
    },
    DrawElements {
        count: u32,
    },
    CreateUniformLink {
        new_uniform_id: Uuid,
        existing_program_id: Uuid,
        uniform_name: String,
    },
    UploadUniformMat4 {
        id: Uuid,
        data: glam::Mat4,
    },
}
impl Display for RenderCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RenderCommand::")?;
        match self {
            Self::CreateVertexSource { id, vertices, indices } => write!(f, "CreateVertexSource {{ id: {id}, {} vertices, {} indices }}", vertices.len(), indices.len()),
            Self::BindVertexSource { id } => write!(f, "BindVertexSource {{ id: {id} }}"),
            Self::DeleteVertexSource { id } => write!(f, "DeleteVertexSource {{ id: {id} }}"),
            Self::CreateTextureRGB8 { id, dimensions, pixels } => write!(f, "CreateTextureRGB8 {{ id: {id}, {}x{}, {} bytes }}", dimensions.0, dimensions.1, pixels.len()),
            Self::BindTexture { id, texture_slot } => write!(f, "BindTexture {{ id: {id}, slot: {texture_slot} }}"),
            Self::DeleteTexture { id } => write!(f, "DeleteTexture {{ id: {id} }}"),
            Self::CreateShader { id, sh_type, source } => write!(f, "CreateShader {{ id: {id}, shader_type: {sh_type:?}, {} byte source }}", source.len()),
            Self::DeleteShader { id } => write!(f, "DeleteShader {{ id: {id} }}"),
            Self::CreateShaderProgram { id, shader_ids } => write!(f, "CreateShaderProgram {{ id: {id}, shader_ids: {shader_ids:?} }}"),
            Self::DeleteShaderProgram { id } => write!(f, "DeleteShaderProgram {{ id: {id} }}"),
            Self::UseShaderProgram { id } => write!(f, "UseShaderProgram {{ id: {id} }}"),
            Self::DrawElements{ count } => write!(f, "DrawElements{{ count: {count} }}"),
            Self::CreateUniformLink { new_uniform_id, existing_program_id, uniform_name } => write!(f, "CreateUniformLink {{ new_uniform_id: {new_uniform_id}, existing_program_id: {existing_program_id}, uniform_name: {uniform_name} }}"),
            Self::UploadUniformMat4 { id, data } => write!(f, "UploadUniformMat4 {{ id: {id}, data: {data} }}"),
        }
    }
}

pub struct VertexSource {
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,
    ebo: glow::NativeBuffer,
}
