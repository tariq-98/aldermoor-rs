// The effects pass (crate::fx): ground marks projected onto the scene inside the scene pass, and
// sprites, beams and effect meshes drawn over the tone-mapped output. Unlit, with premultiplied
// alpha throughout: rgb adds light, alpha covers what's behind.
#include <metal_stdlib>
using namespace metal;
struct Frame {
    float4x4 vp,previous_vp,inverse_vp;float4 jitter;float4x4 shadows[3];
    float4 eye,sun,sun_colour,ambient,sky,fog,grade[3],params,clock,viewport,splits;
};
struct Material {float4 colour,frame,next,params,extra;};
struct Quad {float4 position,a,b;Material m;};
struct Decal {float4x4 to_decal,to_world;float4 shape,size,fill;Material m;};
struct MeshInstance {float4x4 model;float4 shading,terms;Material m;};
struct Vertex {float4 position,normal,tangent,color,uv;uint4 joints;float4 weights;};
// Output size (w, h, 1/w, 1/h); the camera's right (xyz) and scene time (w); its up (xyz).
struct Globals {float4 output,right,up;};

constexpr sampler atlas_sampler(coord::normalized,address::clamp_to_edge,filter::linear,mip_filter::linear);
constexpr sampler tile_sampler(coord::normalized,address::repeat,filter::linear,mip_filter::linear);
constexpr sampler depth_sampler(coord::normalized,address::clamp_to_edge,filter::nearest);

// Camera::matrix's clip planes: what the depth buffer holds back to metres from the eye.
float linear_depth(float z){return (0.1*800.)/(800.-z*(800.-0.1));}

// A material's texel at uv (0..1 across its sprite or tile): heat, cover, erosion order.
float4 texel(device const Material& m,float2 uv,texture2d<float> atlas,texture2d_array<float> tiles){
    if(m.extra.x<0.5){
        float2 local=clamp(uv,0.,1.);
        float4 t=atlas.sample(atlas_sampler,m.frame.xy+local*m.frame.zw);
        if(m.next.z>0.001)t=mix(t,atlas.sample(atlas_sampler,m.next.xy+local*m.frame.zw),m.next.z);
        return t;
    }
    return tiles.sample(tile_sampler,uv*m.frame.zw+m.frame.xy,uint(m.extra.x-0.5));
}

// A material's colour at a heat: its ramp's, or the heat as a grey.
float3 ramp_colour(device const Material& m,float heat,texture2d<float> ramps){
    if(m.next.w<0.)return float3(heat);
    // Ramps are 32-bit float, read and blended by hand (not every GPU filters them).
    uint last=ramps.get_width()-1,row=uint(m.next.w+0.5);
    float x=saturate(heat)*float(last);
    uint i=min(uint(x),last);
    return mix(ramps.read(uint2(i,row)).rgb,ramps.read(uint2(min(i+1,last),row)).rgb,x-float(i));
}

// Colour (linear, unpremultiplied) and opacity: heat through the ramp, cover eroded.
float4 shade(device const Material& m,float4 t,texture2d<float> ramps){
    float cover=t.g;
    if(m.params.y>0.)cover*=smoothstep(m.params.x,m.params.x+m.params.y,t.b);
    float3 c=ramp_colour(m,saturate(t.r*m.extra.w),ramps);
    return float4(c*m.colour.rgb,cover*m.colour.a);
}

// The composite's exposure, grade and tone map, for effects drawn after it.
float3 tone_map(float3 c,constant Frame& f){
    c*=f.params.x;
    c=float3(dot(f.grade[0].xyz,c),dot(f.grade[1].xyz,c),dot(f.grade[2].xyz,c));
    if(f.params.y>0.5)c=saturate((c*(2.51*c+0.03))/(c*(2.43*c+0.59)+0.14));
    return max(c,0.);
}

// Over the tone-mapped output: light adds as tone-mapped light, paint covers.
float4 over_output(float4 c,float blend,constant Frame& f){
    float3 light=tone_map(c.rgb*c.a,f);
    float3 paint=tone_map(c.rgb,f)*c.a;
    return float4(mix(light,paint,blend),c.a*blend);
}

// How much of an effect at `depth` metres shows in front of the scene at this pixel.
float in_front(float depth,float soft,float2 position,constant Globals& g,depth2d<float> scene){
    float behind=linear_depth(scene.sample(depth_sampler,position*g.output.zw));
    return soft>0.?saturate((behind-depth)/soft):float(depth<behind+0.05);
}

struct QuadVary{float4 position [[position]];float2 uv;float depth;uint instance [[flat]];};

vertex QuadVary quad_vertex(uint vid [[vertex_id]],uint iid [[instance_id]],constant Frame& f [[buffer(1)]],device const Quad* quads [[buffer(2)]],constant Globals& g [[buffer(3)]]){
    float2 corners[6]={float2(-1,-1),float2(1,-1),float2(1,1),float2(-1,-1),float2(1,1),float2(-1,1)};
    float2 c=corners[vid];
    device const Quad& q=quads[iid];
    float3 world;float2 uv=float2(c.x*0.5+0.5,0.5-c.y*0.5);
    int shape=int(q.position.w+0.5);
    if(shape==0){
        float s=sin(q.a.z),k=cos(q.a.z);
        float2 p=float2(c.x*q.a.x,c.y*q.a.y);
        p=float2(p.x*k-p.y*s,p.x*s+p.y*k);
        float3 centre=q.position.xyz+normalize(f.eye.xyz-q.position.xyz)*q.b.w;
        world=centre+g.right.xyz*p.x+g.up.xyz*p.y;
    }else if(shape==1){
        float3 tail=q.position.xyz,head=q.a.xyz,mid=(tail+head)*0.5;
        float3 side=cross(head-tail,f.eye.xyz-mid);
        float l=length(side);
        side=l>1e-6?side/l:g.right.xyz;
        float along=c.y*0.5+0.5;
        world=mix(tail,head,along)+side*c.x*mix(q.b.x,q.a.w,along);
        // A sprite stands head up (its top at the head); a tile runs along its u.
        uv=q.m.extra.x<0.5?float2(c.x*0.5+0.5,1.-along):float2(along,c.x*0.5+0.5);
    }else{
        world=q.position.xyz+q.a.xyz*c.x+q.b.xyz*c.y;
    }
    float4 clip=f.vp*float4(world,1);
    QuadVary o;o.position=clip;o.uv=uv;o.depth=clip.w;o.instance=iid;
    return o;
}

fragment float4 quad_fragment(QuadVary i [[stage_in]],constant Frame& f [[buffer(1)]],device const Quad* quads [[buffer(2)]],constant Globals& g [[buffer(3)]],texture2d<float> atlas [[texture(0)]],texture2d_array<float> tiles [[texture(1)]],texture2d<float> ramps [[texture(2)]],depth2d<float> scene [[texture(3)]]){
    device const Quad& q=quads[i.instance];
    device const Material& m=q.m;
    float4 t=texel(m,i.uv,atlas,tiles);
    // A tile's beam is a ribbon off a missile: it cools and thins away towards its tail.
    if(int(q.position.w+0.5)==1&&m.extra.x>0.5){t.r*=0.35+0.65*i.uv.x;t.g*=smoothstep(0.,0.7,i.uv.x);}
    float4 c=shade(m,t,ramps);
    c.a*=in_front(i.depth,m.params.z,i.position.xy,g,scene);
    if(c.a<0.001)discard_fragment();
    return over_output(c,m.params.w,f);
}

struct MeshVary{float4 position [[position]];float3 world,normal,object,object_normal;float4 colour;float2 uv;float depth;uint instance [[flat]];};

vertex MeshVary mesh_vertex(uint vid [[vertex_id]],uint iid [[instance_id]],device const Vertex* vertices [[buffer(0)]],constant Frame& f [[buffer(1)]],device const MeshInstance* meshes [[buffer(2)]]){
    Vertex v=vertices[vid];
    device const MeshInstance& m=meshes[iid];
    float3 world=(m.model*float4(v.position.xyz,1)).xyz;
    float4 clip=f.vp*float4(world,1);
    MeshVary o;o.position=clip;o.world=world;o.normal=(m.model*float4(v.normal.xyz,0)).xyz;o.object=v.position.xyz;o.object_normal=v.normal.xyz;o.colour=v.color;o.uv=v.uv.xy;o.depth=clip.w;o.instance=iid;
    return o;
}

fragment float4 mesh_fragment(MeshVary i [[stage_in]],bool front [[front_facing]],constant Frame& f [[buffer(1)]],device const MeshInstance* meshes [[buffer(2)]],constant Globals& g [[buffer(3)]],texture2d<float> atlas [[texture(0)]],texture2d_array<float> tiles [[texture(1)]],texture2d<float> ramps [[texture(2)]],depth2d<float> scene [[texture(3)]]){
    device const MeshInstance& inst=meshes[i.instance];
    device const Material& m=inst.m;
    float3 n=normalize(i.normal)*(front?1.:-1.);
    float facing=abs(dot(n,normalize(f.eye.xyz-i.world)));
    float rim=pow(1.-facing,inst.shading.x)*inst.shading.y;
    float mask=smoothstep(inst.shading.z,1.,i.colour.r)*inst.shading.w;
    float time=g.right.w;
    float twinkle=pow(0.5+0.5*sin(time*inst.terms.y+i.colour.g*43.),8.)*inst.terms.x;
    float4 t;
    if(m.extra.x>0.5){
        // Tiles wrap a mesh by its own shape (three projections, blended by its normal), so
        // a shell needs no UVs and shows no seam.
        float3 w=abs(normalize(i.object_normal));
        w/=max(dot(w,float3(1)),0.001);
        float2 scroll=float2(0.,time*inst.terms.w),rep=m.frame.zw;
        uint layer=uint(m.extra.x-0.5);
        t=tiles.sample(tile_sampler,i.object.zy*rep+scroll,layer)*w.x
         +tiles.sample(tile_sampler,i.object.xz*rep+scroll,layer)*w.y
         +tiles.sample(tile_sampler,i.object.xy*rep+scroll,layer)*w.z;
    }else{
        t=texel(m,i.uv+float2(0.,time*inst.terms.w),atlas,tiles);
    }
    t*=inst.terms.z;
    float heat=t.r+mask+rim+twinkle;
    // Edge-on surfaces fade out rather than alias at the silhouette.
    float cover=(t.g+mask+rim+twinkle*0.5)*smoothstep(0.,0.15,facing);
    float4 c=shade(m,float4(heat,cover,i.colour.g,1.),ramps);
    c.a*=in_front(i.depth,m.params.z,i.position.xy,g,scene);
    if(c.a<0.001)discard_fragment();
    return over_output(c,m.params.w,f);
}

struct DecalVary{float4 position [[position]];uint instance [[flat]];};

vertex DecalVary decal_vertex(uint vid [[vertex_id]],uint iid [[instance_id]],constant Frame& f [[buffer(1)]],device const Decal* decals [[buffer(2)]]){
    // A cube, its triangles wound anticlockwise seen from outside. Back faces are drawn, so a
    // camera inside a box still sees its mark.
    ushort corners[36]={1,3,7,1,7,5, 0,4,6,0,6,2, 2,6,7,2,7,3, 0,1,5,0,5,4, 4,5,7,4,7,6, 0,2,3,0,3,1};
    ushort k=corners[vid];
    float3 p=float3(k&1?1.:-1.,k&2?1.:-1.,k&4?1.:-1.);
    float4 clip=f.vp*(decals[iid].to_world*float4(p,1));
    // Jittered like the scene it's drawn into, so temporal AA resolves both alike.
    clip.xy+=f.jitter.xy*clip.w;
    DecalVary o;o.position=clip;o.instance=iid;
    return o;
}

// Inside the scene pass: the scene's linear depth is in the motion target's w. No early outs:
// derivatives need every pixel of a quad, and a mark outside its box simply adds nothing.
fragment float4 decal_fragment(DecalVary i [[stage_in]],float4 motion [[color(1)]],constant Frame& f [[buffer(1)]],device const Decal* decals [[buffer(2)]],texture2d<float> atlas [[texture(0)]],texture2d_array<float> tiles [[texture(1)]],texture2d<float> ramps [[texture(2)]]){
    float depth=motion.w;
    float2 ndc=float2(i.position.x*f.viewport.z*2.-1.,1.-i.position.y*f.viewport.w*2.);
    float4 far=f.inverse_vp*float4(ndc,1,1);
    float3 world=f.eye.xyz+(far.xyz/far.w-f.eye.xyz)*(depth/800.);
    device const Decal& d=decals[i.instance];
    float3 local=(d.to_decal*float4(world,1)).xyz;
    // Only surfaces facing up the box take the mark: not the legs of whoever stands in it.
    // Across a silhouette the next pixel is another surface, so its slope can't be trusted.
    float3 wx=dfdx(world),wy=dfdy(world);
    float3 n=normalize(cross(wx,wy));
    float3 up=normalize(d.to_world[1].xyz);
    float even=1.-smoothstep(0.02,0.05,(length(wx)+length(wy))/max(depth,0.1));
    float lie=smoothstep(0.45,0.7,abs(dot(n,up)))*(1.-smoothstep(0.75,1.,abs(local.y)))*even;
    float sky=float(depth>0.01&&depth<799.);
    float boxed=float(all(abs(local)<=1.));
    // Metres from the shape's edge (positive inside), and from the fill's leading edge.
    int kind=int(d.shape.x+0.5);
    float2 p=local.xz*d.size.xz;
    float grown=d.shape.z;
    float inside,front;
    if(kind==2){
        // From the apex, which sits size.w behind the box's centre; the reach is twice size.z.
        p.y+=d.size.w;
        float reach=d.size.z*2.,r=length(p),angle=atan2(p.x,p.y);
        inside=min(reach-r,(d.shape.y-abs(angle))*max(r,0.001));
        front=r-reach*grown;
    }else if(kind==3){
        inside=min(d.size.x-abs(p.x),d.size.z-abs(p.y));
        front=p.y+d.size.z-2.*d.size.z*grown;
    }else{
        float r=length(p);
        inside=d.size.x-r;front=r-d.size.x*grown;
    }
    float aa=max(fwidth(inside),1e-4);
    device const Material& m=d.m;
    float4 out=0.;
    if(kind==0){
        float4 c=shade(m,texel(m,float2(local.x*0.5+0.5,0.5-local.z*0.5),atlas,tiles),ramps);
        // A pool of light under the mark in its ramp's colour, tinted and weighted by the fill.
        float r2=dot(local.xz,local.xz);
        float pool=r2<1.?(1.-r2)*(1.-r2):0.;
        float3 under=ramp_colour(m,0.5*pool,ramps)*d.fill.rgb*d.fill.a*pool;
        out=float4(c.rgb*c.a+under,c.a*m.params.w);
    }else{
        float shape=smoothstep(-aa,aa,inside);
        float edge=max(d.shape.w,0.01);
        // A crisp ring on the boundary with a soft glow either side, as fight indicators are
        // drawn: the area itself stays nearly clear, so what lands in it is what's watched.
        float line=exp(-abs(inside)/(edge*0.3+aa));
        float glow=shape*exp(-max(inside,0.)/(edge*1.5))*0.2+(1.-shape)*exp(min(inside,0.)/(edge*0.8))*0.15;
        // As a warning ripens a second ring sweeps out to its edge, a faint tint behind it.
        float filled=grown<0.?1.:smoothstep(aa,-aa,front);
        float lead=grown<0.||grown>=1.?0.:exp(-abs(front)/(edge*0.35+aa))*shape*0.8;
        float breakup=1.;
        if(m.extra.x>0.5)breakup=0.3+1.4*tiles.sample(tile_sampler,world.xz*m.frame.zw+m.frame.xy,uint(m.extra.x-0.5)).r;
        float body=shape*filled*d.fill.a*breakup;
        float3 light=m.colour.rgb*m.colour.a*(line+glow+lead)+d.fill.rgb*body;
        // A slight darkening over the whole area lifts the ring off bright ground.
        out=float4(light,shape*m.params.w);
    }
    return out*(lie*sky*boxed);
}
