#version 450

layout(location = 0) in vec4 vert_color;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = vert_color;
}