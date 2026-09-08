from __future__ import annotations

import math
from typing import Any

from .errors import ValidationError

_MAX_SCHEMA_DEPTH = 16
_MAX_OBJECT_PROPERTIES = 128
_MAX_ARRAY_ITEMS = 512
_MAX_STRING_BYTES = 256 * 1024


def _type_matches(value: Any, type_name: str) -> bool:
    return {
        "object": isinstance(value, dict),
        "array": isinstance(value, list),
        "string": isinstance(value, str),
        "integer": isinstance(value, int) and not isinstance(value, bool),
        "number": isinstance(value, (int, float)) and not isinstance(value, bool),
        "boolean": isinstance(value, bool),
        "null": value is None,
    }.get(type_name, True)


def validate(value: Any, schema: dict[str, Any], *, path: str = "$", depth: int = 0) -> None:
    """Validate the useful JSON-Schema subset used by ABIYSS tool contracts."""
    if depth > _MAX_SCHEMA_DEPTH or not isinstance(schema, dict):
        raise ValidationError(f"invalid schema at {path}")

    type_spec = schema.get("type")
    if isinstance(type_spec, list):
        errors: list[str] = []
        for candidate in type_spec:
            try:
                validate(value, {**schema, "type": candidate}, path=path, depth=depth + 1)
                return
            except ValidationError as exc:
                errors.append(str(exc))
        raise ValidationError(f"type mismatch at {path}: {'; '.join(errors[:3])}")

    if type_spec and not _type_matches(value, type_spec):
        raise ValidationError(f"expected {type_spec} at {path}")
    if isinstance(value, float) and not math.isfinite(value):
        raise ValidationError(f"non-finite number at {path}")

    if isinstance(value, dict):
        properties = schema.get("properties", {})
        if not isinstance(properties, dict) or len(properties) > _MAX_OBJECT_PROPERTIES:
            raise ValidationError(f"invalid object schema at {path}")
        required = schema.get("required", [])
        if not isinstance(required, list) or any(not isinstance(item, str) for item in required):
            raise ValidationError(f"invalid required list at {path}")
        for key in required:
            if key not in value:
                raise ValidationError(f"missing {path}.{key}")
        if schema.get("additionalProperties") is False:
            extras = sorted(set(value) - set(properties))
            if extras:
                raise ValidationError(f"unexpected properties at {path}: {extras[:8]}")
        for key, item in value.items():
            if not isinstance(key, str):
                raise ValidationError(f"non-string object key at {path}")
            if key in properties:
                validate(item, properties[key], path=f"{path}.{key}", depth=depth + 1)
        return

    if isinstance(value, list):
        minimum = int(schema.get("minItems", 0))
        maximum = min(int(schema.get("maxItems", _MAX_ARRAY_ITEMS)), _MAX_ARRAY_ITEMS)
        if len(value) < minimum or len(value) > maximum:
            raise ValidationError(f"array bounds at {path}")
        item_schema = schema.get("items")
        if item_schema is not None:
            for index, item in enumerate(value):
                validate(item, item_schema, path=f"{path}[{index}]", depth=depth + 1)
        return

    if isinstance(value, str):
        if len(value.encode("utf-8")) > min(int(schema.get("maxLength", _MAX_STRING_BYTES)), _MAX_STRING_BYTES):
            raise ValidationError(f"string too long at {path}")
        if "enum" in schema and value not in schema["enum"]:
            raise ValidationError(f"enum violation at {path}")
        return

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            raise ValidationError(f"minimum violation at {path}")
        if "maximum" in schema and value > schema["maximum"]:
            raise ValidationError(f"maximum violation at {path}")
