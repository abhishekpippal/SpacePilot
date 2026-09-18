from abc import ABC, abstractmethod
import hashlib
import hmac
import json
import time
import httpx
from .config import get_settings


class PaymentError(Exception):
    pass


class PaymentProvider(ABC):
    @abstractmethod
    def create_checkout_session(self, **kwargs): ...
    @abstractmethod
    def create_billing_portal_session(self, customer_id, return_url): ...
    @abstractmethod
    def verify_webhook(self, body, signature): ...


class StripeProvider(PaymentProvider):
    def _post(self, path, data):
        s = get_settings()
        try:
            r = httpx.post(
                f"https://api.stripe.com/v1/{path}",
                auth=(s.stripe_secret_key, ""),
                data=data,
                timeout=20,
            )
            r.raise_for_status()
            return r.json()
        except httpx.HTTPError as exc:
            raise PaymentError("payment provider unavailable") from exc

    def create_checkout_session(self, **k):
        data = {
            "mode": "subscription",
            "line_items[0][price]": k["price_id"],
            "line_items[0][quantity]": 1,
            "client_reference_id": k["user_id"],
            "customer_email": k["email"],
            "success_url": k["success_url"],
            "cancel_url": k["cancel_url"],
            "subscription_data[metadata][spacepilot_user_id]": k["user_id"],
            "subscription_data[metadata][spacepilot_plan]": k["plan"],
        }
        return self._post("checkout/sessions", data)

    def create_billing_portal_session(self, customer_id, return_url):
        return self._post(
            "billing_portal/sessions",
            {"customer": customer_id, "return_url": return_url},
        )

    def verify_webhook(self, body, signature):
        fields = dict(x.split("=", 1) for x in signature.split(",") if "=" in x)
        timestamp = int(fields.get("t", "0"))
        secret = get_settings().stripe_webhook_secret
        if not secret or abs(time.time() - timestamp) > 300:
            raise PaymentError("invalid webhook")
        expected = hmac.new(
            secret.encode(), f"{timestamp}.".encode() + body, hashlib.sha256
        ).hexdigest()
        if not hmac.compare_digest(expected, fields.get("v1", "")):
            raise PaymentError("invalid webhook")
        return json.loads(body)
