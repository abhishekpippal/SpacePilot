import base64
import pytest
from pydantic import ValidationError
from app.config import Settings


def staging(**overrides):
    values = {
        "app_env": "staging",
        "database_url": "postgresql+psycopg://service@db/staging",
        "jwt_signing_secret": "j" * 32,
        "entitlement_signing_private_key": base64.urlsafe_b64encode(bytes(32)).decode(),
        "billing_success_url": "https://staging.spacepilot.app/success",
        "billing_cancel_url": "https://staging.spacepilot.app/cancel",
        "password_reset_url": "https://staging.spacepilot.app/reset",
    }
    values.update(overrides)
    return Settings(**values)


def test_staging_requires_secure_core_configuration():
    assert staging().app_env == "staging"
    with pytest.raises(ValidationError):
        staging(database_url="sqlite:///unsafe.db")
    with pytest.raises(ValidationError):
        staging(jwt_signing_secret="weak")
    with pytest.raises(ValidationError):
        staging(billing_success_url="http://unsafe.example")


def test_staging_rejects_live_or_partial_payment_configuration():
    with pytest.raises(ValidationError):
        staging(stripe_secret_key="sk_live_forbidden")
    with pytest.raises(ValidationError):
        staging(
            stripe_secret_key="sk_test_safe",
            stripe_webhook_secret="whsec_test",
            stripe_price_pro_monthly="price_month",
            stripe_price_pro_annual="",
        )
    configured = staging(
        stripe_secret_key="sk_test_safe",
        stripe_webhook_secret="whsec_test",
        stripe_price_pro_monthly="price_month",
        stripe_price_pro_annual="price_year",
    )
    assert configured.stripe_secret_key.startswith("sk_test_")


def test_health_does_not_disclose_environment(client):
    payload = client.get("/health").json()
    assert payload == {"status": "ok", "version": "0.1.0"}
    assert "database" not in payload and "environment" not in payload
