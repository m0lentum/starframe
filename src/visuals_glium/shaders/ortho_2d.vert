#version 330

in vec2 v_position;
uniform mat3 model_view;

void main() {
    vec3 viewed = model_view * vec3(v_position, 1.0);
    gl_Position = vec4(viewed.xy, 0.0, 1.0);
}