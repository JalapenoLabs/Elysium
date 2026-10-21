# Copyright © 2026 Jalapeno Labs
#
# Exports the open .blend to a binary glTF, run inside Blender by /opt/elysium/bin/export-glb:
#
#   blender --background --factory-startup <file.blend> --python export_glb.py -- <output.glb>
#
# Fixed options, so the same .blend always exports the same way whoever made it: the whole scene,
# modifiers applied (what the agent rendered is what the viewer shows), +Y up as glTF expects,
# and no cameras or lights, which are staging for renders rather than part of the asset.

import sys

import bpy

output = sys.argv[sys.argv.index("--") + 1]
bpy.ops.export_scene.gltf(
    filepath=output,
    export_format="GLB",
    use_selection=False,
    export_apply=True,
    export_yup=True,
    export_cameras=False,
    export_lights=False,
)
