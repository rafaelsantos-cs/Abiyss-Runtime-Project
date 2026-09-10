from __future__ import annotations

import math
from typing import Any

from .errors import ValidationError

_MAX_SCHEMA_DEPTH = 16
_MAX_OBJECT_PROPERTIES = 128
_MAX_ARRAY_ITEMS = 512
_MAX_STRING_BYTES = 256 * 1024
_VALID_TYPES = frozenset({"object", "array", "string", "integer", "number", "boolean", "null"})


def _type_matches(value: Any, type_name: str) -> bool:
    return {
        "object": isinstance(value, dict),
        "array": isinstance(value, list),
        "string": isinstance(value, str),
        "integer": isinstance(value, int) and not isinstance(value, bool),
        "number": isinstance(value, (int, float)) and not isinstance(value, bool),
        "boolean": isinstance(value, bool),
        "null": value is None,
    }.get(type_name, False)


def validate(value: Any, schema: dict[str, Any], *, path: str = "$", depth: int = 0) -> None:
    """Validate the bounded JSON-Schema subset used by ABIYSS tool contracts.

    Schema vocabulary is fail-closed: unknown or malformed schema types are
    rejected instead of being treated as permissive wildcards.
    """
    if depth > _MAX_SCHEMA_DEPTH or not isinstance(schema, dict):
        raise ValidationError(f"invalid schema at {path}")

    type_spec = schema.get("type")
    if isinstance(type_spec, list):
        if not type_spec or any(not isinstance(candidate, str) or candidate not in _VALID_TYPES for candidate in type_spec):
            raise ValidationError(f"invalid type declaration at {path}")
        errors: list[str] = []
        for candidate in dict.fromkeys(type_spec):
            try:
                validate(value, {**schema, "type": candidate}, path=path, depth=depth + 1)
                return
            except ValidationError as exc:
                errors.append(str(exc))
        raise ValidationError(f"type mismatch at {path}: {'; '.join(errors[:3])}")

    if type_spec is not None:
        if not isinstance(type_spec, str) or type_spec not in _VALID_TYPES:
            raise ValidationError(f"invalid type declaration at {path}")
        if not _type_matches(value, type_spec):
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
        if len(required) > _MAX_OBJECT_PROPERTIES:
            raise ValidationError(f"required list too large at {path}")
        if any(item not in properties for item in required):
            raise ValidationError(f"required property missing from schema at {path}")
        for key, subschema in properties.items():
            if not isinstance(key, str) or len(key.encode("utf-8")) > 256 or not isinstance(subschema, dict):
                raise ValidationError(f"invalid property schema at {path}")
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
        try:
            minimum = int(schema.get("minItems", 0))
            maximum = min(int(schema.get("maxItems", _MAX_ARRAY_ITEMS)), _MAX_ARRAY_ITEMS)
        except (TypeError, ValueError) as exc:
            raise ValidationError(f"invalid array bounds at {path}") from exc
        if minimum < 0 or maximum < minimum:
            raise ValidationError(f"invalid array bounds at {path}")
        if len(value) < minimum or len(value) > maximum:
            raise ValidationError(f"array bounds at {path}")
        item_schema = schema.get("items")
        if item_schema is not None and not isinstance(item_schema, dict):
            raise ValidationError(f"invalid item schema at {path}")
        if item_schema is not None:
            for index, item in enumerate(value):
                validate(item, item_schema, path=f"{path}[{index}]", depth=depth + 1)
        return

    if isinstance(value, str):
        try:
            maximum = min(int(schema.get("maxLength", _MAX_STRING_BYTES)), _MAX_STRING_BYTES)
        except (TypeError, ValueError) as exc:
            raise ValidationError(f"invalid string bound at {path}") from exc
        if maximum < 0 or len(value.encode("utf-8")) > maximum:
            raise ValidationError(f"string too long at {path}")
        enum = schema.get("enum")
        if enum is not None and (not isinstance(enum, list) or len(enum) > _MAX_ARRAY_ITEMS or value not in enum):
            raise ValidationError(f"enum violation at {path}")
        return

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        minimum = schema.get("minimum")
        maximum = schema.get("maximum")
        if minimum is not None:
            if isinstance(minimum, bool) or not isinstance(minimum, (int, float)) or not math.isfinite(float(minimum)):
                raise ValidationError(f"invalid minimum at {path}")
            if value < minimum:
                raise ValidationError(f"minimum violation at {path}")
        if maximum is not None:
            if isinstance(maximum, bool) or not isinstance(maximum, (int, float)) or not math.isfinite(float(maximum)):
                raise ValidationError(f"invalid maximum at {path}")
            if value > maximum:
                raise ValidationError(f"maximum violation at {path}")
