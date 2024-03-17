"""Script for exporting Blender scenes
in an extended glTF format used by Starframe."""

import bpy
import sys
from bpy.app.handlers import persistent


@persistent
def export(*_):
    # save into the current directory if no extra arguments given,
    # otherwise into the given directory
    # (but always with a file name derived from the blend file)
    target_dir = "."
    # save as glTF without textures for inspectable results
    is_debug_mode = False
    try:
        args = sys.argv[sys.argv.index("--") + 1:]
        if not args[0].startswith("-"):
            target_dir = args[0].rstrip("/")
        if "--debug" in args:
            is_debug_mode = True
    except Exception:
        # no parameters given, use defaults
        pass

    blend_file = bpy.path.basename(bpy.data.filepath)
    target_file = blend_file.removesuffix(".blend")

    # remove contents of linked instance collections from this file
    # and replace them with links to the original file
    for obj in bpy.data.objects:
        if not obj.instance_collection or not obj.instance_collection.library:
            continue
        obj["link"] = {
            "source": obj.instance_collection.library.name.removesuffix(".blend"),
            "name": obj.instance_collection.name,
        }
        for contained_obj in obj.instance_collection.objects:
            bpy.data.objects.remove(contained_obj)

    bpy.ops.export_scene.gltf(
        # extension gets set automatically depending on `export_format`
        filepath=f"{target_dir}/{target_file}",
        check_existing=False,
        export_format="GLB" if not is_debug_mode else "GLTF_SEPARATE",
        export_image_format="AUTO" if not is_debug_mode else "NONE",
        use_visible=False,  # all objects, not just visible ones (useful for asset collections)
        export_extras=True,  # custom properties
        export_skins=True,
        export_animations=True,
        export_def_bones=True,  # exclude control-only bones
        export_morph=True,
        export_optimize_animation_size=True,
        export_yup=False,  # no coordinate transformations, we model in the xy plane
        export_texcoords=True,
        export_normals=True,
        export_tangents=False,
        export_morph_normal=True,
        export_morph_tangent=False,
        export_materials="EXPORT",
        export_colors=False,
    )


bpy.app.handlers.load_post.append(export)
