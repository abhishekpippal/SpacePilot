from functools import lru_cache
from pathlib import Path
import base64
from urllib.parse import urlparse
from pydantic import model_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_file=Path(__file__).parents[1] / ".env", extra="ignore"
    )
    app_env: str = "development"
    database_url: str
    jwt_signing_secret: str
    entitlement_signing_private_key: str
    openai_api_key: str = ""
    openai_model: str = "gpt-5-mini"
    cors_allowed_origins: str = ""
    access_token_minutes: int = 15
    refresh_token_days: int = 30
    offline_grace_days: int = 7
    ai_free_monthly_limit: int = 0
    ai_pro_monthly_limit: int = 500
    minimum_desktop_version: str = "0.6.0"
    latest_desktop_version: str = "0.6.0"
    stripe_secret_key: str = ""
    stripe_webhook_secret: str = ""
    stripe_price_pro_monthly: str = ""
    stripe_price_pro_annual: str = ""
    billing_success_url: str = "https://spacepilot.app/billing/success"
    billing_cancel_url: str = "https://spacepilot.app/billing/cancel"
    resend_api_key: str = ""
    email_from: str = "SpacePilot <accounts@spacepilot.app>"
    password_reset_url: str = "https://spacepilot.app/reset-password"
    billing_grace_days: int = 3
    trusted_hosts: str = "localhost,127.0.0.1"
    forwarded_allow_ips: str = "127.0.0.1"

    @model_validator(mode="after")
    def validate_environment(self):
        if self.app_env not in {"development", "staging", "production"}:
            raise ValueError("APP_ENV must be development, staging, or production")
        if self.app_env in {"staging", "production"}:
            if not self.database_url.startswith(
                ("postgresql://", "postgresql+psycopg://")
            ):
                raise ValueError("A PostgreSQL DATABASE_URL is required")
            if len(self.jwt_signing_secret) < 32:
                raise ValueError("JWT signing material is missing or weak")
            try:
                if (
                    len(base64.urlsafe_b64decode(self.entitlement_signing_private_key))
                    != 32
                ):
                    raise ValueError
            except Exception as exc:
                raise ValueError("Invalid entitlement signing key") from exc
            for value in (
                self.billing_success_url,
                self.billing_cancel_url,
                self.password_reset_url,
            ):
                if urlparse(value).scheme != "https":
                    raise ValueError("External application URLs must use HTTPS")
        payment_values = [
            self.stripe_secret_key,
            self.stripe_webhook_secret,
            self.stripe_price_pro_monthly,
            self.stripe_price_pro_annual,
        ]
        if self.app_env == "staging" and any(payment_values):
            if not all(payment_values) or not self.stripe_secret_key.startswith(
                "sk_test_"
            ):
                raise ValueError(
                    "Staging billing requires a complete Stripe test-mode configuration"
                )
        return self

    def model_post_init(self, context):
        if self.app_env == "production" and (
            len(self.jwt_signing_secret) < 32
            or not self.openai_api_key
            or not self.stripe_secret_key
            or not self.stripe_webhook_secret
            or not self.resend_api_key
            or not self.stripe_price_pro_monthly
            or not self.stripe_price_pro_annual
            or not self.stripe_secret_key.startswith("sk_live_")
        ):
            raise ValueError("Critical production secrets are missing or weak")


@lru_cache
def get_settings() -> Settings:
    return Settings()
