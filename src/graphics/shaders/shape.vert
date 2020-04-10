#version 450

layout(location = 0) in vec2 v_position;
layout(location = 1) in vec4 v_color;

layout(set = 0, binding = 0) uniform Matrices {
    mat3 view;
};

layout(location = 0) out vec4 vert_color;

void main() {
    vec3 viewed = view * vec3(v_position, 1.0);
    gl_Position = vec4(viewed.xy, 0.0, 1.0);
}