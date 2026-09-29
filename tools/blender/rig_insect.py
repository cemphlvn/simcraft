"""Rig and animate a six-legged insect mesh (a generated .glb), headless, and export it for the game.

    Blender -b --factory-startup --python tools/blender/rig_insect.py -- in.glb out_dir name [--tris 12000,3000,800]

The mesh must face -Y in Blender (glTF +Z, the spec's front) with +Z up, legs spread (as image-to-3D gives it).
The anatomy is read from the geometry: the six legs are the vertices far from the body's axis, split by side and
by position along the body; each leg gets a hip, knee and foot from its vertices; the head is the front, the
antennae what reaches past it. Bones get the names the game's contract uses (`leg_<front|mid|hind>_<l|r>_<upper|lower>`,
`thorax`, `abdomen`, `head`, `jaw_l`, `jaw_r`, `antenna_l`, `antenna_r`) and a socket node `carry` between the
mandibles. Clips: `walk` (alternating tripods), `carry` (walking, head up, mandibles shut), `dig` (head bobbing,
mandibles chewing), `idle` (antennae sweeping, abdomen breathing). Level-of-detail files `<name>_lod<k>.glb`.
"""

import math
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

args = sys.argv[sys.argv.index("--") + 1 :]
src, out_dir, name = args[0], Path(args[1]), args[2]
tris_levels = [12000, 3000, 800]
texture_size = 1024
if "--tris" in args:
    tris_levels = [int(t) for t in args[args.index("--tris") + 1].split(",")]
out_dir.mkdir(parents=True, exist_ok=True)
FPS = 24


def log(*a):
    print("RIG", *a, flush=True)


def fresh_mesh():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = FPS
    bpy.ops.import_scene.gltf(filepath=src)
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    for o in bpy.context.scene.objects:
        o.select_set(o in meshes)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    mesh = bpy.context.view_layer.objects.active
    # Unparent (the importer may put it under a root node), apply transforms, feet on z = 0.
    bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    zmin = min(v.co.z for v in mesh.data.vertices)
    for v in mesh.data.vertices:
        v.co.z -= zmin
    for o in list(bpy.context.scene.objects):
        if o is not mesh:
            bpy.data.objects.remove(o, do_unlink=True)
    return mesh


def decimate(mesh, target):
    have = sum(len(p.vertices) - 2 for p in mesh.data.polygons)
    if have > target:
        mod = mesh.modifiers.new("decimate", "DECIMATE")
        mod.ratio = target / have
        bpy.ops.object.modifier_apply(modifier=mod.name)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.quads_convert_to_tris()
    bpy.ops.object.mode_set(mode="OBJECT")
    return sum(len(p.vertices) - 2 for p in mesh.data.polygons)


def shrink_textures(size):
    for img in bpy.data.images:
        if img.size[0] > size:
            img.scale(size, size)


def anatomy(mesh):
    """Hips, knees, feet of six legs; the head, jaw and antenna points; from the vertices."""
    vs = [v.co.copy() for v in mesh.data.vertices]
    ys = sorted(v.y for v in vs)
    y0, y1 = ys[0], ys[-1]
    length = y1 - y0
    zs = sorted(v.z for v in vs)
    # The body's half-width along its length: the median |x| of the vertices in each slice, and its core height.
    slices = 40
    core = []
    for i in range(slices):
        a, b = y0 + length * i / slices, y0 + length * (i + 1) / slices
        xs = sorted(abs(v.x) for v in vs if a <= v.y < b)
        core.append(xs[len(xs) // 2] if xs else 0.0)
    body_w = sorted(core)[len(core) * 3 // 4]
    legs_pts = [v for v in vs if abs(v.x) > body_w * 1.6 and v.z < zs[len(zs) * 9 // 10]]
    # Front 20% of the body is the head region (antennae reach past it): leg vertices there are antennae.
    head_front = y0 + length * 0.22
    antenna_pts = [v for v in legs_pts if v.y < head_front]
    legs_pts = [v for v in legs_pts if v.y >= head_front]
    legs = {}
    for side, sign in (("l", 1), ("r", -1)):
        pts = sorted((v for v in legs_pts if v.x * sign > 0), key=lambda v: v.y)
        # Three groups along the body: split at the two widest gaps in y between the leg tips' reach.
        tips = sorted(pts, key=lambda v: -abs(v.x))[: max(3, len(pts) // 12)]
        tip_ys = sorted(v.y for v in tips)
        gaps = sorted(range(1, len(tip_ys)), key=lambda i: tip_ys[i] - tip_ys[i - 1])[-2:]
        cuts = sorted(tip_ys[i] for i in gaps)
        groups = [[v for v in pts if v.y < cuts[0]], [v for v in pts if cuts[0] <= v.y < cuts[1]], [v for v in pts if v.y >= cuts[1]]]
        for which, g in zip(("front", "mid", "hind"), groups, strict=True):
            if not g:
                continue
            foot = max(g, key=lambda v: abs(v.x) + 0.3 * abs(v.y - sum(p.y for p in g) / len(g)))
            inner = min(g, key=lambda v: abs(v.x))
            knee = max(g, key=lambda v: v.z)
            hip = Vector((sign * body_w * 0.9, inner.y, max(inner.z, knee.z * 0.6)))
            legs[f"{which}_{side}"] = (hip, knee.copy(), foot.copy())
    thorax_y = sum(leg[0].y for leg in legs.values()) / max(len(legs), 1)
    mid_z = (zs[0] + zs[-1]) / 2
    head = Vector((0, y0 + length * 0.14, mid_z))
    return {
        "legs": legs,
        "antenna_pts": antenna_pts,
        "body_w": body_w,
        "y0": y0,
        "y1": y1,
        "length": length,
        "thorax": Vector((0, thorax_y, mid_z)),
        "head": head,
        "neck": Vector((0, y0 + length * 0.3, mid_z)),
        "abdomen_end": Vector((0, y1 - length * 0.03, mid_z)),
        "jaw": Vector((0, y0 + length * 0.04, mid_z * 0.7)),
    }


def build_rig(mesh, an, source):
    arm_data = bpy.data.armatures.new(name)
    arm = bpy.data.objects.new(name + "_rig", arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    eb = arm_data.edit_bones

    def bone(n, head, tail, parent=None, connect=False):
        b = eb.new(n)
        b.head, b.tail = head, tail
        if (b.tail - b.head).length < 1e-4:
            b.tail = b.head + Vector((0, 0, 0.01))
        if parent:
            b.parent = eb[parent]
            b.use_connect = connect
        return b

    root = Vector((0, an["thorax"].y, 0))
    bone("root", root, root + Vector((0, 0, an["thorax"].z * 0.5)))
    bone("thorax", an["thorax"], an["neck"], "root")
    bone("head", an["neck"], an["head"], "thorax")
    bone("abdomen", an["thorax"], an["abdomen_end"], "root")
    for s, sign in (("l", 1), ("r", -1)):
        jaw_root = an["head"] + Vector((sign * an["body_w"] * 0.35, -an["length"] * 0.02, -an["head"].z * 0.25))
        bone(f"jaw_{s}", jaw_root, an["jaw"] + Vector((sign * an["body_w"] * 0.1, 0, 0)), "head")
        pts = [v for v in an["antenna_pts"] if v.x * sign > 0]
        if pts:
            tip = min(pts, key=lambda v: v.y)
            base = an["head"] + Vector((sign * an["body_w"] * 0.5, 0, an["head"].z * 0.15))
            midp = base.lerp(tip, 0.5) + Vector((0, 0, an["length"] * 0.02))
            bone(f"antenna_{s}", base, midp, "head")
            bone(f"antenna_{s}_tip", midp, tip, f"antenna_{s}", True)
    for key, (hip, knee, foot) in an["legs"].items():
        which, s = key.split("_")
        parent = "thorax"
        bone(f"leg_{which}_{s}_upper", hip, knee, parent)
        bone(f"leg_{which}_{s}_lower", knee, foot, f"leg_{which}_{s}_upper", True)
    bpy.ops.object.mode_set(mode="OBJECT")
    # The socket: an empty in the mandibles, riding on the head.
    carry = bpy.data.objects.new("carry", None)
    bpy.context.scene.collection.objects.link(carry)
    carry.parent = arm
    carry.parent_type = "BONE"
    carry.parent_bone = "head"
    carry.matrix_world = Matrix.Translation(an["jaw"] + Vector((0, -an["length"] * 0.04, 0)))
    bind(mesh, arm, source)
    used = {g.group for v in mesh.data.vertices for g in v.groups if g.weight > 0.01}
    empty = [g.name for g in mesh.vertex_groups if g.index not in used]
    log("bones", len(arm_data.bones), "legs found", sorted(an["legs"]), "bones that move no vertex", empty)
    return arm


def select(active, *others):
    for o in bpy.context.scene.objects:
        o.select_set(False)
    for o in (active, *others):
        o.select_set(True)
    bpy.context.view_layer.objects.active = active


def bind(mesh, arm, source):
    """Weights by distance to the bones: each vertex follows its nearest bone segment, blending smoothly into the
    next nearest where two meet (inverse distance to the 4th power). Deterministic and independent of the mesh's
    topology: Blender's heat solver fails ("failed to find solution") on generated meshes, and an insect's limbs are
    separated enough for distance to be right. `source` is unused (kept for a solver that needs the full mesh)."""
    del source
    select(arm, mesh)
    bpy.ops.object.parent_set(type="ARMATURE_NAME")
    segments = [(b.name, b.head_local.copy(), b.tail_local.copy()) for b in arm.data.bones if b.name != "root"]

    def dist(p, a, b):
        ab = b - a
        t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.length_squared, 1e-12)))
        return (p - (a + ab * t)).length

    groups = {g.name: g for g in mesh.vertex_groups}
    for v in mesh.data.vertices:
        ds = sorted((dist(v.co, a, b), n) for n, a, b in segments)
        near = [(d, n) for d, n in ds[:3] if d <= ds[0][0] * 1.35 + 1e-4]
        ws = [(1.0 / (d + 1e-4) ** 4, n) for d, n in near]
        total = sum(w for w, _ in ws)
        for w, n in ws:
            groups[n].add([v.index], w / total, "REPLACE")


def rot_world(arm, bone_name, axis, angle):
    """A pose rotation about an armature-space axis through the bone's head."""
    b = arm.data.bones[bone_name]
    rest = b.matrix_local.to_3x3()
    r = Matrix.Rotation(angle, 3, Vector(axis))
    return (rest.inverted() @ r @ rest).to_quaternion()


def key(arm, frame, poses):
    for pb in arm.pose.bones:
        pb.rotation_mode = "QUATERNION"
    for bone_name, q in poses.items():
        if q is not None and bone_name in arm.pose.bones:
            pb = arm.pose.bones[bone_name]
            pb.rotation_quaternion = q
            pb.keyframe_insert("rotation_quaternion", frame=frame)


def combine(arm, bone_name, turns):
    if bone_name not in arm.data.bones:
        return None
    q = None
    for axis, angle in turns:
        r = rot_world(arm, bone_name, axis, angle)
        q = r if q is None else r @ q
    return q


def clip(arm, clip_name, seconds, pose_at):
    """Keys every bone each frame from `pose_at(phase in 0..1)` into a new action."""
    action = bpy.data.actions.new(clip_name)
    arm.animation_data_create()
    arm.animation_data.action = action
    frames = int(seconds * FPS)
    for f in range(frames + 1):
        key(arm, f, pose_at(f / frames))
    arm.animation_data.action = None
    track = arm.animation_data.nla_tracks.new()
    track.name = clip_name
    track.strips.new(clip_name, 0, action)
    return action


def author(arm, an):
    tau = math.tau
    legs = [pb.name for pb in arm.pose.bones if pb.name.startswith("leg_") and pb.name.endswith("_upper")]
    tripod_a = {"leg_front_l_upper", "leg_mid_r_upper", "leg_hind_l_upper"}

    def gait(p, stride=0.35, lift=0.35, head_up=0.0, jaw_open=0.15):
        poses = {}
        for n in legs:
            side = 1 if "_l_" in n else -1
            ph = (p + (0.0 if n in tripod_a else 0.5)) % 1.0
            # Swing (first half): forward and up; stance: back, on the ground.
            if ph < 0.5:
                s = math.sin(ph / 0.5 * math.pi)
                swing = -stride + 2 * stride * (ph / 0.5)
                up = lift * s
            else:
                swing = stride - 2 * stride * ((ph - 0.5) / 0.5)
                up = 0.0
            # Forward is -Y: a leg swings forward by turning about Z (sign by side), lifts about Y.
            poses[n] = combine(arm, n, [((0, 0, 1), -side * swing * 0.6), ((0, 1, 0), -side * up)])
            lower = n.replace("_upper", "_lower")
            poses[lower] = combine(arm, lower, [((0, 1, 0), side * up * 0.8)])
        bob = math.sin(p * 2 * tau) * 0.03
        poses["thorax"] = combine(arm, "thorax", [((1, 0, 0), bob)])
        poses["head"] = combine(arm, "head", [((1, 0, 0), -head_up + math.sin(p * tau) * 0.04), ((0, 0, 1), math.sin(p * tau) * 0.05)])
        poses["abdomen"] = combine(arm, "abdomen", [((0, 0, 1), -math.sin(p * tau) * 0.06), ((1, 0, 0), bob * 0.5)])
        for s, sign in (("l", 1), ("r", -1)):
            poses[f"jaw_{s}"] = combine(arm, f"jaw_{s}", [((0, 0, 1), sign * jaw_open)])
            poses[f"antenna_{s}"] = combine(
                arm, f"antenna_{s}", [((1, 0, 0), 0.15 * math.sin(p * 2 * tau + sign)), ((0, 0, 1), sign * 0.12 * math.sin(p * tau))]
            )
            poses[f"antenna_{s}_tip"] = combine(arm, f"antenna_{s}_tip", [((1, 0, 0), 0.2 * math.sin(p * 2 * tau + sign + 0.6))])
        return poses

    clip(arm, "walk", 1.0, lambda p: gait(p))
    clip(arm, "carry", 1.2, lambda p: gait(p, stride=0.28, lift=0.3, head_up=0.25, jaw_open=-0.05))

    def dig(p):
        poses = gait(0.0, stride=0.0, lift=0.0)
        poses["head"] = combine(arm, "head", [((1, 0, 0), 0.35 * (0.5 + 0.5 * math.sin(p * 2 * tau)))])
        poses["thorax"] = combine(arm, "thorax", [((1, 0, 0), 0.1 * math.sin(p * 2 * tau))])
        for s, sign in (("l", 1), ("r", -1)):
            poses[f"jaw_{s}"] = combine(arm, f"jaw_{s}", [((0, 0, 1), sign * 0.3 * (0.5 + 0.5 * math.sin(p * 4 * tau)))])
        return poses

    clip(arm, "dig", 1.0, dig)

    def idle(p):
        poses = gait(0.0, stride=0.0, lift=0.0, jaw_open=0.05 + 0.05 * math.sin(p * tau))
        for s, sign in (("l", 1), ("r", -1)):
            poses[f"antenna_{s}"] = combine(
                arm, f"antenna_{s}", [((1, 0, 0), 0.3 * math.sin(p * tau + sign)), ((0, 0, 1), sign * 0.3 * math.sin(p * 2 * tau))]
            )
        poses["abdomen"] = combine(arm, "abdomen", [((1, 0, 0), 0.04 * math.sin(p * tau))])
        return poses

    clip(arm, "idle", 3.0, idle)


def export(path):
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        export_animations=True,
        export_animation_mode="NLA_TRACKS",
        export_skins=True,
        export_yup=True,
        export_apply=False,
        export_image_format="JPEG",
        export_jpeg_quality=90,
    )


for level, target in enumerate(tris_levels):
    mesh = fresh_mesh()
    source = mesh.copy()
    source.data = mesh.data.copy()
    shrink_textures(texture_size >> level if level < 2 else 256)
    got = decimate(mesh, target)
    an = anatomy(mesh)
    arm = build_rig(mesh, an, source)
    author(arm, an)
    path = out_dir / (f"{name}.glb" if level == 0 else f"{name}_lod{level}.glb")
    export(path)
    log(f"lod{level}", got, "triangles, length", round(an["length"], 3), "->", path, f"{path.stat().st_size // 1024} KB")
