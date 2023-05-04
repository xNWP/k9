#version 330 core
layout (location = 0) in vec3 pos;
layout (location = 1) in vec2 uv;

uniform mat4 transform;
out vec2 uv_coord;

void main() {
    gl_Position = transform * vec4(pos.xyz, 1.0f);
    uv_coord = uv;
}