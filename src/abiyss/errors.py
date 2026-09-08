class AbiyssError(Exception):
    """Base exception for ABIYSS."""


class ValidationError(AbiyssError):
    """Input or contract validation failed."""


class PersistenceError(AbiyssError):
    """Durable state could not be read or written safely."""


class QueueInvariantError(AbiyssError):
    """A Query Queue invariant was violated."""


class SecurityError(AbiyssError):
    """A security policy rejected an operation."""


class ToolDenied(SecurityError):
    """A tool call was rejected by its local policy."""


class ProviderError(AbiyssError):
    """The model provider failed or returned an invalid response."""


class RecoveryRequired(AbiyssError):
    """A durable side-effect state requires explicit recovery."""
