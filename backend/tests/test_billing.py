from datetime import datetime, timedelta, timezone
from sqlalchemy import select
from app import billing, password_reset
from app.models import Entitlement, PaymentEvent, SubscriptionRecord, User


class FakePayments:
    def create_checkout_session(self, **kwargs):
        return {"url": "https://checkout.stripe.test/session"}

    def create_billing_portal_session(self, *args):
        return {"url": "https://billing.stripe.test/portal"}

    def verify_webhook(self, body, signature):
        if signature != "valid":
            raise ValueError("invalid")
        import json

        return json.loads(body)


def account(client, email="billing@example.com"):
    result = client.post(
        "/v1/auth/signup",
        json={"email": email, "password": "correct horse battery staple"},
    ).json()
    return {"Authorization": f"Bearer {result['access_token']}"}


def test_checkout_rejects_client_price_and_invalid_plan(client, monkeypatch):
    headers = account(client)
    monkeypatch.setattr(billing, "provider", FakePayments())
    monkeypatch.setattr(
        billing,
        "paid_plans",
        lambda: {
            "PRO_MONTHLY": type(
                "P", (), {"price_id": "price_server", "code": "PRO_MONTHLY"}
            )()
        },
    )
    assert (
        client.post(
            "/v1/billing/checkout", headers=headers, json={"plan": "HACK", "price": "1"}
        ).status_code
        == 400
    )
    response = client.post(
        "/v1/billing/checkout",
        headers=headers,
        json={"plan": "PRO_MONTHLY", "price": "attacker"},
    )
    assert response.json()["url"].startswith("https://checkout.stripe.test")


def test_webhook_signature_and_idempotency(client, db, monkeypatch):
    monkeypatch.setattr(billing, "provider", FakePayments())
    u = User(email="p@example.com", normalized_email="p@example.com", password_hash="x")
    db.add(u)
    db.flush()
    db.add(
        Entitlement(
            user_id=u.id,
            plan="FREE",
            status="ACTIVE",
            features={},
            device_limit=1,
            ai_monthly_limit=0,
        )
    )
    db.commit()
    event = {
        "id": "evt_once",
        "type": "customer.subscription.created",
        "data": {
            "object": {
                "id": "sub_1",
                "customer": "cus_1",
                "status": "active",
                "current_period_end": int(
                    (datetime.now(timezone.utc) + timedelta(days=30)).timestamp()
                ),
                "metadata": {
                    "spacepilot_user_id": str(u.id),
                    "spacepilot_plan": "PRO_MONTHLY",
                },
            }
        },
    }
    assert (
        client.post(
            "/v1/billing/webhook",
            content=__import__("json").dumps(event),
            headers={"stripe-signature": "bad"},
        ).status_code
        == 400
    )
    assert (
        client.post(
            "/v1/billing/webhook",
            content=__import__("json").dumps(event),
            headers={"stripe-signature": "valid"},
        ).json()["status"]
        == "processed"
    )
    assert (
        client.post(
            "/v1/billing/webhook",
            content=__import__("json").dumps(event),
            headers={"stripe-signature": "valid"},
        ).json()["status"]
        == "duplicate"
    )
    assert (
        db.scalar(select(Entitlement).where(Entitlement.user_id == u.id)).plan == "PRO"
    )
    assert len(db.scalars(select(PaymentEvent)).all()) == 1


def test_reconciliation_cancellation_expiry_and_grace(db):
    from app.entitlement_service import reconcile_entitlement

    u = User(email="r@example.com", normalized_email="r@example.com", password_hash="x")
    db.add(u)
    db.flush()
    e = Entitlement(
        user_id=u.id,
        plan="FREE",
        status="ACTIVE",
        features={},
        device_limit=1,
        ai_monthly_limit=0,
    )
    db.add(e)
    s = SubscriptionRecord(
        user_id=u.id,
        provider="stripe",
        external_id="sub_r",
        internal_plan="PRO_MONTHLY",
        status="active",
        cancel_at_period_end=True,
        current_period_end=datetime.now(timezone.utc) + timedelta(days=2),
    )
    db.add(s)
    db.commit()
    assert reconcile_entitlement(db, u.id).plan == "PRO"
    s.status = "canceled"
    s.current_period_end = datetime.now(timezone.utc) - timedelta(days=1)
    db.flush()
    assert reconcile_entitlement(db, u.id).plan == "FREE"
    s.status = "past_due"
    s.current_period_end = datetime.now(timezone.utc) - timedelta(days=1)
    db.flush()
    assert reconcile_entitlement(db, u.id).status == "BILLING_GRACE"


def test_password_reset_is_single_use_and_revokes_sessions(client, db, monkeypatch):
    monkeypatch.setattr(
        password_reset,
        "token_urlsafe",
        lambda n: "known-reset-token-that-is-long-enough",
    )
    account(client, "reset@example.com")
    assert client.post(
        "/v1/auth/password-reset/request", json={"email": "reset@example.com"}
    ).json() == {"status": "accepted"}
    body = {
        "token": "known-reset-token-that-is-long-enough",
        "password": "a completely new password",
    }
    assert client.post("/v1/auth/password-reset/confirm", json=body).status_code == 200
    assert client.post("/v1/auth/password-reset/confirm", json=body).status_code == 400
