struct Frame {
    vp: mat4x4<f32>, previous_vp: mat4x4<f32>, inverse_vp: mat4x4<f32>, jitter: vec4<f32>,
    shadows: array<mat4x4<f32>,3>, eye: vec4<f32>, sun: vec4<f32>, sun_colour: vec4<f32>,
    ambient: vec4<f32>, sky: vec4<f32>, fog: vec4<f32>, grade: array<vec4<f32>,3>,
    params: vec4<f32>, clock: vec4<f32>, viewport: vec4<f32>, splits: vec4<f32>,
}
@group(0) @binding(0) var<uniform> f: Frame;
@group(1) @binding(0) var colour: texture_2d<f32>;
@group(1) @binding(1) var motion: texture_2d<f32>;
@group(1) @binding(2) var depth: texture_depth_2d;
@group(1) @binding(3) var history: texture_2d<f32>;
@group(1) @binding(4) var linear: sampler;
struct Full { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn fullscreen(@builtin(vertex_index) id: u32) -> Full {
    let uv=vec2(f32((id<<1u)&2u),f32(id&2u)); return Full(vec4(uv*vec2(2.,-2.)+vec2(-1.,1.),0.,1.),uv);
}
fn to_ycocg(c: vec3<f32>) -> vec3<f32> { return vec3(dot(c,vec3(0.25,0.5,0.25)),dot(c,vec3(0.5,0.,-0.5)),dot(c,vec3(-0.25,0.5,-0.25))); }
fn from_ycocg(c: vec3<f32>) -> vec3<f32> { return vec3(c.x+c.y-c.z,c.x+c.z,c.x-c.y-c.z); }
fn catmull_weights(t: f32) -> vec4<f32> { return vec4(-0.5*t+t*t-0.5*t*t*t,1.-2.5*t*t+1.5*t*t*t,0.5*t+2.*t*t-1.5*t*t*t,-0.5*t*t+0.5*t*t*t); }
fn history_catmull(uv: vec2<f32>) -> vec3<f32> {
    let size=vec2<f32>(textureDimensions(history));let p=uv*size-0.5;let base=floor(p);
    let wx=catmull_weights(p.x-base.x);let wy=catmull_weights(p.y-base.y);
    let x=vec3(base.x-1.,base.x+wx.z/(wx.y+wx.z),base.x+2.);
    let y=vec3(base.y-1.,base.y+wy.z/(wy.y+wy.z),base.y+2.);
    let weights_x=vec3(wx.x,wx.y+wx.z,wx.w);let weights_y=vec3(wy.x,wy.y+wy.z,wy.w);var c=vec3(0.);
    for(var j=0;j<3;j++) { for(var k=0;k<3;k++) { c+=textureSampleLevel(history,linear,(vec2(x[k],y[j])+0.5)/size,0.).rgb*weights_x[k]*weights_y[j]; }}
    return max(c,vec3(0.));
}
@fragment fn temporal_resolve(i: Full) -> @location(0) vec4<f32> {
    let size=vec2<i32>(textureDimensions(colour));let p=clamp(vec2<i32>((i.uv+f.jitter.zw)*vec2<f32>(size)),vec2(0),size-1);
    let now=textureSampleLevel(colour,linear,i.uv+f.jitter.zw,0.).rgb;
    var nearest=1.;var closest=p;var lo=vec3(1e30);var hi=-lo;var mean=vec3(0.);var square=vec3(0.);
    for(var y=-1;y<=1;y++) { for(var x=-1;x<=1;x++) {
        let q=clamp(p+vec2(x,y),vec2(0),size-1);let c=to_ycocg(textureLoad(colour,q,0).rgb);
        lo=min(lo,c);hi=max(hi,c);mean+=c;square+=c*c;let z=textureLoad(depth,q,0);if z<nearest { nearest=z;closest=q; }
    }}
    let z=textureLoad(depth,p,0);var linear_z=textureLoad(motion,p,0).w;
    if linear_z<=0. { linear_z=(0.1*800.)/(800.-z*(800.-0.1)); }
    let mv=textureLoad(motion,closest,0);let uv=i.uv-mv.xy;
    if f.clock.w<0.5 || any(uv<=vec2(0.)) || any(uv>=vec2(1.)) || z>=1. { return vec4(now,linear_z); }
    let old=vec4(history_catmull(uv),textureSampleLevel(history,linear,uv,0.).a);
    if abs(old.a-mv.z)>max(0.03,mv.z*0.02) { return vec4(now,linear_z); }
    mean/=9.;let sigma=sqrt(max(square/9.-mean*mean,vec3(0.)));
    let old_y=clamp(to_ycocg(old.rgb),max(lo,mean-1.25*sigma),min(hi,mean+1.25*sigma));
    let weight=mix(0.08,0.25,clamp(length(mv.xy*vec2<f32>(size))/20.,0.,1.));
    return vec4(mix(from_ycocg(old_y),now,weight),linear_z);
}
@fragment fn composite(i: Full) -> @location(0) vec4<f32> {
    var c=textureSampleLevel(colour,linear,i.uv,0.).rgb*f.params.x;
    c=vec3(dot(f.grade[0].xyz,c),dot(f.grade[1].xyz,c),dot(f.grade[2].xyz,c));
    if f.params.y>0.5 { c=clamp((c*(2.51*c+0.03))/(c*(2.43*c+0.59)+0.14),vec3(0.),vec3(1.)); }
    return vec4(c,1.);
}
