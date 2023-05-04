#version 330 core

in vec2 uv_coord;
out vec4 out_colour;

uniform sampler2D tex;

void main() {
    out_colour = texture(tex, uv_coord);
}