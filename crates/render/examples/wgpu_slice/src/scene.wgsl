diagnostic(off, derivative_uniformity);
// Mechanical port of the rigid-scene path in src/shaders.metal. All look values are uniforms.
struct Frame {
    vp: mat4x4<f32>, previous_vp: mat4x4<f32>, inverse_vp: mat4x4<f32>, jitter: vec4<f32>,
    shadows: array<mat4x4<f32>,3>, eye: vec4<f32>, sun: vec4<f32>, sun_colour: vec4<f32>,
    ambient: vec4<f32>, sky: vec4<f32>, fog: vec4<f32>, grade: array<vec4<f32>,3>,
    params: vec4<f32>, clock: vec4<f32>, viewport: vec4<f32>, splits: vec4<f32>,
}
struct Surface {
    bounds: vec4<f32>, scales: vec4<f32>, roughness: vec4<f32>, water_deep: vec4<f32>,
    water_shallow: vec4<f32>, water_absorption: vec4<f32>, water_flow: vec4<f32>, sky: vec4<f32>, flags: vec4<u32>,
}
struct Instance { model: mat4x4<f32>, base: vec4<f32>, emission: vec4<f32>, params: vec4<f32>, flags: vec4<u32> }
@group(0) @binding(0) var<uniform> f: Frame;
@group(0) @binding(1) var<uniform> surface: Surface;
@group(0) @binding(2) var shadow: texture_depth_2d_array;
@group(0) @binding(3) var comparison: sampler_comparison;
@group(0) @binding(4) var terrain_colour: texture_2d_array<f32>;
@group(0) @binding(5) var terrain_normal: texture_2d_array<f32>;
@group(0) @binding(6) var terrain_weights: texture_2d<f32>;
@group(0) @binding(7) var water_normal: texture_2d<f32>;
@group(0) @binding(8) var sky_map: texture_2d<f32>;
@group(0) @binding(9) var sky_reflection: texture_2d<f32>;
@group(0) @binding(10) var repeat_sampler: sampler;
@group(0) @binding(11) var clamp_sampler: sampler;
@group(0) @binding(12) var environment_sampler: sampler;
@group(1) @binding(0) var<storage,read> instances: array<Instance>;
@group(1) @binding(1) var base_map: texture_2d<f32>;
@group(1) @binding(2) var normal_map: texture_2d<f32>;
@group(1) @binding(3) var orm_map: texture_2d<f32>;
@group(1) @binding(4) var emission_map: texture_2d<f32>;
@group(1) @binding(5) var occlusion_map: texture_2d<f32>;
@group(2) @binding(0) var<uniform> cascade: vec4<u32>;
const PI: f32 = 3.141592653589793;
struct Vertex {
    @location(0) position: vec4<f32>, @location(1) normal: vec4<f32>,
    @location(2) tangent: vec4<f32>, @location(3) color: vec4<f32>, @location(4) uv: vec4<f32>,
}
struct Vary {
    @builtin(position) position: vec4<f32>, @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>, @location(2) tangent: vec4<f32>, @location(3) color: vec4<f32>,
    @location(4) uv: vec2<f32>, @location(5) current_clip: vec4<f32>, @location(6) previous_clip: vec4<f32>,
    @location(7) water_depth: f32, @location(8) @interpolate(flat) instance: u32,
}
fn wind(p: vec3<f32>, weight: f32, time: f32) -> vec3<f32> {
    let phase=dot(p.xz,vec2(0.35,0.27));
    let gust=sin(time*0.9+phase)*0.6+sin(time*2.3+phase*1.7)*0.25;
    let flutter=sin(time*7.+phase*5.)*0.08;
    return p+normalize(vec3(1.,0.,0.35))*weight*f.clock.z*(gust+flutter)*0.12;
}
fn deform(a: Vertex, id: u32) -> Vary {
    let d=instances[id];
    let rest=(d.model*vec4(a.position.xyz,1.)).xyz;
    let world=wind(rest,a.normal.w,f.clock.x);
    var o: Vary;
    o.world=world; o.current_clip=f.vp*vec4(world,1.);
    o.previous_clip=f.previous_vp*vec4(wind(rest,a.normal.w,f.clock.y),1.);
    o.position=o.current_clip; o.position.x+=f.jitter.x*o.position.w; o.position.y+=f.jitter.y*o.position.w;
    o.normal=normalize((d.model*vec4(a.normal.xyz,0.)).xyz);
    o.tangent=vec4((d.model*vec4(a.tangent.xyz,0.)).xyz,a.tangent.w);
    o.color=a.color; o.uv=a.uv.xy; o.water_depth=a.uv.z; o.instance=id; return o;
}
@vertex fn scene_vertex(a: Vertex, @builtin(instance_index) id: u32) -> Vary { return deform(a,id); }
@vertex fn shadow_vertex(a: Vertex, @builtin(instance_index) id: u32) -> Vary {
    var o=deform(a,id); o.position=f.shadows[cascade.x]*vec4(o.world,1.); return o;
}
@fragment fn shadow_fragment(i: Vary) {
    let d=instances[i.instance];
    if d.flags.x==1u && textureSample(base_map,repeat_sampler,i.uv).a*d.base.a*i.color.a<d.params.w { discard; }
}
fn sample_cascade(world: vec3<f32>, layer: u32, bias: f32) -> f32 {
    let clip=f.shadows[layer]*vec4(world,1.); let p=clip.xyz/clip.w; let uv=p.xy*vec2(0.5,-0.5)+0.5;
    if any(uv<vec2(0.)) || any(uv>vec2(1.)) || p.z<0. || p.z>1. { return 1.; }
    var v=0.;
    for(var y=-1; y<=1; y+=2) { for(var x=-1; x<=1; x+=2) {
        v+=textureSampleCompareLevel(shadow,comparison,uv+vec2(f32(x),f32(y))*0.5*f.splits.w,i32(layer),p.z-bias);
    }}
    return v*0.25;
}
fn shadow_visibility(p: vec3<f32>, depth: f32, nl: f32) -> f32 {
    let layer=select(select(2u,1u,depth<f.splits.y),0u,depth<f.splits.x);
    if depth>f.splits.z { return 1.; }
    let bias=max(0.0001,0.0006*(1.-nl)); var v=sample_cascade(p,layer,bias); let end=f.splits[layer];
    if layer<2u && depth>end*0.88 { v=mix(v,sample_cascade(p,layer+1u,bias),smoothstep(end*0.88,end,depth)); }
    return v;
}
struct TerrainSample { colour: vec3<f32>, perturbation: vec3<f32> }
fn terrain_layer(position: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>, weights: vec3<f32>, layer: u32) -> TerrainSample {
    let uv=array<vec2<f32>,3>(vec2(position.z,-position.y),position.xz,vec2(position.x,-position.y));
    let gx=array<vec2<f32>,3>(vec2(dx.z,-dx.y),dx.xz,vec2(dx.x,-dx.y));
    let gy=array<vec2<f32>,3>(vec2(dy.z,-dy.y),dy.xz,vec2(dy.x,-dy.y));
    var result: TerrainSample;
    for(var axis=0u;axis<3u;axis++) {
        if weights[axis]<0.001 { continue; }
        result.colour+=textureSampleGrad(terrain_colour,repeat_sampler,uv[axis],i32(layer),gx[axis],gy[axis]).rgb*weights[axis];
        let nm=textureSampleGrad(terrain_normal,repeat_sampler,uv[axis],i32(layer),gx[axis],gy[axis]).rgb*2.-1.;
        let slope=nm.xy/max(nm.z,0.1);
        var perturb=vec3(slope.x,slope.y,0.);
        if axis==0u { perturb=vec3(0.,slope.y,slope.x); }
        if axis==1u { perturb=vec3(slope.x,0.,-slope.y); }
        result.perturbation+=perturb*weights[axis];
    }
    return result;
}
struct SceneOutput { @location(0) colour: vec4<f32>, @location(1) motion: vec4<f32> }
@fragment fn scene_fragment(i: Vary, @builtin(front_facing) front: bool) -> SceneOutput {
    let d=instances[i.instance];
    var albedo=textureSample(base_map,repeat_sampler,i.uv)*d.base;
    var surface_normal=vec3(0.,0.,1.); var roughness=d.emission.w;
    let is_terrain=d.flags.y==1u && surface.flags.x>0u;
    var perturbation=vec3(0.); let dx=dpdx(i.world); let dy=dpdy(i.world);
    if is_terrain {
        let uv=clamp((i.world.xz-surface.bounds.xy)/(surface.bounds.zw-surface.bounds.xy),vec2(0.),vec2(1.));
        var weights=max(textureSampleLevel(terrain_weights,clamp_sampler,uv,0.),vec4(0.));
        for(var layer=surface.flags.x;layer<4u;layer++) { weights[layer]=0.; }
        let sum=dot(weights,vec4(1.)); weights=select(vec4(1.,0.,0.,0.),weights/max(sum,0.0001),sum>0.0001);
        var projections=pow(abs(normalize(i.normal)),vec3(4.)); projections/=max(dot(projections,vec3(1.)),0.0001);
        albedo=vec4(0.,0.,0.,albedo.a); roughness=0.;
        for(var layer=0u;layer<surface.flags.x;layer++) {
            if weights[layer]==0. { continue; }
            let sample=terrain_layer(i.world*surface.scales[layer],dx*surface.scales[layer],dy*surface.scales[layer],projections,layer);
            albedo=vec4(albedo.rgb+sample.colour*weights[layer],albedo.a);
            perturbation+=sample.perturbation*weights[layer]; roughness+=surface.roughness[layer]*weights[layer];
        }
    }
    if d.flags.y==2u && surface.flags.y>0u {
        let uv=(i.world.xz+f.clock.x*surface.water_flow.yz)*surface.water_flow.x;
        surface_normal=textureSample(water_normal,repeat_sampler,uv).rgb*2.-1.;
        surface_normal=vec3(surface_normal.xy*surface.water_shallow.w,surface_normal.z);
        let transmission=exp(-surface.water_absorption.rgb*max(i.water_depth,0.));
        albedo=vec4(mix(surface.water_deep.rgb,surface.water_shallow.rgb,transmission),surface.water_deep.w);
        roughness=surface.water_flow.w;
    }
    albedo*=i.color; if d.flags.x==1u && albedo.a<d.params.w { discard; }
    var n=normalize(i.normal)*select(1.,-1.,d.flags.z!=0u && !front); var t=i.tangent.xyz; var b=vec3(0.);
    if dot(t,t)>0.1 { t=normalize(t-n*dot(n,t)); b=cross(n,t)*i.tangent.w; }
    else {
        let du=dpdx(i.uv);let dv=dpdy(i.uv);let det=du.x*dv.y-du.y*dv.x;
        if abs(det)>0.000001 { t=normalize((dx*dv.y-dy*du.y)/det); t=normalize(t-n*dot(t,n)); b=normalize((dx*dv.x-dy*du.x)/det); }
        else { t=normalize(cross(select(vec3(1.,0.,0.),vec3(0.,1.,0.),abs(n.y)<0.99),n));b=cross(n,t); }
    }
    if is_terrain { perturbation*=d.params.y; n=normalize(n+perturbation-n*dot(n,perturbation)); }
    else {
        var nm=textureSample(normal_map,repeat_sampler,i.uv).rgb*2.-1.;
        if d.flags.y==2u && surface.flags.y>0u { nm=surface_normal; }
        nm=vec3(nm.xy*d.params.y,nm.z); n=normalize(t*nm.x+b*nm.y+n*nm.z);
    }
    let mr=textureSample(orm_map,repeat_sampler,i.uv).rgb;let rough=max(0.08,roughness*mr.g);let metal=d.params.x*mr.b;
    let ao=mix(1.,textureSample(occlusion_map,repeat_sampler,i.uv).r,d.params.z);
    let l=normalize(f.sun.xyz);let v=normalize(f.eye.xyz-i.world);let h=normalize(l+v);
    let nl=max(dot(n,l),0.);let nv=max(dot(n,v),0.001);let nh=max(dot(n,h),0.);let vh=max(dot(v,h),0.);
    let a2=pow(rough,4.);let den=nh*nh*(a2-1.)+1.;let D=a2/(PI*den*den+0.00001);
    let k=(rough+1.)*(rough+1.)/8.;let G=(nv/(nv*(1.-k)+k))*(nl/(nl*(1.-k)+k));
    let F0=mix(vec3(0.04),albedo.rgb,metal);let F=F0+(1.-F0)*pow(1.-vh,5.);
    let spec=D*G*F/max(4.*nv*nl,0.001);let diffuse=(1.-F)*(1.-metal)*albedo.rgb/PI;
    let visibility=shadow_visibility(i.world,i.current_clip.w,nl);let ray=reflect(-v,n);
    var environment=mix(f.ambient.rgb,f.sky.rgb,clamp(ray.y*0.5+0.5,0.,1.));
    let env_uv=vec2(atan2(ray.z,ray.x)/(2.*PI)+0.5+surface.sky.x,acos(clamp(ray.y,-1.,1.))/PI);
    if surface.flags.z>0u { environment=textureSampleLevel(sky_reflection,environment_sampler,env_uv,rough*f32(textureNumLevels(sky_reflection)-1u)).rgb*surface.sky.y; }
    let r=rough*vec4(-1.,-0.0275,-0.572,0.022)+vec4(1.,0.0425,1.04,-0.04);
    let a004=min(r.x*r.x,exp2(-9.28*nv))*r.x+r.y;let ab=vec2(-1.04,1.04)*a004+r.zw;
    let indirect=environment*(F0*ab.x+ab.y)*ao;
    var colour=(diffuse+spec)*f.sun_colour.rgb*f.sun.w*nl*visibility+albedo.rgb*(1.-metal)*f.ambient.rgb*ao+indirect+textureSample(emission_map,repeat_sampler,i.uv).rgb*d.emission.rgb;
    if d.flags.y==2u && surface.flags.y>0u {
        var reflection=f.sky.rgb;
        if surface.flags.z>0u { reflection=textureSample(sky_map,repeat_sampler,env_uv).rgb*surface.sky.y; }
        let f0=pow((surface.sky.z-1.)/(surface.sky.z+1.),2.);let fresnel=f0+(1.-f0)*pow(1.-nv,5.);
        colour=mix(colour,reflection,clamp(fresnel*surface.water_absorption.w,0.,1.));
    }
    colour=mix(colour,f.fog.rgb,1.-exp(-length(i.world-f.eye.xyz)*f.fog.w));
    let current=i.current_clip.xy/i.current_clip.w*vec2(0.5,-0.5)+0.5;
    let previous=i.previous_clip.xy/max(i.previous_clip.w,0.001)*vec2(0.5,-0.5)+0.5;
    return SceneOutput(vec4(colour,albedo.a),vec4(current-previous,max(i.previous_clip.w,0.001),max(i.current_clip.w,0.001)));
}
struct Full { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn fullscreen(@builtin(vertex_index) id: u32) -> Full {
    let uv=vec2(f32((id<<1u)&2u),f32(id&2u)); return Full(vec4(uv*vec2(2.,-2.)+vec2(-1.,1.),0.,1.),uv);
}
@fragment fn sky_fragment(i: Full) -> SceneOutput {
    let point=f.inverse_vp*vec4(i.uv*vec2(2.,-2.)+vec2(-1.,1.),1.,1.);
    let ray=normalize(point.xyz/point.w-f.eye.xyz);
    let uv=vec2(atan2(ray.z,ray.x)/(2.*PI)+0.5+surface.sky.x,acos(clamp(ray.y,-1.,1.))/PI);
    let previous=f.previous_vp*vec4(ray,0.);let old_uv=previous.xy/max(previous.w,0.001)*vec2(0.5,-0.5)+0.5;
    return SceneOutput(vec4(textureSample(sky_map,repeat_sampler,uv).rgb*surface.sky.y,1.),vec4(i.uv-f.jitter.zw-old_uv,800.,800.));
}
