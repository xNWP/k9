use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use uuid::Uuid;

use crate::{system::{FrameState, FirstCallState}, System, SystemCallbacks};

use super::{component::GraphicsComponent, renderer::RenderCommand, Vertex};

pub enum GraphicsCommand {
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
        filepath: PathBuf,
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
        filename: String,
    },
    CreateShaderBuiltIn {
        id: Uuid,
        shader: BuiltInShader,
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

pub struct GraphicsSystem {
    tracked: BTreeSet<Uuid>,
    graphics_commands: Vec<GraphicsCommand>,
    texture_store: TextureStore,
    shader_store: ShaderStore,
    shader_program_store: ShaderProgramStore,
}

type RealId = Uuid;
type RefId = Uuid;
struct TextureStore {
    ref_counts: BTreeMap<RealId, u32>,
    ref_real_map: BTreeMap<RefId, RealId>,
    path_real_map: BTreeMap<PathBuf, RealId>,
}
impl TextureStore {
    pub fn new() -> Self {
        Self {
            ref_counts: BTreeMap::new(),
            ref_real_map: BTreeMap::new(),
            path_real_map: BTreeMap::new(),
        }
    }
}

struct ShaderStore {
    ref_counts: BTreeMap<RealId, u32>,
    ref_real_map: BTreeMap<RefId, RealId>,
    name_real_map: BTreeMap<String, RealId>,
}
impl ShaderStore {
    pub fn new() -> Self {
        Self {
            ref_counts: BTreeMap::new(),
            ref_real_map: BTreeMap::new(),
            name_real_map: BTreeMap::new(),
        }
    }
}

struct ShaderProgramStore {
    ref_counts: BTreeMap<RealId, u32>,
    shaders_real_map: BTreeMap<BTreeSet<Uuid>, RealId>,
    ref_real_map: BTreeMap<RefId, RealId>,
}
impl ShaderProgramStore {
    pub fn new() -> Self {
        Self {
            ref_counts: BTreeMap::new(),
            shaders_real_map: BTreeMap::new(),
            ref_real_map: BTreeMap::new(),
        }
    }
}

impl GraphicsSystem {
    pub fn new() -> Self {
        Self {
            graphics_commands: Vec::new(),
            tracked: BTreeSet::new(),
            texture_store: TextureStore::new(),
            shader_store: ShaderStore::new(),
            shader_program_store: ShaderProgramStore::new(),
        }
    }

    pub fn get_render_commands(&mut self) -> Vec<RenderCommand> {
        let mut rval = Vec::new();

        let mut gfx_commands = Vec::new();
        gfx_commands.append(&mut self.graphics_commands);
        for cmd in gfx_commands {
            match cmd {
                GraphicsCommand::CreateVertexSource {
                    id,
                    vertices,
                    indices,
                } => {
                    rval.push(RenderCommand::CreateVertexSource {
                        id,
                        vertices,
                        indices,
                    });
                }
                GraphicsCommand::DeleteVertexSource { id } => {
                    rval.push(RenderCommand::DeleteVertexSource { id });
                }
                GraphicsCommand::BindVertexSource { id } => {
                    rval.push(RenderCommand::BindVertexSource { id })
                }
                GraphicsCommand::CreateTextureRGB8 { id, filepath } => {
                    if let Some(real_id) = self.texture_store.path_real_map.get(&filepath) {
                        self.texture_store.ref_real_map.insert(id, *real_id);
                        if let Some(rc) = self.texture_store.ref_counts.get_mut(real_id) {
                            *rc += 1;
                        } else {
                            log::error!("texture store corrupted on create rgb8");
                            continue;
                        }
                    } else {
                        let (pixels, dimensions) = match image::open(&filepath) {
                            Ok(x) => {
                                let dimensions = (x.width() as i32, x.height() as i32);
                                (x.into_rgb8().into_raw(), dimensions)
                            }
                            Err(e) => {
                                log::error!("couldn't open image {filepath:?}: {e}");
                                continue;
                            }
                        };

                        rval.push(RenderCommand::CreateTextureRGB8 {
                            id,
                            dimensions,
                            pixels,
                        });

                        self.texture_store.path_real_map.insert(filepath, id);
                        self.texture_store.ref_real_map.insert(id, id);
                        self.texture_store.ref_counts.insert(id, 1);
                    }
                }
                GraphicsCommand::BindTexture { id, texture_slot } => {
                    if let Some(real_id) = self.texture_store.ref_real_map.get(&id) {
                        rval.push(RenderCommand::BindTexture {
                            id: *real_id,
                            texture_slot,
                        });
                    } else {
                        log::error!("texture store corrupted on bind");
                        continue;
                    }
                }
                GraphicsCommand::DeleteTexture { id } => {
                    if let Some(real_id) = self.texture_store.ref_real_map.remove(&id) {
                        let mut mark_delete = false;
                        if let Some(ref_count) = self.texture_store.ref_counts.get_mut(&real_id) {
                            debug_assert!(*ref_count != 0);
                            *ref_count -= 1;

                            if *ref_count == 0 {
                                mark_delete = true;
                            }
                        } else {
                            log::error!("texture store corrupted on delete, get ref_count");
                            continue;
                        }

                        if mark_delete {
                            rval.push(RenderCommand::DeleteTexture { id: real_id });
                            self.texture_store.ref_counts.remove(&real_id);
                            self.texture_store.path_real_map = self
                                .texture_store
                                .path_real_map
                                .drain_filter(|_k, v| *v != real_id)
                                .collect();
                        }
                    }
                }
                GraphicsCommand::CreateShader {
                    id,
                    sh_type,
                    filename,
                } => {
                    if let Some(real_id) = self.shader_store.name_real_map.get(&filename) {
                        if let Some(ref_count) = self.shader_store.ref_counts.get_mut(&real_id) {
                            *ref_count += 1;
                        } else {
                            log::error!("shader store corrupted on create, get ref_count");
                        }
                        self.shader_store.ref_real_map.insert(id, *real_id);
                    } else {
                        let source = match std::fs::read_to_string(&filename) {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("couldn't read file '{filename}' to string: {e}");
                                continue;
                            }
                        };
                        rval.push(RenderCommand::CreateShader {
                            id,
                            sh_type,
                            source,
                        });

                        self.shader_store.name_real_map.insert(filename, id);
                        self.shader_store.ref_counts.insert(id, 1);
                        self.shader_store.ref_real_map.insert(id, id);
                    }
                }
                GraphicsCommand::CreateShaderBuiltIn { id, shader } => {
                    const CORE_FUNC: fn(
                        &mut GraphicsSystem,
                        ShaderType,
                        &'static str,
                        &'static str,
                        Uuid,
                        &mut Vec<RenderCommand>,
                    ) = |this: &mut GraphicsSystem,
                         sh_type: ShaderType,
                         name: &'static str,
                         source: &'static str,
                         id: Uuid,
                         rcmds: &mut Vec<RenderCommand>| {
                        if let Some(real_id) = this.shader_store.name_real_map.get(name) {
                            if let Some(ref_count) = this.shader_store.ref_counts.get_mut(real_id) {
                                *ref_count += 1;
                            } else {
                                log::error!("shader store corrupted on create, get ref_count");
                            }
                            this.shader_store.ref_real_map.insert(id, *real_id);
                        } else {
                            rcmds.push(RenderCommand::CreateShader {
                                id,
                                sh_type,
                                source: source.to_string(),
                            });

                            this.shader_store.name_real_map.insert(name.to_string(), id);
                            this.shader_store.ref_counts.insert(id, 1);
                            this.shader_store.ref_real_map.insert(id, id);
                        }
                    };

                    match shader {
                        BuiltInShader::TexQuadFrag => {
                            const SOURCE: &'static str =
                                include_str!("../../assets/shaders/k9_texquad.frag.glsl");
                            CORE_FUNC(
                                self,
                                ShaderType::Fragment,
                                "k9_built_in_texquad.frag.glsl",
                                SOURCE,
                                id,
                                &mut rval,
                            );
                        }
                        BuiltInShader::TexQuadVert => {
                            const SOURCE: &'static str =
                                include_str!("../../assets/shaders/k9_texquad.vert.glsl");
                            CORE_FUNC(
                                self,
                                ShaderType::Vertex,
                                "k9_built_in_texquad.vert.glsl",
                                SOURCE,
                                id,
                                &mut rval,
                            );
                        }
                    }
                }
                GraphicsCommand::DeleteShader { id } => {
                    if let Some(real_id) = self.shader_store.ref_real_map.remove(&id) {
                        let mut mark_delete = false;
                        if let Some(rc) = self.shader_store.ref_counts.get_mut(&real_id) {
                            debug_assert!(*rc != 0);
                            *rc -= 1;

                            if *rc == 0 {
                                mark_delete = true;
                            }
                        } else {
                            log::error!(
                                "shader_store corrupted in delete shader, ref_counts, id: {id}"
                            );
                        }

                        if mark_delete {
                            self.shader_store.ref_counts.remove(&real_id);
                            self.shader_store.name_real_map = self
                                .shader_store
                                .name_real_map
                                .drain_filter(|_k, v| *v != real_id)
                                .collect();

                            rval.push(RenderCommand::DeleteShader { id: real_id });
                        }
                    } else {
                        log::error!("couldn't find shader to delete with id: {id}");
                    }
                }
                GraphicsCommand::CreateShaderProgram { id, shader_ids } => {
                    let mut real_shader_ids = BTreeSet::new();
                    for ref_id in &shader_ids {
                        if let Some(real_id) = self.shader_store.ref_real_map.get(ref_id) {
                            real_shader_ids.insert(*real_id);
                        } else {
                            log::error!("shader_store corrupted in create shader program, get real from ref");
                        }
                    }

                    if let Some(real_id) = self
                        .shader_program_store
                        .shaders_real_map
                        .get(&real_shader_ids)
                    {
                        if let Some(rc) = self.shader_program_store.ref_counts.get_mut(real_id) {
                            *rc += 1;
                            self.shader_program_store.ref_real_map.insert(id, *real_id);
                        } else {
                            log::error!("shader_program_store corrupted in create shader program, get ref_count from real");
                        }
                    } else {
                        self.shader_program_store.ref_real_map.insert(id, id);
                        self.shader_program_store.ref_counts.insert(id, 1);
                        self.shader_program_store
                            .shaders_real_map
                            .insert(real_shader_ids, id);

                        rval.push(RenderCommand::CreateShaderProgram { id, shader_ids })
                    }
                }
                GraphicsCommand::DeleteShaderProgram { id } => {
                    if let Some(real_id) = self.shader_program_store.ref_real_map.remove(&id) {
                        let mut mark_delete = false;
                        if let Some(rc) = self.shader_program_store.ref_counts.get_mut(&real_id) {
                            debug_assert!(*rc != 0);
                            *rc -= 1;
                            if *rc == 0 {
                                mark_delete = true;
                            }
                        } else {
                            log::error!("shader_program_store corrupted in delete shader program, get ref_count");
                        }

                        if mark_delete {
                            self.shader_program_store.ref_counts.remove(&real_id);
                            self.shader_program_store.shaders_real_map = self
                                .shader_program_store
                                .shaders_real_map
                                .drain_filter(|_k, v| *v != real_id)
                                .collect();

                            rval.push(RenderCommand::DeleteShaderProgram { id: real_id });
                        }
                    } else {
                        log::error!("couldn't find shader program to delete with id: {id}");
                    }
                }
                GraphicsCommand::UseShaderProgram { id } => {
                    if let Some(real_id) = self.shader_program_store.ref_real_map.get(&id) {
                        rval.push(RenderCommand::UseShaderProgram { id: *real_id })
                    } else {
                        log::error!("couldn't get shader program id to use: {id}");
                    }
                }
                GraphicsCommand::DrawElements { count } => {
                    rval.push(RenderCommand::DrawElements { count });
                }
                GraphicsCommand::CreateUniformLink {
                    new_uniform_id,
                    existing_program_id,
                    uniform_name,
                } => {
                    rval.push(RenderCommand::CreateUniformLink {
                        new_uniform_id,
                        existing_program_id,
                        uniform_name,
                    });
                }
                GraphicsCommand::UploadUniformMat4 { id, data } => {
                    rval.push(RenderCommand::UploadUniformMat4 { id, data });
                }
            }
        }

        rval
    }
}
impl System for GraphicsSystem {
    const UUID: Uuid = uuid::uuid!("adcc866b-a2a4-4225-aa97-8aec4cc81107");
}
impl SystemCallbacks for GraphicsSystem {
    fn first_call(&mut self, _first_call_state: FirstCallState, _state: FrameState) {}
    fn update(&mut self, state: FrameState) {
        let ents = state.ents;

        // generate render commands
        let mut k9cmd = GraphicsCommandInterface::new();

        // get delete entities
        if let Some(delete_ents) = ents.get_by_component_delete_mut::<GraphicsComponent>() {
            for (d_id, d_ent) in delete_ents {
                if let Some(gfx_comp) = d_ent.get_component_mut::<GraphicsComponent>() {
                    gfx_comp.delete(&mut k9cmd, &state.screen_camera);
                }
                self.tracked.remove(&d_id);
            }
        }

        // get add entities
        if let Some(mut gfx_ents) = ents.get_by_component_mut::<GraphicsComponent>() {
            for (n_id, n_ent) in &mut gfx_ents {
                if !self.tracked.contains(n_id) {
                    self.tracked.insert(*n_id);
                    if let Some(gfx_comp) = n_ent.get_component_mut::<GraphicsComponent>() {
                        gfx_comp.create(&mut k9cmd, &state.screen_camera);
                    }
                }
            }

            // call render on survivors
            for (_, gfx_ent) in gfx_ents {
                if let Some(gfx_comp) = gfx_ent.get_component_mut::<GraphicsComponent>() {
                    gfx_comp.render(&mut k9cmd, &state.screen_camera);
                }
            }
        }

        self.graphics_commands.append(&mut k9cmd.into_raw());
    }

    fn exiting(&mut self, _state: FrameState) {}
}

#[derive(Debug)]
pub enum ShaderType {
    Vertex,
    Fragment,
    Compute,
    Geometry,
}
impl Into<u32> for ShaderType {
    fn into(self) -> u32 {
        match self {
            Self::Vertex => glow::VERTEX_SHADER,
            Self::Fragment => glow::FRAGMENT_SHADER,
            Self::Compute => glow::COMPUTE_SHADER,
            Self::Geometry => glow::GEOMETRY_SHADER,
        }
    }
}

pub enum BuiltInShader {
    TexQuadVert,
    TexQuadFrag,
}

pub struct GraphicsCommandInterface {
    cmds: Vec<GraphicsCommand>,
}
impl GraphicsCommandInterface {
    pub fn new() -> Self {
        Self { cmds: Vec::new() }
    }

    pub fn into_raw(self) -> Vec<GraphicsCommand> {
        self.cmds
    }

    pub fn create_vertex_source(&mut self, vertices: Vec<Vertex>, indices: Vec<u16>) -> Uuid {
        let id = Uuid::new_v4();
        self.cmds.push(GraphicsCommand::CreateVertexSource {
            id,
            vertices,
            indices,
        });
        id
    }
    pub fn bind_vertex_source(&mut self, id: Uuid) {
        self.cmds.push(GraphicsCommand::BindVertexSource { id });
    }
    pub fn delete_vertex_source(&mut self, id: Uuid) {
        self.cmds.push(GraphicsCommand::DeleteVertexSource { id });
    }

    pub fn create_texture_rgb8(&mut self, filepath: PathBuf) -> Uuid {
        let id = Uuid::new_v4();
        self.cmds
            .push(GraphicsCommand::CreateTextureRGB8 { id, filepath });
        id
    }
    pub fn bind_texture(&mut self, id: Uuid, texture_slot: u8) {
        self.cmds
            .push(GraphicsCommand::BindTexture { id, texture_slot });
    }
    pub fn delete_texture(&mut self, id: Uuid) {
        self.cmds.push(GraphicsCommand::DeleteTexture { id });
    }

    pub fn create_shader(&mut self, sh_type: ShaderType, filename: impl ToString) -> Uuid {
        let id = Uuid::new_v4();
        self.cmds.push(GraphicsCommand::CreateShader {
            id,
            sh_type,
            filename: filename.to_string(),
        });
        id
    }
    pub fn create_shader_builtin(&mut self, shader: BuiltInShader) -> Uuid {
        let id = Uuid::new_v4();
        self.cmds
            .push(GraphicsCommand::CreateShaderBuiltIn { id, shader });
        id
    }

    pub fn delete_shader(&mut self, id: Uuid) {
        self.cmds.push(GraphicsCommand::DeleteShader { id });
    }

    pub fn create_shader_program(&mut self, shader_ids: Vec<Uuid>) -> Uuid {
        let id = Uuid::new_v4();
        self.cmds
            .push(GraphicsCommand::CreateShaderProgram { id, shader_ids });
        id
    }
    pub fn delete_shader_program(&mut self, id: Uuid) {
        self.cmds.push(GraphicsCommand::DeleteShaderProgram { id });
    }
    pub fn use_shader_program(&mut self, id: Uuid) {
        self.cmds.push(GraphicsCommand::UseShaderProgram { id });
    }

    pub fn draw_elements(&mut self, count: u32) {
        self.cmds.push(GraphicsCommand::DrawElements { count });
    }

    pub fn create_uniform_link(&mut self, program_id: Uuid, name: impl ToString) -> Uuid {
        let id = Uuid::new_v4();
        self.cmds.push(GraphicsCommand::CreateUniformLink {
            new_uniform_id: id,
            existing_program_id: program_id,
            uniform_name: name.to_string(),
        });
        id
    }

    pub fn upload_uniform_mat4(&mut self, id: Uuid, data: glam::Mat4) {
        self.cmds
            .push(GraphicsCommand::UploadUniformMat4 { id, data });
    }
}
