#version 330 core
layout(location=0) in vec3 position;
layout(location=1) in vec3 normal;
layout(location=2) in vec3 bodyColor;
uniform mat4 mvp;
out vec3 n;
out vec3 material;
void main(){n=normal;material=bodyColor;gl_Position=mvp*vec4(position,1.0);}
