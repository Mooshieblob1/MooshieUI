import { checkNodeAvailable, isCustomNodeInstalled } from "./api.js";

async function nodePackageAvailable(packageName: string, verifyNode: string): Promise<boolean> {
  try {
    const installed = await isCustomNodeInstalled(packageName);
    if (installed) return true;
    return await checkNodeAvailable(verifyNode);
  } catch {
    return false;
  }
}

/** Returns true when Untwisting RoPE style transfer custom nodes are loaded in ComfyUI. */
export async function checkStyleTransferNodesReady(): Promise<boolean> {
  const [untwisting, scaleImage] = await Promise.all([
    nodePackageAvailable("ComfyUi-Untwisting-RoPE", "RFInversion"),
    nodePackageAvailable(
      "ComfyUi-Scale-Image-to-Total-Pixels-Advanced",
      "ImageScaleToTotalPixelsX",
    ),
  ]);
  return untwisting && scaleImage;
}
