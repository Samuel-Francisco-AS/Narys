"""Run: blender -b -t 2 --python scripts/preview_luna.py"""
from pathlib import Path
import bpy
ROOT = Path(__file__).resolve().parents[1]
from mathutils import Vector
bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
bpy.ops.import_scene.gltf(filepath=str(ROOT/'public/models/Luna.glb'))
for o in bpy.data.objects:
 if o.animation_data: o.animation_data.action=None
scene=bpy.context.scene
scene.render.engine='CYCLES';scene.cycles.samples=16
scene.world.color=(.07,.09,.15)
bpy.ops.object.camera_add(location=(0,-3.5,1.05));cam=bpy.context.object
cam.rotation_euler=(Vector((0,0,.83))-cam.location).to_track_quat('-Z','Y').to_euler();cam.data.type='ORTHO';cam.data.ortho_scale=1.9;scene.camera=cam
scene.render.resolution_x=640;scene.render.resolution_y=800;scene.render.resolution_percentage=100
scene.view_settings.view_transform='Standard'
scene.render.filepath=str(ROOT/'docs/luna-preview.png');bpy.ops.render.render(write_still=True)
