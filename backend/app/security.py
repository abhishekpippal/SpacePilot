import base64
import hashlib
import secrets
from datetime import datetime, timedelta, timezone
import jwt
from argon2 import PasswordHasher
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from .config import get_settings

ph = PasswordHasher()


def password_hash(value: str) -> str:
    return ph.hash(value)


def password_valid(encoded: str, value: str) -> bool:
    try:
        return ph.verify(encoded, value)
    except Exception:
        return False


def token_hash(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


def new_refresh() -> str:
    return secrets.token_urlsafe(48)


def access(user_id) -> str:
    s = get_settings()
    now = datetime.now(timezone.utc)
    return jwt.encode(
        {
            "sub": str(user_id),
            "type": "access",
            "iat": now,
            "exp": now + timedelta(minutes=s.access_token_minutes),
        },
        s.jwt_signing_secret,
        algorithm="HS256",
    )


def decode_access(value: str) -> str:
    return jwt.decode(
        value,
        get_settings().jwt_signing_secret,
        algorithms=["HS256"],
        options={"require": ["exp", "sub"]},
    )["sub"]


def sign_entitlement(payload: dict) -> str:
    raw = base64.urlsafe_b64decode(get_settings().entitlement_signing_private_key)
    return jwt.encode(
        payload, Ed25519PrivateKey.from_private_bytes(raw), algorithm="EdDSA"
    )


def entitlement_public_key() -> str:
    raw = base64.urlsafe_b64decode(get_settings().entitlement_signing_private_key)
    public = (
        Ed25519PrivateKey.from_private_bytes(raw)
        .public_key()
        .public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    )
    return base64.urlsafe_b64encode(public).decode()
