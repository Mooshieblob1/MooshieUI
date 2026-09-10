"""Tensor contracts for pause nodes using the checked-out ComfyUI sampling rules.

No model weights are needed. Extracting the relevant classes avoids initializing
ComfyUI's server and device manager. Run with --comfyui-dir <ComfyUI checkout>.
"""
import argparse
import ast
from pathlib import Path
from types import SimpleNamespace
import unittest

import torch


def load_definitions(path, names, namespace):
    source = ast.parse(path.read_text(encoding="utf-8"))
    selected = [node for node in source.body if isinstance(node, (ast.ClassDef, ast.FunctionDef)) and node.name in names]
    assert {node.name for node in selected} == set(names), f"Missing classes in {path}"
    exec(compile(ast.Module(body=selected, type_ignores=[]), str(path), "exec"), namespace)
    return [namespace[name] for name in names]


def prepare_noise(latent, seed):
    return torch.randn(latent.shape, generator=torch.Generator().manual_seed(seed))


def calculate_sigmas(_sampling, scheduler, steps):
    # Deterministic schedules isolate the tail selection and interpolation.
    values = torch.linspace(1, 0, steps + 1)
    return values.square() if scheduler == "curved" else values


class PauseNodeTests(unittest.TestCase):
    def model(self, sampling):
        return SimpleNamespace(
            get_model_object=lambda name: sampling,
            model=SimpleNamespace(
                process_latent_in=lambda x: (x - 0.1) * 2,
                process_latent_out=lambda x: x / 2 + 0.1,
            ),
        )

    def test_edit_enters_resumed_sampler_at_correct_noise_level(self):
        devices = ["cpu"] + (["mps"] if torch.backends.mps.is_available() else [])
        for sampling_type in SAMPLING_TYPES:
            for device in devices:
                for opacity in (0.0, 0.25, 1.0):
                    with self.subTest(sampling=sampling_type.__name__, device=device, mask=opacity):
                        sampling = sampling_type()
                        model = self.model(sampling)
                        sigma = torch.tensor(0.6)
                        edited = {"samples": torch.full((1, 4, 8, 8), 0.4, device=device)}
                        original_noisy = torch.full((2, 4, 8, 8), -0.3)
                        paused = {
                            "samples": model.model.process_latent_out(
                                sampling.inverse_noise_scaling(sigma, original_noisy)
                            ).to(device),
                            "noise_mask": torch.ones(1, 8, 8),
                            "batch_index": [0, 1],
                        }
                        result, = ResumeEdit().blend(
                            model, torch.tensor([1.0, 0.6, 0.0]), 1, edited, paused, 42,
                            mask=torch.full((1, 16, 16), opacity, device=device),
                        )
                        # ComfyUI resumes a returned LATENT by processing it in
                        # and applying noise_scaling with zero added noise.
                        returned = model.model.process_latent_in(result["samples"])
                        resumed = sampling.noise_scaling(sigma, torch.zeros_like(returned), returned)
                        clean_edit = model.model.process_latent_in(edited["samples"].cpu()).expand(2, -1, -1, -1)
                        expected_edit = sampling.noise_scaling(sigma, prepare_noise(clean_edit, 42), clean_edit)
                        torch.testing.assert_close(resumed, expected_edit * opacity + original_noisy * (1 - opacity))
                        self.assertNotIn("noise_mask", result)
                        self.assertIn("noise_mask", paused, "input is not mutated")
                        self.assertEqual(result["batch_index"], [0, 1])

    def test_edit_without_mask_is_reproducible_and_resizes(self):
        model = self.model(SAMPLING_TYPES[0]())
        args = (model, torch.tensor([1.0, 0.5, 0.0]), 1,
                {"samples": torch.zeros(1, 4, 4, 4)}, {"samples": torch.ones(2, 4, 8, 8)})
        first, = ResumeEdit().blend(*args, seed=123)
        second, = ResumeEdit().blend(*args, seed=123)
        different, = ResumeEdit().blend(*args, seed=124)
        self.assertEqual(first["samples"].shape, (2, 4, 8, 8))
        torch.testing.assert_close(first["samples"], second["samples"])
        self.assertFalse(torch.equal(first["samples"], different["samples"]))

    def test_tail_preserves_boundary_and_reaches_zero(self):
        model = self.model(SAMPLING_TYPES[0]())
        for scheduler in ("linear", "curved"):
            for steps in (1, 7, 31):
                with self.subTest(scheduler=scheduler, steps=steps):
                    result, = SigmaTail().build(model, torch.tensor([1.0, 0.63, 0.0]), 1, scheduler, steps)
                    self.assertEqual(len(result), steps + 1)
                    self.assertAlmostEqual(result[0].item(), 0.63, places=6)
                    self.assertEqual(result[-1].item(), 0)
                    self.assertTrue(torch.all(result[:-1] >= result[1:]))
                    self.assertTrue(torch.isfinite(result).all())

    def test_tail_handles_exhausted_schedule(self):
        result, = SigmaTail().build(self.model(SAMPLING_TYPES[0]()), torch.tensor([1.0, 0.0]), 99, "linear", 5)
        self.assertTrue(torch.equal(result, torch.zeros(2)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--comfyui-dir", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    sampling_path = args.comfyui_dir / "comfy/model_sampling.py"
    sampling_names = ["EPS", "CONST"]
    if "def reshape_sigma(" in sampling_path.read_text(encoding="utf-8"):
        sampling_names.append("reshape_sigma")
    SAMPLING_TYPES = load_definitions(sampling_path, sampling_names, {"torch": torch})[:2]
    fake_comfy = SimpleNamespace(
        sample=SimpleNamespace(prepare_noise=prepare_noise),
        samplers=SimpleNamespace(calculate_sigmas=calculate_sigmas),
    )
    SigmaTail, ResumeEdit = load_definitions(
        root / "src-tauri/src/comfyui/mooshie_nodes.py", ["MooshieSigmaTail", "MooshieResumeEdit"],
        {"torch": torch, "comfy": fake_comfy},
    )
    unittest.main(argv=[__file__], verbosity=2)
