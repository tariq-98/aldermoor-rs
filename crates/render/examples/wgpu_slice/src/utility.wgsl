@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear: sampler;
@group(0) @binding(2) var<uniform> params: vec4<f32>;
struct Full { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn fullscreen(@builtin(vertex_index) id: u32) -> Full {
    let uv=vec2(f32((id<<1u)&2u),f32(id&2u));return Full(vec4(uv*vec2(2.,-2.)+vec2(-1.,1.),0.,1.),uv);
}
@fragment fn mip(i: Full) -> @location(0) vec4<f32> { return textureSampleLevel(source,linear,i.uv,0.); }
@fragment fn prefilter(i: Full) -> @location(0) vec4<f32> {
    let pi=3.141592653589793;let phi=(i.uv.x-0.5)*2.*pi;let theta=i.uv.y*pi;
    let n=vec3(cos(phi)*sin(theta),cos(theta),sin(phi)*sin(theta));
    let tangent=normalize(cross(select(vec3(1.,0.,0.),vec3(0.,1.,0.),abs(n.y)<0.999),n));let bitangent=cross(n,tangent);
    var sum=vec3(0.);var weight=0.;let a=max(params.x*params.x,0.001);
    for(var k=0u;k<128u;k++) {
        let xi=vec2(f32(k)/128.,f32(reverseBits(k))*2.3283064365386963e-10);
        let phi_h=2.*pi*xi.x;let cos_h=sqrt((1.-xi.y)/(1.+(a*a-1.)*xi.y));let sin_h=sqrt(max(0.,1.-cos_h*cos_h));
        let h=tangent*cos(phi_h)*sin_h+bitangent*sin(phi_h)*sin_h+n*cos_h;let l=reflect(-n,h);
        let nl=max(dot(n,l),0.);let uv=vec2(atan2(l.z,l.x)/(2.*pi)+0.5,acos(clamp(l.y,-1.,1.))/pi);
        sum+=textureSampleLevel(source,linear,uv,0.).rgb*nl;weight+=nl;
    }
    return vec4(sum/max(weight,0.001),1.);
}
