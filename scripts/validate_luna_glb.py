#!/usr/bin/env python3
"""Validate the distributed Luna GLB contract used by LegacyGlbAdapter.

The validator is intentionally dependency-free. It checks the GLB container,
requires unique Idle/Wave clips, reports duration/channel counts, and warns
when the Blender export is materially heavier than the historical M0-B1 asset.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

GLB_MAGIC = 0x46546C67
JSON_CHUNK = 0x4E4F534A
BIN_CHUNK = 0x004E4942

HISTORICAL_M0B1_SIZE = 2_490_812
HISTORICAL_M0B1_CHANNELS_PER_CLIP = 17
REQUIRED_CLIPS = ("Idle", "Wave")


class ValidationError(RuntimeError):
    pass


def parse_glb(path: Path) -> tuple[dict, bytes, int, str]:
    data = path.read_bytes()
    if len(data) < 12:
        raise ValidationError("arquivo curto demais para ser um GLB")

    magic, version, declared_length = struct.unpack_from("<III", data, 0)
    if magic != GLB_MAGIC:
        raise ValidationError("magic GLB inválido")
    if version != 2:
        raise ValidationError(f"versão GLB não suportada: {version}")
    if declared_length != len(data):
        raise ValidationError(
            f"tamanho declarado {declared_length} difere do tamanho real {len(data)}"
        )

    offset = 12
    document = None
    binary = b""
    while offset < len(data):
        if offset + 8 > len(data):
            raise ValidationError("cabeçalho de chunk truncado")
        chunk_length, chunk_type = struct.unpack_from("<II", data, offset)
        offset += 8
        end = offset + chunk_length
        if end > len(data):
            raise ValidationError("chunk ultrapassa o fim do arquivo")
        payload = data[offset:end]
        offset = end

        if chunk_type == JSON_CHUNK:
            document = json.loads(payload.decode("utf-8").rstrip(" \t\r\n\0"))
        elif chunk_type == BIN_CHUNK:
            binary = payload

    if document is None:
        raise ValidationError("chunk JSON ausente")

    return document, binary, len(data), hashlib.sha256(data).hexdigest()


def accessor_max_time(document: dict, binary: bytes, accessor_index: int) -> float:
    accessors = document.get("accessors", [])
    buffer_views = document.get("bufferViews", [])
    try:
        accessor = accessors[accessor_index]
    except IndexError as exc:
        raise ValidationError(f"accessor inexistente: {accessor_index}") from exc

    if accessor.get("type") != "SCALAR" or accessor.get("componentType") != 5126:
        raise ValidationError(
            f"accessor de tempo {accessor_index} não é SCALAR/FLOAT"
        )

    max_values = accessor.get("max")
    if max_values:
        return float(max_values[0])

    if "bufferView" not in accessor:
        raise ValidationError(f"accessor de tempo {accessor_index} sem bufferView")
    try:
        view = buffer_views[accessor["bufferView"]]
    except IndexError as exc:
        raise ValidationError(
            f"bufferView inexistente no accessor {accessor_index}"
        ) from exc

    count = int(accessor.get("count", 0))
    if count <= 0:
        raise ValidationError(f"accessor de tempo {accessor_index} vazio")

    start = int(view.get("byteOffset", 0)) + int(accessor.get("byteOffset", 0))
    stride = int(view.get("byteStride", 4))
    last = start + (count - 1) * stride
    if last + 4 > len(binary):
        raise ValidationError(f"accessor de tempo {accessor_index} fora do BIN")

    maximum = float("-inf")
    for i in range(count):
        (value,) = struct.unpack_from("<f", binary, start + i * stride)
        maximum = max(maximum, value)
    return maximum


def clip_metrics(document: dict, binary: bytes) -> list[dict]:
    metrics = []
    for animation in document.get("animations", []):
        channels = animation.get("channels", [])
        samplers = animation.get("samplers", [])
        maximum = 0.0
        for sampler in samplers:
            if "input" not in sampler:
                raise ValidationError(
                    f"sampler sem input na animação {animation.get('name')!r}"
                )
            maximum = max(
                maximum,
                accessor_max_time(document, binary, int(sampler["input"])),
            )
        metrics.append(
            {
                "name": animation.get("name") or "",
                "duration": maximum,
                "channels": len(channels),
                "samplers": len(samplers),
            }
        )
    return metrics


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "path",
        nargs="?",
        default="public/models/Luna.glb",
        help="GLB a validar (default: public/models/Luna.glb)",
    )
    args = parser.parse_args()
    path = Path(args.path)

    try:
        document, binary, size, sha256 = parse_glb(path)
        metrics = clip_metrics(document, binary)
    except (OSError, ValueError, json.JSONDecodeError, struct.error, ValidationError) as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return 1

    names = [item["name"] for item in metrics]
    duplicates = sorted({name for name in names if names.count(name) > 1 and name})
    missing = [name for name in REQUIRED_CLIPS if names.count(name) != 1]

    print(f"{path}: {size} bytes; SHA256 {sha256}")
    for item in metrics:
        print(
            f"- {item['name'] or '<sem nome>'}: "
            f"{item['duration']:.3f}s; "
            f"{item['channels']} canais; {item['samplers']} samplers"
        )

    failures = []
    if duplicates:
        failures.append("nomes de clipe duplicados: " + ", ".join(duplicates))
    if missing:
        failures.append(
            "Idle/Wave devem existir exatamente uma vez; problema em: "
            + ", ".join(missing)
        )
    for item in metrics:
        if not item["channels"] or not item["samplers"]:
            failures.append(f"clipe vazio: {item['name'] or '<sem nome>'}")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        return 1

    ratio = size / HISTORICAL_M0B1_SIZE
    if ratio > 1.25:
        print(
            "WARN: asset está "
            f"{ratio:.2f}x maior que o M0-B1 histórico "
            f"({HISTORICAL_M0B1_SIZE} bytes)."
        )
    for item in metrics:
        if item["channels"] > HISTORICAL_M0B1_CHANNELS_PER_CLIP * 4:
            print(
                "WARN: "
                f"{item['name']} usa {item['channels']} canais; "
                f"M0-B1 usava {HISTORICAL_M0B1_CHANNELS_PER_CLIP}. "
                "Revalidar performance antes de atribuir regressões ao runtime/UI."
            )

    print("PASS: contrato legado Idle/Wave válido.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
