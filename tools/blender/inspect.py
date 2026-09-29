"""Look at a .glb in Blender, headless: size, triangles, materials and textures, bones and clips, and preview
renders from three sides (to judge a generated model before rigging it).

    Blender -b --factory-startup --python tools/blender/inspect.py -- model.glb out_prefix
"""

import math
import sys

import bpy
from mathutils import Vector

args = sys.argv[sys.argv.index("--") + 1 :]
path, out = args[0], args[1]

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=path)
meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in meshes)
corners = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
lo = Vector([min(c[i] for c in corners) for i in range(3)])
hi = Vector([max(c[i] for c in corners) for i in range(3)])
print(f"INSPECT meshes {len(meshes)} triangles {tris}")
print(
    f"INSPECT bounds min {tuple(round(v, 3) for v in lo)} max {tuple(round(v, 3) for v in hi)} size {tuple(round(v, 3) for v in hi - lo)}"
)
for m in bpy.data.materials:
    texs = [
        n.image.name + f" {n.image.size[0]}x{n.image.size[1]}"
        for n in (m.node_tree.nodes if m.use_nodes else [])
        if n.type == "TEX_IMAGE" and n.image
    ]
    print(f"INSPECT material {m.name}: {texs}")
for a in bpy.data.armatures:
    print(f"INSPECT armature {a.name}: {len(a.bones)} bones {[b.name for b in a.bones][:24]}")
for a in bpy.data.actions:
    print(f"INSPECT clip {a.name}: frames {tuple(a.frame_range)}")

# Previews: top, side, three-quarter, soft light, white world.
scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x = scene.render.resolution_y = 768
scene.world = bpy.data.worlds.new("w")
scene.world.use_nodes = True
scene.world.node_tree.nodes["Background"].inputs[0].default_value = (1, 1, 1, 1)
scene.world.node_tree.nodes["Background"].inputs[1].default_value = 1.0
sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
sun.data.energy = 3.0
sun.rotation_euler = (math.radians(40), 0, math.radians(30))
scene.collection.objects.link(sun)
centre = (lo + hi) / 2
size = max(hi - lo)
cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
cam.data.type = "ORTHO"
cam.data.ortho_scale = size * 1.15
scene.collection.objects.link(cam)
scene.camera = cam
for name, direction in {"top": (0, 0, 1), "side": (1, 0, 0), "front": (0, -1, 0), "three": (1, -1, 0.8)}.items():
    d = Vector(direction).normalized()
    cam.location = centre + d * size * 3
    cam.rotation_euler = (centre - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = f"{out}_{name}.png"
    bpy.ops.render.render(write_still=True)
    print(f"INSPECT render {scene.render.filepath}")

# Clips: four frames of each (three-quarter view), to see the rig move. The importer puts clips on NLA tracks.
arm = next((o for o in scene.objects if o.type == "ARMATURE"), None)
if arm and arm.animation_data:
    d = Vector((1, -1, 0.8)).normalized()
    cam.location = centre + d * size * 3
    cam.rotation_euler = (centre - cam.location).to_track_quat("-Z", "Y").to_euler()
    for track in arm.animation_data.nla_tracks:
        track.mute = True
    for track in arm.animation_data.nla_tracks:
        if not track.strips:
            continue
        track.mute = False
        strip = track.strips[0]
        for k in range(4):
            frame = int(strip.frame_start + (strip.frame_end - strip.frame_start) * k / 4)
            scene.frame_set(frame)
            scene.render.filepath = f"{out}_clip_{track.name}_{k}.png"
            bpy.ops.render.render(write_still=True)
        track.mute = True
        print(f"INSPECT clip rendered {track.name}")
