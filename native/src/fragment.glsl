#version 330 core
in vec3 n;
in vec3 material;
uniform bool selected;
out vec4 color;
uniform vec3 tint;
void main(){vec3 normal=normalize(n);if(!gl_FrontFacing)normal=-normal;float light=0.38+0.55*max(dot(normal,normalize(vec3(0.5,-0.6,1.0))),0.0)+0.15*max(dot(normal,normalize(vec3(-1.0,0.4,0.1))),0.0);color=vec4((selected?tint:material)*light,1.0);}
