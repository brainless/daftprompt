"""Checkpoint verification and model loading for the Epic 014 harness.

Model-facing layer: everything here imports gliner2/torch lazily so the pure
modules and tests never load (or even import) a checkpoint. Design points:

- The checkpoint's `config.json` is resolved and inspected **before** any
  weight download. A checkpoint that is not a GLiNER2.5 boundary model, or
  whose relation head is disabled (`boundary_head.enable_relations` false —
  including configs older than config_version 3, which migrate to disabled),
  raises `CheckpointCapabilityError` with an explicit message. Failures are
  never downgraded to empty results.
- Two variants share one input/scoring contract:
  `jointie` (default; typed endpoints via `gliner2.joint_ie.JointIE` +
  the schema's JointIE mapping) and `independent` (untyped
  `extract_relations` + entity extraction in one schema; endpoint kinds are
  attached post-hoc by span match and pairs are enforced by the validator).
- Device is resolved explicitly (cuda -> mps -> cpu) and recorded.
- HF revision is probed best-effort for reproducibility; a network failure is
  recorded as a note, never as a silent unknown.
"""
from __future__ import annotations

import dataclasses
import hashlib
from pathlib import Path
from typing import Any, Optional

from schema import core

KNOWN_MODEL_IDS = (
    "fastino/gliner2.5-small-v1",
    "fastino/gliner2.5-base-v1",
    "fastino/gliner2.5-multi-v1",
)

VARIANTS = ("jointie", "independent")


class CheckpointCapabilityError(RuntimeError):
    """The selected checkpoint cannot run this experiment's extraction."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(65536), b""):
            digest.update(block)
    return digest.hexdigest()


def resolve_device(requested: str = "auto") -> str:
    import torch

    if requested != "auto":
        return requested
    if torch.cuda.is_available():
        return "cuda"
    if getattr(torch.backends, "mps", None) is not None and torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def device_info(requested: str, resolved: str) -> dict:
    info: dict[str, Any] = {"requested": requested, "resolved": resolved}
    try:
        import torch

        info["cuda_available"] = bool(torch.cuda.is_available())
        info["mps_available"] = bool(
            getattr(torch.backends, "mps", None) is not None and torch.backends.mps.is_available()
        )
        if resolved == "cuda" and torch.cuda.is_available():
            info["cuda_device_name"] = torch.cuda.get_device_name(0)
    except Exception as exc:  # device introspection must not block a run
        info["introspection_error"] = repr(exc)
    return info


def hub_kwargs(cache_dir: Optional[Path] = None, revision: Optional[str] = None,
               local_files_only: bool = False) -> dict[str, Any]:
    kwargs: dict[str, Any] = {}
    if cache_dir is not None:
        kwargs["cache_dir"] = str(cache_dir)
    if revision:
        kwargs["revision"] = revision
    if local_files_only:
        kwargs["local_files_only"] = True
    return kwargs


def apply_hub_env(options: dict[str, Any]) -> None:
    """Make --cache-dir/--local-files-only authoritative for every hub fetch.

    Model-neutral workaround for gliner2 2.0.0: `AutoExtractor.from_pretrained`
    consumes hub options only for the config fetch and delegates to the
    architecture loader with `**model_kwargs` alone (gliner2/auto.py), so
    `cache_dir`/`local_files_only`/`revision` never reach the tokenizer or
    weight downloads (`load_extractor_tokenizer` gets no options at all).
    Setting HF_HUB_CACHE / HF_HUB_OFFLINE before any hub access redirects
    every fetch and keeps the documented CLI contract true for both variants
    and any checkpoint. huggingface_hub reads these at import time, so call
    this before the first model load; if it is already imported its constants
    are patched as a fallback. Note: `revision` is still not honored by the
    gliner2 weight/tokenizer path (it resolves refs/main) — the manifest
    records the resolved sha instead.
    """
    import os

    cache_dir = options.get("cache_dir")
    if cache_dir is not None:
        resolved = Path(cache_dir).expanduser().resolve()
        resolved.mkdir(parents=True, exist_ok=True)
        os.environ["HF_HUB_CACHE"] = str(resolved)
        try:
            import huggingface_hub.constants as hf_constants

            hf_constants.HF_HUB_CACHE = resolved
        except Exception:  # hub not importable yet is the normal case
            pass
    if options.get("local_files_only"):
        os.environ["HF_HUB_OFFLINE"] = "1"
        try:
            import huggingface_hub.constants as hf_constants

            hf_constants.HF_HUB_OFFLINE = True
        except Exception:
            pass


def load_checkpoint_config(model_or_path: str, options: Optional[dict] = None):
    """Resolve and parse `config.json` without downloading model weights."""
    from gliner2.configuration import ExtractorConfig
    from gliner2.models.loading import checkpoint_file

    options = options or {}
    path = Path(model_or_path).expanduser()
    if path.is_dir():
        config_path = path / "config.json"
        if not config_path.is_file():
            raise FileNotFoundError(f"no config.json in local checkpoint directory {path}")
    else:
        if path.is_absolute() or str(model_or_path).startswith(("./", "../", "~/")):
            raise FileNotFoundError(
                f"local checkpoint path {model_or_path!r} does not exist"
            )
        config_path = Path(checkpoint_file(str(model_or_path), "config.json", dict(options)))
    return ExtractorConfig.from_pretrained(str(config_path)), config_path


def config_capability(config: Any) -> tuple[bool, str]:
    """Whether the parsed config can run JointIE / independent relations."""
    from gliner2.configuration import architecture_from_config

    architecture = architecture_from_config(config)
    if architecture != "boundary":
        return False, (
            f"checkpoint architecture is {architecture!r}; this experiment requires a "
            "GLiNER2.5 boundary checkpoint (the legacy span architecture has no typed "
            "relation head for JointIE)"
        )
    boundary_head = getattr(config, "boundary_head", None) or {}
    if not bool(boundary_head.get("enable_relations", False)):
        return False, (
            "boundary checkpoint does not enable the relation head "
            "(boundary_head.enable_relations is false; configs older than "
            "config_version 3 migrate to disabled) — JointIE and "
            "extract_relations cannot run on this checkpoint"
        )
    return True, "boundary architecture with relation head enabled"


def verify_checkpoint(model_or_path: str, options: Optional[dict] = None) -> dict:
    """Gate on the relation head before any weight download; fail visibly."""
    config, config_path = load_checkpoint_config(model_or_path, options)
    ok, reason = config_capability(config)
    if not ok:
        raise CheckpointCapabilityError(f"checkpoint {model_or_path!r}: {reason}")
    return {
        "architecture": getattr(config, "architecture", None),
        "enable_relations": True,
        "model_name": getattr(config, "model_name", None),
        "config_path": str(config_path),
        "config_sha256": sha256_file(config_path),
    }


def probe_revision(model_id: str, revision: Optional[str] = None,
                   timeout: float = 10.0) -> tuple[Optional[str], Optional[str]]:
    """Best-effort HF commit sha; returns (sha_or_None, note_or_None)."""
    try:
        from huggingface_hub import HfApi

        info = HfApi().model_info(model_id, revision=revision, timeout=timeout)
        return info.sha, None
    except Exception as exc:
        return None, f"revision probe failed: {type(exc).__name__}: {exc}"


class JointIEAdapter:
    """`gliner2.joint_ie.JointIE` with the schema's derived JointIE mapping."""

    variant = "jointie"

    def __init__(self, model_id: str, device: str, options: dict[str, Any],
                 config_kwargs: dict[str, Any]):
        from gliner2.joint_ie import JointIE, JointIEConfig

        self.config = JointIEConfig(**config_kwargs)
        # Rejected alternative: JointIE.extract_long (gliner2's native chunker
        # splits mid-line and hides per-chunk records; Epic 014 requires
        # recorded whole-line chunks with explicit boundaries/overlaps).
        self.joint = JointIE.from_pretrained(model_id, map_location=device, **options)
        self.joint_schema: Any = None
        self.label_map: dict[str, str] = {}

    def bind_schema(self, schema: core.ExperimentSchema) -> None:
        self.joint_schema = core.to_joint_schema(schema)
        self.label_map = {
            spec.label: spec.canonical_predicate
            for spec in schema.joint_ie_labels().values()
        }

    def extract_chunk(self, text: str) -> dict:
        result = self.joint.extract(text, self.joint_schema, config=self.config)
        raw = result.to_dict(include_confidence=True, include_spans=True)
        raw["feasible"] = bool(result.feasible)
        return raw

    def threshold_record(self) -> dict:
        return dataclasses.asdict(self.config)


class IndependentAdapter:
    """Untyped `extract_relations` + entity extraction in one schema.

    Same input and scoring contract as JointIE: identical snapshot, chunking,
    schema descriptions, and post-hoc pair validation. Endpoint typing is
    absent from the model, so kinds are attached by span match during
    normalization and enforced by `core.validate_claim`.
    """

    variant = "independent"

    def __init__(self, model_id: str, device: str, options: dict[str, Any],
                 threshold: float = 0.5):
        from gliner2 import AutoExtractor

        self.threshold = threshold
        self.model = AutoExtractor.from_pretrained(model_id, map_location=device, **options)
        self.schema: Any = None

    def bind_schema(self, schema: core.ExperimentSchema) -> None:
        builder = self.model.create_schema()
        builder.entities({kind.name: kind.description for kind in schema.entity_kinds.values()})
        builder.relations(
            {
                label: {"description": schema.predicates[label].description}
                for label in schema.independent_labels().values()
            }
        )
        self.schema = builder

    def extract_chunk(self, text: str) -> dict:
        return self.model.extract(
            text,
            self.schema,
            threshold=self.threshold,
            include_confidence=True,
            include_spans=True,
        )

    def threshold_record(self) -> dict:
        return {"threshold": self.threshold}


def load_engine(variant: str, model_id: str, device: str, options: dict[str, Any],
                jointie_config_kwargs: Optional[dict] = None,
                independent_threshold: float = 0.5):
    if variant == "jointie":
        return JointIEAdapter(model_id, device, options, jointie_config_kwargs or {})
    if variant == "independent":
        return IndependentAdapter(model_id, device, options, independent_threshold)
    raise ValueError(f"unknown variant {variant!r}; expected one of {VARIANTS}")
