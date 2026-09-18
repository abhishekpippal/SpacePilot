"""Create backend/.env without echoing or logging secrets."""

from __future__ import annotations

import base64
import getpass
import secrets
from pathlib import Path
from urllib.parse import quote

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def main() -> None:
    password = getpass.getpass("Password for PostgreSQL role spacepilot_dev: ")
    if len(password) < 12:
        raise SystemExit("Use a password containing at least 12 characters.")
    private_key = Ed25519PrivateKey.generate().private_bytes(
        serialization.Encoding.Raw,
        serialization.PrivateFormat.Raw,
        serialization.NoEncryption(),
    )
    values = {
        "APP_ENV": "development",
        "DATABASE_URL": f"postgresql+psycopg://spacepilot_dev:{quote(password, safe='')}@127.0.0.1:5433/spacepilot_dev",
        "JWT_SIGNING_SECRET": secrets.token_urlsafe(48),
        "ENTITLEMENT_SIGNING_PRIVATE_KEY": base64.urlsafe_b64encode(
            private_key
        ).decode(),
        "OPENAI_API_KEY": "",
        "OPENAI_MODEL": "gpt-5-mini",
        "CORS_ALLOWED_ORIGINS": "",
        "ACCESS_TOKEN_MINUTES": "15",
        "REFRESH_TOKEN_DAYS": "30",
        "OFFLINE_GRACE_DAYS": "7",
        "AI_FREE_MONTHLY_LIMIT": "0",
        "AI_PRO_MONTHLY_LIMIT": "500",
        "MINIMUM_DESKTOP_VERSION": "0.6.0",
        "LATEST_DESKTOP_VERSION": "0.6.0",
    }
    target = Path(__file__).resolve().parents[1] / ".env"
    target.write_text(
        "".join(f"{key}={value}\n" for key, value in values.items()), encoding="utf-8"
    )
    print(f"Created {target} with local development secrets (values not displayed).")


if __name__ == "__main__":
    main()
