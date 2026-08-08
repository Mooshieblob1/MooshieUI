# MiniMax H3 Director, vendored into MooshieUI from ComfyUI-MiniMaxH3-Director v0.1.5.
# Upstream: https://github.com/seesee75-commits/ComfyUI-MiniMaxH3-Director (GPL-3.0)
# See LICENSE in this directory for the full GPL-3.0 text.
#
# This file is a MooshieUI rewrite of the upstream __init__.py. What differs:
#
#   * Only the two nodes MooshieUI drives are registered. MiniMaxH3EnhancePrompt and
#     MiniMaxH3PreviewOverride are not vendored: MooshieUI has its own prompt assistant
#     and its own generation preview, so both would be dead weight.
#   * node_ids are namespaced "MooshieH3*" rather than upstream's "MiniMaxH3*CS", so
#     this copy cannot collide if a user also installs the upstream pack via ComfyUI
#     Manager. Both can coexist.
#   * No WEB_DIRECTORY. The upstream js/ timeline editor (~540 KB) only runs on
#     ComfyUI's graph canvas, which MooshieUI never renders. MooshieUI builds the
#     timeline_data JSON itself and passes it as a plain widget value.
#
# MiniMaxH3DirectorChain is likewise not vendored. Upstream withdrew it because there
# was no way to hand it a timeline from the canvas; that specific blocker does not apply
# here, but its other tradeoffs (it swallows the sampler, losing live preview,
# per-window progress and clean interruption) still do. Revisit for long-form video.

from comfy_api.latest import ComfyExtension, io
from typing_extensions import override

from .minimax_director import MiniMaxH3Director
from .minimax_retake import MiniMaxH3RetakeStitch


class MooshieH3DirectorExtension(ComfyExtension):
    @override
    async def get_node_list(self) -> list[type[io.ComfyNode]]:
        return [MiniMaxH3Director, MiniMaxH3RetakeStitch]


async def comfy_entrypoint() -> MooshieH3DirectorExtension:
    return MooshieH3DirectorExtension()


NODE_CLASS_MAPPINGS = {
    "MooshieH3Director": MiniMaxH3Director,
    "MooshieH3RetakeStitch": MiniMaxH3RetakeStitch,
}

NODE_DISPLAY_NAME_MAPPINGS = {
    "MooshieH3Director": "MiniMax H3 Director",
    "MooshieH3RetakeStitch": "MiniMax H3 Retake Stitch",
}

__all__ = ["NODE_CLASS_MAPPINGS", "NODE_DISPLAY_NAME_MAPPINGS"]
