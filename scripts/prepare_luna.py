"""Rebuild Luna GLB from the pinned VRM. Requires Python 3 + Pillow.
Run from any directory: python scripts/prepare_luna.py
No VRM runtime: planar arm motions also animate the matching aim helpers.
"""
import copy
import hashlib
import io
import json
import math
from pathlib import Path
import struct
from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
source = ROOT / 'assets/luna/base.vrm'
raw = source.read_bytes()
SOURCE_SHA = '12c2b97e95e700783a6a550dc0eee2d7880aeedccef9ae67bc4c5a2f0f2631a2'
assert hashlib.sha256(raw).hexdigest() == SOURCE_SHA, 'Unexpected source asset'
length = struct.unpack_from('<I', raw, 12)[0]
g = json.loads(raw[20:20 + length])
old_binary = raw[28 + length:]
old_views = g['bufferViews']
old_accessors = g['accessors']
old_images = g['images']
old_textures = g['textures']
meta = copy.deepcopy(g['extensions']['VRMC_vrm']['meta'])
source_url = 'https://github.com/pixiv/three-vrm/blob/1b4fc0cc7ef39a49d62bb7a66dcfeca8f65316f7/packages/three-vrm/examples/models/VRM1_Constraint_Twist_Sample.vrm'
notice = {
    'source': source_url, 'sourceSha256': SOURCE_SHA,
    'originalLicenseSettings': meta,
    'adapterLicense': 'VRM Public License 1.0 with the same settings as the original',
    'licenseUrl': meta['licenseUrl'],
    'modifications': 'Luna M0-B: cool palette, reduced textures, chest/ear accents, smaller head and dark leggings, standard glTF materials, Idle/Wave clips, no morph targets or VRM dynamics.',
    'disclaimer': 'Provided without warranties; no endorsement by pixiv Inc.',
}
g['asset']['copyright'] = '(c) 2022 pixiv Inc.; adapted for Assistente-3D (Luna M0-B)'
g['asset']['extras'] = notice
g.pop('extensions', None)
g['extensionsUsed'] = ['KHR_materials_unlit']
g.pop('extensionsRequired', None)
g['bufferViews'], g['accessors'], g['images'], g['textures'] = [], [], [], []
binary = bytearray()

def view(data, **extra):
    binary.extend(b'\0' * (-len(binary) % 4))
    idx = len(g['bufferViews'])
    g['bufferViews'].append(dict(buffer=0, byteOffset=len(binary), byteLength=len(data), **extra))
    binary.extend(data)
    return idx

views, accessors = {}, {}
def copy_view(idx):
    if idx not in views:
        v = old_views[idx]; start = v.get('byteOffset', 0)
        views[idx] = view(old_binary[start:start+v['byteLength']], **{k:v[k] for k in ('byteStride','target') if k in v})
    return views[idx]

def copy_accessor(idx):
    if idx not in accessors:
        a = copy.deepcopy(old_accessors[idx]); assert 'sparse' not in a
        a['bufferView'] = copy_view(a['bufferView'])
        accessors[idx] = len(g['accessors']); g['accessors'].append(a)
    return accessors[idx]

for mesh in g['meshes']:
    mesh.pop('weights', None); mesh.pop('extras', None)
    for p in mesh['primitives']:
        p.pop('targets', None)
        p['indices'] = copy_accessor(p['indices'])
        p['attributes'] = {k:copy_accessor(v) for k,v in p['attributes'].items()}
for skin in g['skins']:
    skin['inverseBindMatrices'] = copy_accessor(skin['inverseBindMatrices'])
for node in g['nodes']:
    node.pop('extensions', None); node.pop('weights', None)

# Use inexpensive unlit materials, preserving baked facial details.
for mat in g['materials']:
    name = mat['name']; pbr = mat['pbrMetallicRoughness']
    idx = old_textures[pbr['baseColorTexture']['index']]['source']
    v = old_views[old_images[idx]['bufferView']]; start = v.get('byteOffset',0)
    im = Image.open(io.BytesIO(old_binary[start:start+v['byteLength']])).convert('RGBA')
    im.thumbnail((512,512), Image.Resampling.LANCZOS)
    colors = None
    if 'HAIR' in name: colors = ('#555574','#e0e2f4')
    elif 'EyeIris' in name: colors = ('#20143c','#a48fe5')
    elif 'Tops' in name: colors = ('#111827','#303c58')
    elif 'Bottoms' in name: colors = ('#151927','#66708e')
    elif 'Shoes' in name: colors = ('#171d2c','#727d9e')
    if colors:
        alpha=im.getchannel('A')
        grey=ImageOps.grayscale(im)
        if 'HAIR' in name: grey=ImageOps.autocontrast(grey)
        im=ImageOps.colorize(grey,*colors).convert('RGBA'); im.putalpha(alpha)
    out=io.BytesIO();im.save(out,format='PNG',optimize=True)
    image_idx=len(g['images']);g['images'].append({'bufferView':view(out.getvalue()),'mimeType':'image/png','name':name})
    tex_idx=len(g['textures']);g['textures'].append({'source':image_idx})
    pbr.update(baseColorTexture={'index':tex_idx},metallicFactor=0,roughnessFactor=1)
    mat['extensions']={'KHR_materials_unlit':{}}
    mat.pop('normalTexture',None);mat.pop('emissiveTexture',None)
    mat['doubleSided']=True

# Cover the legs with graphite fabric; split only triangles below the shorts.
leggings = len(g['materials'])
g['materials'].append({'name':'Luna_Graphite_Leggings','pbrMetallicRoughness':{'baseColorFactor':[.07,.085,.14,1],'metallicFactor':0,'roughnessFactor':1},'extensions':{'KHR_materials_unlit':{}}})
def read_values(idx):
    a=g['accessors'][idx];v=g['bufferViews'][a['bufferView']]
    width={'SCALAR':1,'VEC3':3}[a['type']];fmt={5123:'H',5125:'I',5126:'f'}[a['componentType']]
    start=v.get('byteOffset',0)+a.get('byteOffset',0);stride=v.get('byteStride',struct.calcsize(fmt)*width)
    return [struct.unpack_from('<'+fmt*width,binary,start+i*stride) for i in range(a['count'])]
def indices(values):
    idx=len(g['accessors']);g['accessors'].append({'bufferView':view(struct.pack('<'+'I'*len(values),*values),target=34963),'componentType':5125,'count':len(values),'type':'SCALAR'});return idx
body=g['meshes'][0]['primitives'][0]
positions=read_values(body['attributes']['POSITION']);ids=[v[0] for v in read_values(body['indices'])]
upper,lower=[],[]
for i in range(0,len(ids),3):
    tri=ids[i:i+3]
    (lower if all(positions[k][1]<.77 for k in tri) else upper).extend(tri)
body['indices']=indices(upper)
g['meshes'][0]['primitives'].append(dict(attributes=body['attributes'].copy(),indices=indices(lower),material=leggings))
# Slightly more adult proportions; all head-weighted geometry follows the same joint.
g['nodes'][25]['scale']=[v*.90 for v in g['nodes'][25].get('scale',[1,1,1])]

def floats(values, width, bounds=False):
    data=struct.pack('<'+'f'*len(values),*values)
    a={'bufferView':view(data),'componentType':5126,'count':len(values)//width,'type':{1:'SCALAR',3:'VEC3',4:'VEC4'}[width]}
    if bounds:
        a['min']=[min(values[k::width]) for k in range(width)]
        a['max']=[max(values[k::width]) for k in range(width)]
    idx=len(g['accessors']);g['accessors'].append(a);return idx

# Small original octahedron: digital pendant; rigidly follows upper chest.
accent=len(g['materials']);g['materials'].append({'name':'Luna_Cyan_Core','pbrMetallicRoughness':{'baseColorFactor':[.28,.85,.95,1],'metallicFactor':0,'roughnessFactor':1},'extensions':{'KHR_materials_unlit':{}}})
verts=[(0,.022,0),(.012,0,0),(0,-.022,0),(-.012,0,0),(0,0,.008),(0,0,-.008)]
faces=[(0,1,4),(1,2,4),(2,3,4),(3,0,4),(1,0,5),(2,1,5),(3,2,5),(0,3,5)]
pos=[v for tri in faces for i in tri for v in verts[i]]
mesh=len(g['meshes']);g['meshes'].append({'name':'Luna_Core','primitives':[{'attributes':{'POSITION':floats(pos,3,True)},'material':accent}]})
node=len(g['nodes']);g['nodes'].append({'name':'Luna_Core','mesh':mesh,'translation':[0,.067,.092]});g['nodes'][23]['children'].append(node)
for side in [-1,1]:
    ear=len(g['nodes']);g['nodes'].append({'name':f'Luna_Ear_{side}','mesh':mesh,'translation':[side*.083,.108,-.003],'scale':[.45,.5,.6]});g['nodes'][24]['children'].append(ear)

# Local clips. Rotations are relative to the source rest transforms.
def mul(a,b):
    x,y,z,w=a;X,Y,Z,W=b
    return [w*X+x*W+y*Z-z*Y,w*Y-x*Z+y*W+z*X,w*Z+x*Y-y*X+z*W,w*W-x*X-y*Y-z*Z]
def rot(axis,degrees):
    r=math.radians(degrees)/2;q=[0.,0.,0.,math.cos(r)];q[axis]=math.sin(r);return q
rest=[n.get('rotation',[0,0,0,1])[:] for n in g['nodes']]
# The three parallel upper-arm branches deform the arm, sleeve and shoulder.
arm_groups=[([96,89,91],-1),([130,123,125],1)]
def relative(idx,*turns):
    q=[0.,0.,0.,1.]
    for axis,degrees in turns:q=mul(q,rot(axis,degrees))
    return mul(q,rest[idx])

def ease(t,start,end):
    u=max(0.,min(1.,(t-start)/(end-start)))
    return u*u*(3-2*u)

def idle_pose(t):
    phase=math.tau*t/6
    breath=math.sin(phase)
    sway=math.sin(phase)*.65+math.sin(2*phase+.5)*.17-math.sin(.5)*.17
    result={
        21:relative(21,(2,sway*.35)),
        22:relative(22,(0,breath*.85),(2,sway*.4)),
        23:relative(23,(0,breath*.4),(2,-sway*.3)),
        24:relative(24,(0,-breath*.25),(1,math.sin(2*phase)*.55)),
        25:relative(25,(2,-sway*.45+math.sin(3*phase)*.22)),
        88:relative(88,(2,-.8+breath*.35)),
        122:relative(122,(2,.8-breath*.35)),
        132:relative(132),
        153:relative(153),
    }
    for joints,sign in arm_groups:
        angle=sign*(76+breath*.7-sway*.5 if sign==1 else 76+breath*.7+sway*.5)
        for joint in joints: result[joint]=relative(joint,(2,angle))
    result[97]=relative(97,(2,11+breath*.7))
    result[131]=relative(131,(2,-13-breath*.7))
    return result

def wave_pose(t):
    # Smooth, staggered shoulder/upper-arm/elbow arcs; the wrist supplies the greeting.
    result=idle_pose(0)
    shoulder=ease(t,.08,.70)*(1-ease(t,2.52,3.34))
    upper=ease(t,.18,1.03)*(1-ease(t,2.38,3.38))
    elbow=ease(t,.38,1.13)*(1-ease(t,2.28,3.36))
    greeting=ease(t,1.03,1.31)*(1-ease(t,2.22,2.53))
    head=ease(t,.48,1.23)*(1-ease(t,2.35,3.25))
    wrist=math.sin((t-1.30)*math.tau*2.1)*greeting
    result[122]=relative(122,(1,3*shoulder),(2,.8+4*shoulder))
    for joint in arm_groups[1][0]:
        result[joint]=relative(joint,(1,10*upper),(2,76-40*upper))
    result[131]=relative(131,(2,-13-82*elbow+7*wrist))
    for joint in (132,153):result[joint]=relative(joint,(2,10*wrist))
    result[23]=relative(23,(0,.7*upper),(2,-1.1*upper))
    result[24]=relative(24,(0,-1.4*head),(1,1.5*head))
    result[25]=relative(25,(2,-.9*head))
    return result

animated=list(wave_pose(0))
# Match static pose to the first frame, so bounding and initial display use lowered arms.
for idx,q in wave_pose(0).items(): g['nodes'][idx]['rotation']=q
clips=[]
for name,duration in [('Idle',6),('Wave',3.6)]:
    times=[i/30 for i in range(round(duration*30)+1)]
    time_accessor=floats(times,1,True)
    clip={'name':name,'samplers':[],'channels':[]}
    poses=[wave_pose(t) if name=='Wave' else idle_pose(t) for t in times]
    for idx in animated:
        clip['channels'].append({'sampler':len(clip['samplers']),'target':{'node':idx,'path':'rotation'}})
        clip['samplers'].append({'input':time_accessor,'output':floats([v for p in poses for v in p[idx]],4),'interpolation':'LINEAR'})
    clips.append(clip)
g['animations']=clips
g['buffers']=[{'byteLength':len(binary)}]
js=json.dumps(g,separators=(',',':')).encode();js+=b' '*(-len(js)%4)
binary.extend(b'\0'*(-len(binary)%4))
result=struct.pack('<III',0x46546c67,2,28+len(js)+len(binary))+struct.pack('<II',len(js),0x4e4f534a)+js+struct.pack('<II',len(binary),0x004e4942)+binary
output=ROOT/'public/models/Luna.glb';output.write_bytes(result)
(ROOT/'public/models/Luna.LICENSE.json').write_text(json.dumps(notice,indent=2)+'\n')
print(f'{output}: {len(result)} bytes; SHA256 {hashlib.sha256(result).hexdigest()}')
