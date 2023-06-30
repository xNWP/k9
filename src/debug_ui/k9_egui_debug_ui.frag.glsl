#version 330 core

uniform sampler2D u_sampler;

in vec4 v_rgba_in_gamma;
in vec2 v_tc;

void main() {
    vec4 texture_in_gamma = texture2D(u_sampler, v_tc);
    gl_FragColor = v_rgba_in_gamma * texture_in_gamma;
}