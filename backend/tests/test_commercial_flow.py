import jwt
import base64
import uuid
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
from sqlalchemy import func, select

from app import ai_gateway
from app.ai_provider import ProviderResult
from app.capabilities import for_plan
from app.models import AiUsage, Entitlement
from app.security import entitlement_public_key


class MockProvider:
    def generate(self, question, context):
        return ProviderResult(
            summary="Grounded answer",
            facts=["Aggregate fact"],
            recommendations=[],
            warnings=[],
            actions=[],
            model="test-model",
            input_tokens=12,
            output_tokens=7,
        )


def code(response):
    return response.json().get("detail", {}).get("code")


def test_full_commercial_flow(client, db, monkeypatch):
    email = "flow@example.com"
    password = "correct horse battery staple"
    signup = client.post("/v1/auth/signup", json={"email": email, "password": password})
    assert signup.status_code == 201
    login = client.post("/v1/auth/login", json={"email": email, "password": password})
    assert login.status_code == 200
    token = login.json()["access_token"]
    headers = {"Authorization": f"Bearer {token}"}
    device_id = "stable-development-device-001"
    registration = client.post(
        "/v1/devices/register",
        headers=headers,
        json={
            "device_public_id": device_id,
            "device_name": "Test PC",
            "platform": "windows",
            "architecture": "x86_64",
            "app_version": "0.6.0",
        },
    )
    assert registration.status_code == 200
    free = client.get(
        "/v1/entitlement", headers=headers, params={"device_public_id": device_id}
    )
    assert free.json()["plan"] == "FREE"
    public = Ed25519PublicKey.from_public_bytes(
        base64.urlsafe_b64decode(entitlement_public_key())
    )
    payload = jwt.decode(free.json()["assertion"], public, algorithms=["EdDSA"])
    assert payload["device_public_id"] == device_id
    entitlement = db.get(Entitlement, uuid.UUID(payload["user_id"]))
    entitlement.plan = "PRO"
    entitlement.features = for_plan("PRO")
    entitlement.ai_monthly_limit = 5
    db.commit()
    pro = client.get(
        "/v1/entitlement", headers=headers, params={"device_public_id": device_id}
    )
    assert pro.json()["capabilities"]["can_use_ai_copilot"] is True
    monkeypatch.setattr(ai_gateway, "provider", MockProvider())
    answer = client.post(
        "/v1/copilot/query",
        headers=headers,
        json={
            "app_version": "0.6.0",
            "device_id": device_id,
            "question": "What uses space?",
            "context": {"scan_bytes": 100},
        },
    )
    assert answer.status_code == 200
    usage = db.scalar(select(AiUsage))
    assert (usage.model, usage.input_tokens, usage.output_tokens) == (
        "test-model",
        12,
        7,
    )
    device_row = registration.json()["id"]
    assert (
        client.delete(f"/v1/devices/{device_row}", headers=headers).status_code == 204
    )
    denied = client.post(
        "/v1/copilot/query",
        headers=headers,
        json={
            "app_version": "0.6.0",
            "device_id": device_id,
            "question": "Again",
            "context": {"scan_bytes": 100},
        },
    )
    assert code(denied) == "DEVICE_REVOKED"


def test_privacy_validation_and_no_prompt_storage(client, db):
    signup = client.post(
        "/v1/auth/signup",
        json={
            "email": "privacy@example.com",
            "password": "correct horse battery staple",
        },
    ).json()
    payload = {
        "app_version": "0.6.0",
        "device_id": "stable-development-device-001",
        "question": "q",
        "context": {"filename": "secret.txt"},
    }
    response = client.post(
        "/v1/copilot/query",
        headers={"Authorization": f"Bearer {signup['access_token']}"},
        json=payload,
    )
    assert response.status_code == 422
    assert db.scalar(select(func.count()).select_from(AiUsage)) == 0
    assert not any(
        name in AiUsage.__table__.columns
        for name in ["prompt", "context", "filename", "path", "hash"]
    )


def test_rate_limit_is_deterministic(client):
    body = {"email": "limited@example.com", "password": "correct horse battery staple"}
    statuses = [client.post("/v1/auth/signup", json=body).status_code for _ in range(6)]
    assert statuses[-1] == 429
    assert code(client.post("/v1/auth/signup", json=body)) == "RATE_LIMITED"


def test_signed_assertion_rejects_tampering(client):
    signup = client.post(
        "/v1/auth/signup",
        json={
            "email": "signed@example.com",
            "password": "correct horse battery staple",
        },
    ).json()
    headers = {"Authorization": f"Bearer {signup['access_token']}"}
    device_id = "stable-development-device-002"
    client.post(
        "/v1/devices/register",
        headers=headers,
        json={
            "device_public_id": device_id,
            "device_name": "PC",
            "platform": "windows",
            "architecture": "x86_64",
            "app_version": "0.6.0",
        },
    )
    assertion = client.get(
        "/v1/entitlement", headers=headers, params={"device_public_id": device_id}
    ).json()["assertion"]
    parts = assertion.split(".")
    payload = list(parts[1])
    payload[len(payload) // 2] = "A" if payload[len(payload) // 2] != "A" else "B"
    broken = f"{parts[0]}.{''.join(payload)}.{parts[2]}"
    try:
        public = Ed25519PublicKey.from_public_bytes(
            base64.urlsafe_b64decode(entitlement_public_key())
        )
        jwt.decode(broken, public, algorithms=["EdDSA"])
        assert False, "tampered assertion accepted"
    except jwt.InvalidTokenError:
        pass
