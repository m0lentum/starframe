#version 450

layout(location = 0) in vec2 a_position;
layout(location = 1) in vec4 a_color;

layout(location = 0) out vec4 v_color;

layout(set = 0, binding = 0) uniform Globals {
    mat3 u_view;
};

void main() {
    v_color = a_color;
    vec3 viewed = u_view * vec3(a_position, 1.0);
    gl_Position = vec4(viewed.xy, 0.0, 1.0);
}