#include <metal_stdlib>
using namespace metal;
struct Vertex {float4 position,normal,tangent,color,uv;uint4 joints;float4 weights;};
struct Frame {
    float4x4 vp,previous_vp,inverse_vp;float4 jitter;float4x4 shadows[3];
    float4 eye,sun,sun_colour,ambient,sky,fog,grade[3],params,clock,viewport,splits;
};
struct Surfaces {float4 bounds,scales,roughness,water_deep,water_shallow,water_absorption,water_flow,sky;uint4 flags;};
struct Draw {
    float4x4 model,previous_model;
    float4 base,emission,params;uint4 flags;float4 morph,previous_morph,dyes[2],references[2];
};
struct Vary {float4 position [[position]];float3 world,normal;float4 tangent,color;float2 uv;float4 current_clip,previous_clip;float region [[flat]];float2 surface;uint instance [[flat]];};
float3 wind(float3 p,float weight,float time,float strength){float phase=dot(p.xz,float2(0.35,0.27));float gust=sin(time*0.9+phase)*0.6+sin(time*2.3+phase*1.7)*0.25;float flutter=sin(time*7.+phase*5.)*0.08;return p+normalize(float3(1,0,0.35))*weight*strength*(gust+flutter)*0.12;}
float4x4 skin_matrix(Vertex a,device const float4x4* bones,bool on){return on?bones[a.joints.x]*a.weights.x+bones[a.joints.y]*a.weights.y+bones[a.joints.z]*a.weights.z+bones[a.joints.w]*a.weights.w:float4x4(1.);}
Vary deform(uint id,device const Vertex* vertices,constant Frame& f,constant Draw& d,device const float4x4* bones,device const float4* morphs,device const float4x4* previous_bones){
    Vertex a=vertices[id];float3 p=a.position.xyz,old_p=p;
    if(d.morph.z>0.){p+=morphs[id*2].xyz*d.morph.x+morphs[id*2+1].xyz*d.morph.y;old_p+=morphs[id*2].xyz*d.previous_morph.x+morphs[id*2+1].xyz*d.previous_morph.y;}
    float4x4 skin=skin_matrix(a,bones,d.flags.y!=0),old_skin=skin_matrix(a,previous_bones,d.flags.y!=0);
    float3 world=wind((d.model*skin*float4(p,1)).xyz,a.normal.w,f.clock.x,f.clock.z);
    float3 old_world=wind((d.previous_model*old_skin*float4(old_p,1)).xyz,a.normal.w,f.clock.y,f.clock.z);
    Vary o;o.world=world;o.current_clip=f.vp*float4(world,1);o.previous_clip=f.previous_vp*float4(old_world,1);o.position=o.current_clip;o.position.xy+=f.jitter.xy*o.position.w;
    int region=int(round(a.position.w));o.region=a.position.w;
    if(region>=0&&region<31&&(d.flags.x&(1u<<region)))o.position=float4(2,2,2,1);
    o.normal=normalize((d.model*skin*float4(a.normal.xyz,0)).xyz);o.tangent=float4((d.model*skin*float4(a.tangent.xyz,0)).xyz,a.tangent.w);o.color=a.color;o.uv=a.uv.xy;o.surface=a.uv.zw;return o;
}
vertex Vary scene_vertex(uint id [[vertex_id]],uint instance [[instance_id]],device const Vertex* v [[buffer(0)]],constant Frame& f [[buffer(1)]],constant Draw* instances [[buffer(2)]],device const float4x4* bones [[buffer(3)]],device const float4* morphs [[buffer(4)]],device const float4x4* old_bones [[buffer(6)]]){Vary o=deform(id,v,f,instances[instance],bones,morphs,old_bones);o.instance=instance;return o;}
vertex Vary shadow_vertex(uint id [[vertex_id]],uint instance [[instance_id]],device const Vertex* v [[buffer(0)]],constant Frame& f [[buffer(1)]],constant Draw* instances [[buffer(2)]],device const float4x4* bones [[buffer(3)]],device const float4* morphs [[buffer(4)]],constant uint& cascade [[buffer(5)]],device const float4x4* old_bones [[buffer(6)]]){
    constant Draw& d=instances[instance];Vary o=deform(id,v,f,d,bones,morphs,old_bones);o.instance=instance;o.position=f.shadows[cascade]*float4(o.world,1);int region=int(round(o.region));if(region>=0&&region<31&&(d.flags.x&(1u<<region)))o.position=float4(2,2,2,1);return o;
}
fragment void shadow_fragment(Vary i [[stage_in]],constant Draw* instances [[buffer(2)]],texture2d<float> base [[texture(0)]]){constant Draw& d=instances[i.instance];if(d.flags.z==1){constexpr sampler s(coord::normalized,address::repeat,filter::linear,mip_filter::linear);if(base.sample(s,i.uv).a*d.base.a*i.color.a<d.params.w)discard_fragment();}}
float sample_cascade(float3 world,uint layer,float bias,constant Frame& f,depth2d_array<float> shadow){float4 clip=f.shadows[layer]*float4(world,1);float3 p=clip.xyz/clip.w;float2 uv=p.xy*float2(0.5,-0.5)+0.5;if(any(uv<0.)||any(uv>1.)||p.z<0.||p.z>1.)return 1.;constexpr sampler cmp(coord::normalized,address::clamp_to_edge,filter::linear,compare_func::less_equal);float v=0.;for(int y=-1;y<=1;y+=2)for(int x=-1;x<=1;x+=2)v+=shadow.sample_compare(cmp,uv+float2(x,y)*0.5*f.splits.w,layer,p.z-bias);return v*0.25;}
float shadow_visibility(float3 p,float depth,float nl,constant Frame& f,depth2d_array<float> shadow){uint layer=depth<f.splits.x?0:(depth<f.splits.y?1:2);if(depth>f.splits.z)return 1.;float bias=max(0.0001,0.0006*(1.-nl));float v=sample_cascade(p,layer,bias,f,shadow);float end=f.splits[layer];if(layer<2&&depth>end*0.88){float next=sample_cascade(p,layer+1,bias,f,shadow);v=mix(v,next,smoothstep(end*0.88,end,depth));}return v;}
// World-space triplanar layers retain the supplied horizontal control-map placement.
// Explicit gradients keep mip selection valid when a negligible projection is skipped.
struct TerrainSample {float3 colour;float3 perturbation;};
TerrainSample terrain_layer(texture2d_array<float> colour,texture2d_array<float> normals,
    float3 position,float3 dx,float3 dy,float3 weights,float2 side_signs,uint layer,float mip_bias) {
    constexpr sampler s(coord::normalized,address::repeat,filter::linear,mip_filter::linear,max_anisotropy(4));
    // Side projections stand upright: u runs along the face and v down it, so a layer's rows
    // lie level on a cliff (the limestone's bedding) and its columns hang plumb.
    // Keep UVs, explicit mip gradients and tangent slopes in the same signed basis.
    float2 uv[3]={float2(position.z*side_signs.x,-position.y),position.xz,float2(position.x*side_signs.y,-position.y)};
    float2 gx[3]={float2(dx.z*side_signs.x,-dx.y),dx.xz,float2(dx.x*side_signs.y,-dx.y)};
    float2 gy[3]={float2(dy.z*side_signs.x,-dy.y),dy.xz,float2(dy.x*side_signs.y,-dy.y)};
    TerrainSample result={float3(0),float3(0)};
    for(uint axis=0;axis<3;axis++) {
        if(weights[axis]<0.001)continue;
        gradient2d grad(gx[axis]*exp2(mip_bias),gy[axis]*exp2(mip_bias));
        result.colour+=colour.sample(s,uv[axis],layer,grad).rgb*weights[axis];
        float3 nm=normals.sample(s,uv[axis],layer,grad).rgb*2.-1.;
        float2 slope=nm.xy/max(nm.z,0.1);
        // Green points towards the image top (-v), matching the supplied normal maps.
        float3 perturb=axis==0?float3(0,slope.y,slope.x*side_signs.x):
            (axis==1?float3(slope.x,0,-slope.y):float3(slope.x*side_signs.y,slope.y,0));
        result.perturbation+=perturb*weights[axis];
    }
    return result;
}
// Integrate density * exp(-falloff * max(y-base, 0)) along a finite ray.
// Splitting at the base caps density underground and avoids overflowing exp on downward rays.
float height_fog_amount(float3 eye,float3 point,float density,float base,float falloff) {
    if(density<=0.)return 0.;
    float distance=length(point-eye);
    if(distance==0.)return 0.;
    float low=min(eye.y,point.y)-base,high=max(eye.y,point.y)-base;
    float below=high<=0.?1.:(low>=0.?0.:-low/(high-low));
    float span=max(high,0.)-max(low,0.);
    float x=falloff*span;
    // The series removes cancellation at level/near-level rays; it includes x == 0.
    float average=x<0.01?1.-x*0.5+x*x/6.-x*x*x/24.:(1.-exp(-x))/x;
    float mean=below+(1.-below)*exp(-falloff*max(low,0.))*average;
    return 1.-exp(-(mean*density)*distance);
}
struct SceneOutput{float4 colour [[color(0)]];float4 motion [[color(1)]];};
fragment SceneOutput scene_fragment(Vary i [[stage_in]],bool front [[front_facing]],constant Frame& f [[buffer(1)]],constant Draw* instances [[buffer(2)]],texture2d<float> base [[texture(0)]],texture2d<float> normal [[texture(1)]],texture2d<float> orm [[texture(2)]],texture2d<float> emission [[texture(3)]],texture2d<float> occlusion [[texture(4)]],depth2d_array<float> shadow [[texture(5)]],texture2d<float> dye_mask [[texture(6)]],constant Surfaces& surfaces [[buffer(3)]],texture2d_array<float> terrain_colour [[texture(7)]],texture2d_array<float> terrain_normal [[texture(8)]],texture2d<float> terrain_weights [[texture(9)]],texture2d<float> water_normal [[texture(10)]],texture2d<float> sky_map [[texture(11)]],texture2d<float> sky_reflection [[texture(12)]]){
    constant Draw& d=instances[i.instance];constexpr sampler s(coord::normalized,address::repeat,filter::linear,mip_filter::linear,max_anisotropy(4));
    float4 texel=base.sample(s,i.uv,bias(f.params.z));float4 albedo=texel*d.base;
    float2 mask=dye_mask.sample(s,i.uv,bias(f.params.z)).rg;float3 paint=albedo.rgb;
    albedo.rgb=mix(albedo.rgb,paint*d.dyes[0].rgb/max(d.references[0].rgb,float3(0.01)),mask.r*d.dyes[0].w);
    albedo.rgb=mix(albedo.rgb,paint*d.dyes[1].rgb/max(d.references[1].rgb,float3(0.01)),mask.g*d.dyes[1].w);
    float3 surface_normal=float3(0,0,1);float surface_roughness=d.emission.w;
    bool terrain=d.flags.w==1&&surfaces.flags.x>0;
    float3 terrain_perturbation=0.;
    float3 world_dx=dfdx(i.world),world_dy=dfdy(i.world);
    if(terrain){
        constexpr sampler control(coord::normalized,address::clamp_to_edge,filter::linear);
        float2 uv=clamp((i.world.xz-surfaces.bounds.xy)/(surfaces.bounds.zw-surfaces.bounds.xy),0.,1.);
        float4 weights=max(terrain_weights.sample(control,uv),float4(0));
        for(uint layer=surfaces.flags.x;layer<4;layer++)weights[layer]=0.;
        float sum=dot(weights,float4(1));weights=sum>0.0001?weights/sum:float4(1,0,0,0);
        float3 projections=pow(abs(normalize(i.normal)),float3(4));
        projections/=max(dot(projections,float3(1)),0.0001);
        albedo.rgb=0.;surface_roughness=0.;
        for(uint layer=0;layer<surfaces.flags.x;layer++){
            if(weights[layer]==0.)continue;
            TerrainSample sample=terrain_layer(terrain_colour,terrain_normal,
                i.world*surfaces.scales[layer],world_dx*surfaces.scales[layer],world_dy*surfaces.scales[layer],
                // With v down, +u points right when cross(T_u,T_v) == -normal.
                projections,float2(-sign(i.normal.x),sign(i.normal.z)),layer,f.params.z);
            albedo.rgb+=sample.colour*weights[layer];
            terrain_perturbation+=sample.perturbation*weights[layer];
            surface_roughness+=surfaces.roughness[layer]*weights[layer];
        }
    }
    if(d.flags.w==2&&surfaces.flags.y>0){float2 uv=(i.world.xz+f.clock.x*surfaces.water_flow.yz)*surfaces.water_flow.x;surface_normal=water_normal.sample(s,uv,bias(f.params.z)).rgb*2.-1.;surface_normal.xy*=surfaces.water_shallow.w;float3 transmission=exp(-surfaces.water_absorption.rgb*max(i.surface.x,0.));albedo.rgb=mix(surfaces.water_deep.rgb,surfaces.water_shallow.rgb,transmission);albedo.a=surfaces.water_deep.w;surface_roughness=surfaces.water_flow.w;}
    albedo*=i.color;if(d.flags.z==1&&albedo.a<d.params.w)discard_fragment();
    float3 n=normalize(i.normal)*(d.morph.w>0.5&&!front?-1.:1.),t=i.tangent.xyz,b;
    if(dot(t,t)>0.1){t=normalize(t-n*dot(n,t));b=cross(n,t)*i.tangent.w;}else{float3 dx=dfdx(i.world),dy=dfdy(i.world);float2 du=dfdx(i.uv),dv=dfdy(i.uv);float det=du.x*dv.y-du.y*dv.x;if(abs(det)>1e-6){t=normalize((dx*dv.y-dy*du.y)/det);t=normalize(t-n*dot(t,n));b=normalize((dx*dv.x-dy*du.x)/det);}else{t=normalize(cross(abs(n.y)<0.99?float3(0,1,0):float3(1,0,0),n));b=cross(n,t);}}
    if(!isfinite(t.x)||!isfinite(b.x)){t=normalize(cross(abs(n.y)<0.99?float3(0,1,0):float3(1,0,0),n));b=cross(n,t);}
    if(terrain){
        // Project blended slopes onto the geometric tangent plane. A flat normal map
        // leaves the mesh normal unchanged, even between projection axes.
        terrain_perturbation*=d.params.y;
        n=normalize(n+terrain_perturbation-n*dot(n,terrain_perturbation));
    }else{
        float3 nm=d.flags.w==2&&surfaces.flags.y>0?surface_normal:normal.sample(s,i.uv,bias(f.params.z)).xyz*2.-1.;
        nm.xy*=d.params.y;n=normalize(t*nm.x+b*nm.y+n*nm.z);
    }
    float3 mr=orm.sample(s,i.uv,bias(f.params.z)).rgb;float rough=max(0.08,surface_roughness*mr.g),metal=d.params.x*mr.b;
    float ao=mix(1.,occlusion.sample(s,i.uv,bias(f.params.z)).r,d.params.z);float3 l=normalize(f.sun.xyz),v=normalize(f.eye.xyz-i.world),h=normalize(l+v);
    float nl=max(dot(n,l),0.),nv=max(dot(n,v),0.001),nh=max(dot(n,h),0.),vh=max(dot(v,h),0.);
    float a2=pow(rough,4.),den=nh*nh*(a2-1.)+1.;float D=a2/(M_PI_F*den*den+1e-5);float k=(rough+1.)*(rough+1.)/8.;float G=(nv/(nv*(1.-k)+k))*(nl/(nl*(1.-k)+k));float3 F0=mix(float3(0.04),albedo.rgb,metal),F=F0+(1.-F0)*pow(1.-vh,5.);
    float visibility=shadow_visibility(i.world,i.current_clip.w,nl,f,shadow);float3 spec=D*G*F/max(4.*nv*nl,0.001);float3 diffuse=(1.-F)*(1.-metal)*albedo.rgb/M_PI_F;
    float3 ray=reflect(-v,n);
    float3 environment=mix(f.ambient.rgb,f.sky.rgb,saturate(ray.y*0.5+0.5));
    if(surfaces.flags.z>0){constexpr sampler env(coord::normalized,s_address::repeat,t_address::clamp_to_edge,filter::linear,mip_filter::linear);float2 uv=float2(atan2(ray.z,ray.x)/(2.*M_PI_F)+0.5+surfaces.sky.x,acos(clamp(ray.y,-1.,1.))/M_PI_F);environment=sky_reflection.sample(env,uv,level(rough*float(sky_reflection.get_num_mip_levels()-1))).rgb*f.sky.w;}
    // Analytic split-sum BRDF fit (Lazarov), paired with GGX-prefiltered radiance.
    float4 r=rough*float4(-1.,-0.0275,-0.572,0.022)+float4(1.,0.0425,1.04,-0.04);
    float a004=min(r.x*r.x,exp2(-9.28*nv))*r.x+r.y;float2 ab=float2(-1.04,1.04)*a004+r.zw;
    float3 indirect=environment*(F0*ab.x+ab.y)*ao;
    float3 colour=(diffuse+spec)*f.sun_colour.rgb*f.sun.w*nl*visibility+albedo.rgb*(1.-metal)*f.ambient.rgb*ao+indirect+emission.sample(s,i.uv,bias(f.params.z)).rgb*d.emission.rgb;
    if(d.flags.w==2&&surfaces.flags.y>0){float3 reflection=f.sky.rgb;if(surfaces.flags.z>0){float3 ray=reflect(-v,n);float2 uv=float2(atan2(ray.z,ray.x)/(2.*M_PI_F)+0.5+surfaces.sky.x,acos(clamp(ray.y,-1.,1.))/M_PI_F);reflection=sky_map.sample(s,uv).rgb*f.sky.w;}float f0=pow((surfaces.sky.z-1.)/(surfaces.sky.z+1.),2.);float fresnel=f0+(1.-f0)*pow(1.-nv,5.);colour=mix(colour,reflection,saturate(fresnel*surfaces.water_absorption.w));}
    float fog=f.params.w>0.5
        ?height_fog_amount(f.eye.xyz,i.world,f.fog.w,f.grade[0].w,f.grade[1].w)
        :1.-exp(-length(i.world-f.eye.xyz)*f.fog.w);
    colour=mix(colour,f.fog.rgb,fog);
    float2 current=i.current_clip.xy/i.current_clip.w*float2(0.5,-0.5)+0.5;float2 previous=i.previous_clip.xy/max(i.previous_clip.w,0.001)*float2(0.5,-0.5)+0.5;
    return {float4(colour,albedo.a),float4(current-previous,max(i.previous_clip.w,0.001),max(i.current_clip.w,0.001))};
}
struct Full{float4 position [[position]];float2 uv;};
vertex Full fullscreen(uint id [[vertex_id]]){float2 uv=float2((id<<1)&2,id&2);return {float4(uv*float2(2,-2)+float2(-1,1),0,1),uv};}
float3 to_ycocg(float3 c){return float3(dot(c,float3(0.25,0.5,0.25)),dot(c,float3(0.5,0.,-0.5)),dot(c,float3(-0.25,0.5,-0.25)));}
float3 from_ycocg(float3 c){return float3(c.x+c.y-c.z,c.x+c.z,c.x-c.y-c.z);}
// Five-tap Catmull-Rom reconstruction. Depth is deliberately sampled separately.
float4 catmull_weights(float t){return float4(-0.5*t+t*t-0.5*t*t*t,1.-2.5*t*t+1.5*t*t*t,0.5*t+2.*t*t-1.5*t*t*t,-0.5*t*t+0.5*t*t*t);}
float3 history_catmull(texture2d<float> tex,float2 uv){
    constexpr sampler linear(coord::normalized,address::clamp_to_edge,filter::linear);
    float2 size=float2(tex.get_width(),tex.get_height()),p=uv*size-0.5,base=floor(p);
    float4 wx=catmull_weights(p.x-base.x),wy=catmull_weights(p.y-base.y);
    float3 x=float3(base.x-1.,base.x+wx.z/(wx.y+wx.z),base.x+2.);
    float3 y=float3(base.y-1.,base.y+wy.z/(wy.y+wy.z),base.y+2.);
    float3 weights_x=float3(wx.x,wx.y+wx.z,wx.w),weights_y=float3(wy.x,wy.y+wy.z,wy.w),c=0.;
    // Combine the positive centre lobes with bilinear filtering. Omit the four small
    // negative-lobe corner products, then renormalise so a constant colour stays constant.
    c =tex.sample(linear,(float2(x.y,y.x)+0.5)/size).rgb*weights_x.y*weights_y.x;
    c+=tex.sample(linear,(float2(x.x,y.y)+0.5)/size).rgb*weights_x.x*weights_y.y;
    c+=tex.sample(linear,(float2(x.y,y.y)+0.5)/size).rgb*weights_x.y*weights_y.y;
    c+=tex.sample(linear,(float2(x.z,y.y)+0.5)/size).rgb*weights_x.z*weights_y.y;
    c+=tex.sample(linear,(float2(x.y,y.z)+0.5)/size).rgb*weights_x.y*weights_y.z;
    float total=weights_x.y*(weights_y.x+weights_y.y+weights_y.z)
               +weights_y.y*(weights_x.x+weights_x.z);
    c/=total;
    return max(c,0.);
}
fragment float4 temporal_resolve(Full i [[stage_in]],constant Frame& f [[buffer(1)]],texture2d<float> colour [[texture(0)]],texture2d<float> motion [[texture(1)]],depth2d<float> depth [[texture(2)]],texture2d<float> history [[texture(3)]]){
    constexpr sampler linear(coord::normalized,address::clamp_to_edge,filter::linear);
    uint2 size=uint2(colour.get_width(),colour.get_height());
    int2 p=clamp(int2((i.uv+f.jitter.zw)*float2(size)),int2(0),int2(size)-1);
    float3 now=colour.sample(linear,i.uv+f.jitter.zw).rgb;
    float z=depth.read(uint2(p));
    float4 centre_motion=motion.read(uint2(p));
    float linear_z=centre_motion.w>0.?centre_motion.w:(0.1*800.)/(800.-z*(800.-0.1));
    // These pixels already returned the current sample. Reject before the neighbourhood work.
    if(f.clock.w<0.5||z>=1.)return float4(now,linear_z);

    float nearest=1.;int2 closest=p;
    // Preserve the original scan order and strict comparison, including depth ties at edges.
    #pragma unroll
    for(int y=-1;y<=1;y++){
        #pragma unroll
        for(int x=-1;x<=1;x++){
            int2 q=clamp(p+int2(x,y),int2(0),int2(size)-1);
            float d=(x==0&&y==0)?z:depth.read(uint2(q));
            if(d<nearest){nearest=d;closest=q;}
        }
    }
    float4 mv=all(closest==p)?centre_motion:motion.read(uint2(closest));
    float2 uv=i.uv-mv.xy;
    if(any(uv<=0.)||any(uv>=1.))return float4(now,linear_z);
    // Depth rejection needs only bilinear alpha, not the expensive bicubic colour reconstruction.
    float old_depth=history.sample(linear,uv).a;
    if(abs(old_depth-mv.z)>max(0.03,mv.z*0.02))return float4(now,linear_z);

    float3 lo=float3(INFINITY),hi=-lo,mean=0.,square=0.;
    #pragma unroll
    for(int y=-1;y<=1;y++){
        #pragma unroll
        for(int x=-1;x<=1;x++){
            int2 q=clamp(p+int2(x,y),int2(0),int2(size)-1);
            float3 c=to_ycocg(colour.read(uint2(q)).rgb);
            lo=min(lo,c);hi=max(hi,c);mean+=c;square+=c*c;
        }
    }
    mean/=9.;float3 sigma=sqrt(max(square/9.-mean*mean,0.));
    float3 old_y=to_ycocg(history_catmull(history,uv));
    old_y=clamp(old_y,max(lo,mean-1.25*sigma),min(hi,mean+1.25*sigma));
    float weight=mix(0.08,0.25,saturate(length(mv.xy*float2(size))/20.));
    return float4(mix(from_ycocg(old_y),now,weight),linear_z);
}
float3 aces(float3 x){return saturate((x*(2.51*x+0.03))/(x*(2.43*x+0.59)+0.14));}
fragment float4 composite(Full i [[stage_in]],constant Frame& f [[buffer(1)]],texture2d<float> source [[texture(0)]]){constexpr sampler s(filter::linear,coord::normalized,address::clamp_to_edge);float3 c=source.sample(s,i.uv).rgb*f.params.x;c=float3(dot(f.grade[0].xyz,c),dot(f.grade[1].xyz,c),dot(f.grade[2].xyz,c));if(f.params.y>0.5)c=aces(c);return float4(c,1);}
struct GuiVertex{float2 position,uv;float4 colour;};
struct GuiVary{float4 position [[position]];float2 uv;float4 colour;};
vertex GuiVary gui_vertex(uint id [[vertex_id]],device const GuiVertex* vertices [[buffer(0)]],constant float4& size [[buffer(1)]]){GuiVertex v=vertices[id];return {float4(v.position/size.xy*float2(2,-2)+float2(-1,1),0,1),v.uv,v.colour};}
fragment float4 gui_fragment(GuiVary i [[stage_in]],texture2d<float> image [[texture(0)]]){constexpr sampler s(coord::normalized,address::clamp_to_edge,filter::linear);float4 c=i.colour*image.sample(s,i.uv);float3 rgb=c.a>0.?c.rgb/c.a:float3(0);rgb=select(rgb/12.92,pow((rgb+0.055)/1.055,float3(2.4)),rgb>0.04045);return float4(rgb*c.a,c.a);}

fragment SceneOutput sky_fragment(Full i [[stage_in]],constant Frame& f [[buffer(1)]],constant Surfaces& surfaces [[buffer(3)]],texture2d<float> sky_map [[texture(11)]]){
    constexpr sampler s(coord::normalized,s_address::repeat,t_address::clamp_to_edge,filter::linear,mip_filter::linear);
    float4 point=f.inverse_vp*float4(i.uv*float2(2,-2)+float2(-1,1),1,1);
    float3 ray=normalize(point.xyz/point.w-f.eye.xyz);
    float2 uv=float2(atan2(ray.z,ray.x)/(2.*M_PI_F)+0.5+surfaces.sky.x,acos(clamp(ray.y,-1.,1.))/M_PI_F);
    // Longitude is periodic: a quad crossing atan2's wrap spans a small arc, not a full map.
    // Keep latitude's derivative unchanged and clamp its pole samples instead of wrapping.
    float2 gx=dfdx(uv),gy=dfdy(uv);
    gx.x-=round(gx.x);
    gy.x-=round(gy.x);
    float4 previous=f.previous_vp*float4(ray,0);
    float2 old_uv=previous.xy/max(previous.w,0.001)*float2(0.5,-0.5)+0.5;
    float3 colour=sky_map.sample(s,uv,gradient2d(gx,gy)).rgb*f.sky.w;
    if(f.params.w>0.5&&f.grade[2].w>0.){
        float fog=height_fog_amount(f.eye.xyz,f.eye.xyz+ray*f.grade[2].w,f.fog.w,f.grade[0].w,f.grade[1].w);
        colour=mix(colour,f.fog.rgb,fog);
    }
    return {float4(colour,1),float4(i.uv-f.jitter.zw-old_uv,800,800)};
}

// Renderer-derived lighting data from the supplied sky; no authored texture or look values.
fragment float4 prefilter_sky(Full i [[stage_in]],constant float& rough [[buffer(0)]],texture2d<float> sky [[texture(0)]]) {
    constexpr sampler s(coord::normalized,s_address::repeat,t_address::clamp_to_edge,filter::linear,mip_filter::linear);
    float phi=(i.uv.x-0.5)*2.*M_PI_F,theta=i.uv.y*M_PI_F;
    float3 n=float3(cos(phi)*sin(theta),cos(theta),sin(phi)*sin(theta));
    float3 tangent=normalize(cross(abs(n.y)<0.999?float3(0,1,0):float3(1,0,0),n)),bitangent=cross(n,tangent);
    float3 sum=0.;float weight=0.;float a=max(rough*rough,0.001);
    for(uint k=0;k<128;k++){
        float2 xi=float2(float(k)/128.,float(reverse_bits(k))*2.3283064365386963e-10);
        float phi_h=2.*M_PI_F*xi.x,cos_h=sqrt((1.-xi.y)/(1.+(a*a-1.)*xi.y)),sin_h=sqrt(max(0.,1.-cos_h*cos_h));
        float3 h=tangent*cos(phi_h)*sin_h+bitangent*sin(phi_h)*sin_h+n*cos_h,l=reflect(-n,h);
        float nl=max(dot(n,l),0.);float2 uv=float2(atan2(l.z,l.x)/(2.*M_PI_F)+0.5,acos(clamp(l.y,-1.,1.))/M_PI_F);
        sum+=sky.sample(s,uv,level(0)).rgb*nl;weight+=nl;
    }
    return float4(sum/max(weight,0.001),1);
}

// AO and bloom operate on half-internal-resolution targets, before temporal reconstruction.
// All effect magnitudes come from LookSettings; these constants define the sampling kernels.
struct PostUniform { float4 ao, bloom, projection, blur; };
float post_depth(depth2d<float> depth, uint2 p) { return depth.read(p); }
float post_depth(depth2d_ms<float> depth, uint2 p) {
    float z = 1.;
    for (uint k = 0; k < depth.get_num_samples(); ++k) z = min(z, depth.read(p, k));
    return z;
}
template<typename Depth>
float4 post_position(Depth depth, int2 pixel, constant Frame& f) {
    uint2 size = uint2(depth.get_width(), depth.get_height());
    uint2 p = uint2(clamp(pixel, int2(0), int2(size) - 1));
    float z = post_depth(depth, p);
    float2 uv = (float2(p) + 0.5) / float2(size);
    float4 world = f.inverse_vp * float4(uv * float2(2, -2) + float2(-1, 1), z, 1);
    // The inverse perspective transform's w is reciprocal view depth.
    return float4(world.xyz / world.w, z < 1. ? 1. / world.w : 0.);
}
template<typename Depth>
float2 post_ao_value(Full i, constant Frame& f, constant PostUniform& post, Depth depth) {
    int2 size = int2(depth.get_width(), depth.get_height());
    int2 pixel = min(int2(i.uv * float2(size)), size - 1);
    float4 center = post_position(depth, pixel, f);
    if (center.w <= 0.) return float2(1, 0);
    float3 left = center.xyz - post_position(depth, pixel - int2(1, 0), f).xyz;
    float3 right = post_position(depth, pixel + int2(1, 0), f).xyz - center.xyz;
    float3 up = center.xyz - post_position(depth, pixel - int2(0, 1), f).xyz;
    float3 down = post_position(depth, pixel + int2(0, 1), f).xyz - center.xyz;
    float3 dx = dot(left, left) < dot(right, right) ? left : right;
    float3 dy = dot(up, up) < dot(down, down) ? up : down;
    float3 normal = cross(dx, dy);
    float norm2 = dot(normal, normal);
    if (norm2 < 1e-12) return float2(1, center.w);
    normal *= rsqrt(norm2);
    if (dot(normal, f.eye.xyz - center.xyz) < 0.) normal = -normal;
    float2 radius = post.ao.x * post.projection.xy * 0.5 * float2(size) / center.w;
    // Interleaved gradient noise breaks up the fixed ring. The post pass supplies
    // a frame phase only when temporal reconstruction can integrate the samples.
    float2 noise_pixel = floor(i.position.xy) + post.projection.z * 5.588238;
    float noise = fract(52.9829189 * fract(dot(noise_pixel, float2(0.06711056, 0.00583715))));
    float obscurance = 0.;
    for (uint k = 0; k < 8; ++k) {
        float angle = (float(k) / 8. + noise) * (2. * M_PI_F);
        float radial = sqrt((float(k) + noise) / 8.);
        float2 offset = float2(cos(angle), sin(angle)) * radius * radial;
        int2 q = pixel + int2(round(offset));
        // No repeated clamped border samples: off-screen occluders are unknown.
        if (any(q < 0) || any(q >= size)) continue;
        float4 sample = post_position(depth, q, f);
        if (sample.w <= 0.) continue;
        float3 delta = sample.xyz - center.xyz;
        float distance2 = dot(delta, delta);
        if (distance2 <= 1e-12) continue;
        float distance = sqrt(distance2);
        float horizon = max(dot(normal, delta) - post.ao.y, 0.) / distance;
        obscurance += horizon * saturate(1. - distance2 / (post.ao.x * post.ao.x));
    }
    return float2(saturate(1. - post.ao.z * obscurance / 8.), center.w);
}
fragment float2 post_ao(Full i [[stage_in]], constant PostUniform& post [[buffer(0)]], constant Frame& f [[buffer(1)]], depth2d<float> depth [[texture(1)]]) {
    return post_ao_value(i, f, post, depth);
}
fragment float2 post_ao_msaa(Full i [[stage_in]], constant PostUniform& post [[buffer(0)]], constant Frame& f [[buffer(1)]], depth2d_ms<float> depth [[texture(1)]]) {
    return post_ao_value(i, f, post, depth);
}
fragment float4 post_bloom_extract(Full i [[stage_in]], constant PostUniform& post [[buffer(0)]], texture2d<float> source [[texture(0)]]) {
    int2 size = int2(source.get_width(), source.get_height());
    int2 base = int2(i.position.xy) * 2;
    float3 sum = 0.;
    float weights = 0.;
    // projection.w is the authored knee/threshold ratio; zero retains a hard threshold.
    float knee = post.bloom.x * post.projection.w;
    for (int y = 0; y < 2; ++y) for (int x = 0; x < 2; ++x) {
        float3 c = max(source.read(uint2(min(base + int2(x, y), size - 1))).rgb, 0.);
        float peak = max(c.r, max(c.g, c.b));
        float excess = max(peak - post.bloom.x, 0.);
        if (knee > 0.) {
            float soft = clamp(peak - post.bloom.x + knee, 0., 2. * knee);
            excess = max(excess, soft * soft / (4. * knee));
        }
        // Karis weighting suppresses isolated HDR fireflies before downsampling.
        // These are linear Rec.709 luminance coefficients, not look tuning.
        float weight = 1. / (1. + dot(c, float3(0.2126, 0.7152, 0.0722)));
        sum += c * (excess / max(peak, 1e-6)) * weight;
        weights += weight;
    }
    return float4(min(sum / weights, 65504.), 1);
}
fragment float4 post_bloom_blur(Full i [[stage_in]], constant PostUniform& post [[buffer(0)]], texture2d<float> source [[texture(0)]]) {
    constexpr sampler linear(coord::normalized, address::clamp_to_edge, filter::linear);
    // Bilinear pairs implement the normalised [1,4,6,4,1]/16 binomial kernel.
    // A radius above roughly two texels spreads these taps apart and can ring;
    // wide bloom needs a mip chain, not a larger authored radius on this one level.
    float2 offset = post.blur.xy * post.bloom.y * 1.2 / float2(source.get_width(), source.get_height());
    float3 c = source.sample(linear, i.uv).rgb * 0.375;
    c += (source.sample(linear, i.uv - offset).rgb + source.sample(linear, i.uv + offset).rgb) * 0.3125;
    return float4(c, 1);
}
template<typename Depth>
float4 post_apply_value(Full i, constant Frame& f, constant PostUniform& post, texture2d<float> source, Depth depth, texture2d<float> ao, texture2d<float> bloom) {
    constexpr sampler linear(coord::normalized, address::clamp_to_edge, filter::linear);
    float4 c = source.sample(linear, i.uv);
    if (post.ao.w > 0.5) {
        float z = post_position(depth, int2(i.position.xy), f).w;
        if (z > 0.) {
            int2 size = int2(ao.get_width(), ao.get_height());
            float2 p = i.uv * float2(size) - 0.5;
            int2 base = int2(floor(p));
            float2 blend = fract(p);
            float weighted = 0., weights = 0.;
            for (int y = 0; y < 2; ++y) for (int x = 0; x < 2; ++x) {
                float2 tap = ao.read(uint2(clamp(base + int2(x, y), int2(0), size - 1))).rg;
                float weight = (x ? blend.x : 1. - blend.x) * (y ? blend.y : 1. - blend.y);
                weight *= tap.y > 0. ? saturate(1. - abs(tap.y - z) / post.ao.x) : 0.;
                weighted += tap.x * weight;
                weights += weight;
            }
            // This forward post pass can only attenuate the complete lit colour,
            // including direct/emissive light. Ambient-only AO needs scene-pass integration.
            c.rgb *= weights > 1e-6 ? weighted / weights : 1.;
        }
    }
    if (post.bloom.w > 0.5) c.rgb += bloom.sample(linear, i.uv).rgb * post.bloom.z;
    return float4(min(c.rgb, 65504.), c.a);
}
fragment float4 post_apply(Full i [[stage_in]], constant PostUniform& post [[buffer(0)]], constant Frame& f [[buffer(1)]], texture2d<float> source [[texture(0)]], depth2d<float> depth [[texture(1)]], texture2d<float> ao [[texture(2)]], texture2d<float> bloom [[texture(3)]]) {
    return post_apply_value(i, f, post, source, depth, ao, bloom);
}
fragment float4 post_apply_msaa(Full i [[stage_in]], constant PostUniform& post [[buffer(0)]], constant Frame& f [[buffer(1)]], texture2d<float> source [[texture(0)]], depth2d_ms<float> depth [[texture(1)]], texture2d<float> ao [[texture(2)]], texture2d<float> bloom [[texture(3)]]) {
    return post_apply_value(i, f, post, source, depth, ao, bloom);
}
