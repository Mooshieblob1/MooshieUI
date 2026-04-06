"""
Compute latent_rgb_factors for NanoSaurLatentFormat.

Loads the full Nanosaur VAE (encoder + decoder), encodes a diverse set of
synthetic test images, then fits a least-squares regression from the
processed latent space to [0,1] RGB.

Usage:
    python scripts/compute_nanosaur_rgb_factors.py <path_to_nanosaur_vae_decoder.safetensors>

Outputs the latent_rgb_factors and latent_rgb_factors_bias in copy-paste
format for nodes.py.
"""
import sys
import os
import importlib.util

import torch
import numpy as np
from safetensors.torch import load_file

# Import vae.py directly to avoid __init__.py pulling in comfy
_vae_path = os.path.join(os.path.dirname(__file__), "..", "comfyui-nodes", "nanosaur_support", "vae.py")
_spec = importlib.util.spec_from_file_location("nanosaur_vae", _vae_path)
_vae_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_vae_mod)
NanoSaurVAE = _vae_mod.NanoSaurVAE

LATENT_CHANNELS = 96
SCALE_FACTOR = 2.3623
SHIFT_FACTOR = -0.0179
SPATIAL_DOWN = 16  # each latent pixel = 16x16 image pixels


def process_in(z_actual: torch.Tensor) -> torch.Tensor:
    """Convert actual latent → processed (sampler) space."""
    return (z_actual - SHIFT_FACTOR) / SCALE_FACTOR


def create_test_images(n_images: int, size: int, device, dtype) -> torch.Tensor:
    """Create diverse test images in [-1, 1] range for VAE encoding.
    Returns [N, 3, size, size]."""
    images = []

    # Solid color patches — sweep the RGB cube
    n_solid = min(216, n_images // 3)  # 6^3 = 216 colors
    steps = int(round(n_solid ** (1/3)))
    for r in np.linspace(-1, 1, steps):
        for g in np.linspace(-1, 1, steps):
            for b in np.linspace(-1, 1, steps):
                img = torch.full((1, 3, size, size), 0.0, device=device, dtype=dtype)
                img[0, 0] = r
                img[0, 1] = g
                img[0, 2] = b
                images.append(img)

    # Horizontal and vertical gradients in different color pairs
    for c1, c2 in [(0,1), (0,2), (1,2), (1,0), (2,0), (2,1)]:
        for n_grad in range(8):
            img = torch.zeros(1, 3, size, size, device=device, dtype=dtype)
            grad = torch.linspace(-1, 1, size, device=device, dtype=dtype)
            img[0, c1] = grad.unsqueeze(0).expand(size, size)
            img[0, c2] = grad.unsqueeze(1).expand(size, size) * (0.5 + 0.5 * n_grad / 8)
            images.append(img)

    # Random smooth patches (low-frequency noise → bilinear upsample)
    n_random = max(0, n_images - len(images))
    for _ in range(n_random):
        small = torch.randn(1, 3, 4, 4, device=device, dtype=dtype)
        img = torch.nn.functional.interpolate(small, size=(size, size), mode='bilinear', align_corners=False)
        img = img.clamp(-1, 1)
        images.append(img)

    return torch.cat(images[:n_images], dim=0)


def main():
    if len(sys.argv) < 2:
        print(f"Usage: python {sys.argv[0]} <path_to_nanosaur_vae_decoder.safetensors>")
        sys.exit(1)

    vae_path = sys.argv[1]
    if not os.path.isfile(vae_path):
        print(f"Error: File not found: {vae_path}")
        sys.exit(1)

    device = "cuda" if torch.cuda.is_available() else "cpu"
    dtype = torch.float32

    print(f"Loading full Nanosaur VAE from: {vae_path}")
    print(f"Device: {device}")

    # Build full VAE (encoder + decoder) and load weights
    vae = NanoSaurVAE(latent_dim=LATENT_CHANNELS)
    state_dict = load_file(vae_path)
    vae.load_state_dict(state_dict, strict=False)
    vae = vae.to(device, dtype).eval()

    print("VAE loaded (encoder + decoder).")

    # Generate diverse test images and encode them
    img_size = 256  # Must be divisible by 16 (patch size)
    n_images = 800
    batch_size = 8

    print(f"Creating {n_images} test images at {img_size}x{img_size}...")
    test_images = create_test_images(n_images, img_size, device, dtype)

    latent_h = img_size // SPATIAL_DOWN  # 16
    latent_w = img_size // SPATIAL_DOWN

    all_processed_latents = []
    all_rgb = []

    print("Encoding images and collecting latent-RGB pairs...")
    with torch.no_grad():
        for i in range(0, n_images, batch_size):
            batch = test_images[i:i+batch_size]
            b = batch.shape[0]

            # Encode: image [-1,1] → actual latent space
            z_actual = vae.encode(batch)  # [B, 96, H/16, W/16]

            # Convert to processed (sampler) space — this is what the preview sees
            z_proc = process_in(z_actual)  # [B, 96, latent_h, latent_w]

            # The target RGB: downsample original image to latent resolution, in [0, 1]
            # Convert [-1, 1] → [0, 1]
            batch_01 = (batch + 1.0) / 2.0
            rgb_down = torch.nn.functional.avg_pool2d(batch_01, kernel_size=SPATIAL_DOWN)
            # [B, 3, latent_h, latent_w]

            # Flatten spatial dims: [B*H*W, C]
            lat_flat = z_proc.permute(0, 2, 3, 1).reshape(-1, LATENT_CHANNELS)
            rgb_flat = rgb_down.permute(0, 2, 3, 1).reshape(-1, 3)

            all_processed_latents.append(lat_flat.cpu())
            all_rgb.append(rgb_flat.cpu())

            if (i + batch_size) % 100 == 0 or i + batch_size >= n_images:
                print(f"  {min(i + batch_size, n_images)}/{n_images}")

    # Stack all samples
    X = torch.cat(all_processed_latents, dim=0).numpy()  # [N, 96]
    Y = torch.cat(all_rgb, dim=0).numpy()                # [N, 3]

    print(f"\nFitting Ridge regression on {X.shape[0]} samples...")
    print(f"  Latent stats: mean={X.mean():.4f}, std={X.std():.4f}, "
          f"min={X.min():.4f}, max={X.max():.4f}")
    print(f"  RGB stats: mean={Y.mean():.4f}, std={Y.std():.4f}")

    # Ridge regression to avoid multicollinearity blowup.
    # The 96 latent channels are highly correlated (DINOv3 → linear projection),
    # so OLS produces huge coefficients with massive cancellation.
    # Ridge keeps factors stable and generalizable to diffusion latents.
    X_mean = X.mean(axis=0)
    Y_mean = Y.mean(axis=0)
    Xc = X - X_mean
    Yc = Y - Y_mean

    # Sweep alpha to find one that gives factors in the right magnitude range
    # (similar to other ComfyUI models: ~0.01-0.15 per factor)
    XtX = Xc.T @ Xc
    XtY = Xc.T @ Yc
    I = np.eye(LATENT_CHANNELS)

    best_alpha = None
    best_W = None
    target_max = 0.15  # target max factor magnitude (like SD3/Flux)

    for alpha in [1e0, 1e1, 1e2, 5e2, 1e3, 2e3, 5e3, 1e4, 2e4, 5e4, 1e5, 2e5, 5e5, 1e6]:
        W_test = np.linalg.solve(XtX + alpha * I, XtY)
        max_mag = np.abs(W_test).max()
        mean_mag = np.abs(W_test).mean()
        # Check R²
        b_test = Y_mean - X_mean @ W_test
        Y_pred = X @ W_test + b_test
        r2_test = 1 - np.sum((Y - Y_pred) ** 2) / np.sum((Y - Y.mean(axis=0)) ** 2)
        print(f"  alpha={alpha:.0e}: max_factor={max_mag:.4f}, mean_factor={mean_mag:.4f}, R²={r2_test:.4f}")
        if max_mag <= target_max and (best_alpha is None or alpha < best_alpha):
            best_alpha = alpha
            best_W = W_test

    if best_W is None:
        # Use the largest alpha tested
        best_alpha = 1e6
        best_W = np.linalg.solve(XtX + best_alpha * I, XtY)
        print(f"  Using alpha={best_alpha:.0e} (max magnitude not reached)")

    W = best_W
    bias = Y_mean - X_mean @ W

    # Quality metrics
    Y_pred = X @ W + bias
    rmse = np.sqrt(np.mean((Y - Y_pred) ** 2, axis=0))
    r2 = 1 - np.sum((Y - Y_pred) ** 2, axis=0) / np.sum((Y - Y.mean(axis=0)) ** 2, axis=0)
    print(f"\nSelected alpha={best_alpha:.0e}")
    print(f"RMSE per channel (R,G,B): [{rmse[0]:.4f}, {rmse[1]:.4f}, {rmse[2]:.4f}]")
    print(f"R² per channel (R,G,B): [{r2[0]:.4f}, {r2[1]:.4f}, {r2[2]:.4f}]")
    print(f"Factor magnitude: max={np.abs(W).max():.6f}, mean={np.abs(W).mean():.6f}")
    print(f"Bias: [{bias[0]:.4f}, {bias[1]:.4f}, {bias[2]:.4f}]")

    # Format output
    print("\n" + "=" * 70)
    print("COPY-PASTE INTO NanoSaurLatentFormat.__init__():")
    print("=" * 70)
    print("        self.latent_rgb_factors = [")
    for i in range(LATENT_CHANNELS):
        r, g, b = W[i]
        comma = "," if i < LATENT_CHANNELS - 1 else ""
        print(f"            [{r:9.4f}, {g:9.4f}, {b:9.4f}]{comma}")
    print("        ]")
    br, bg, bb = bias
    print(f"        self.latent_rgb_factors_bias = [{br:.4f}, {bg:.4f}, {bb:.4f}]")
    print("=" * 70)


if __name__ == "__main__":
    main()
