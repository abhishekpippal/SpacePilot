from sqlalchemy import select
from app import ai_gateway
from app.ai_provider import ProviderUnavailable
from app.capabilities import for_plan
from app.models import AiUsage, Entitlement


def setup(client, db, pro=False):
    issued = client.post(
        "/v1/auth/signup",
        json={"email": "ai@example.com", "password": "correct horse battery staple"},
    )
    h = {"Authorization": f"Bearer {issued.json()['access_token']}"}
    identifier = "stable-ai-device-identifier"
    client.post(
        "/v1/devices/register",
        headers=h,
        json={
            "device_public_id": identifier,
            "device_name": "PC",
            "platform": "windows",
            "architecture": "x86_64",
            "app_version": "0.6.0",
        },
    )
    if pro:
        e = db.scalar(select(Entitlement))
        e.plan = "PRO"
        e.features = for_plan("PRO")
        e.ai_monthly_limit = 1
        db.commit()
    return h, identifier


def query(client, h, d, context=None):
    return client.post(
        "/v1/copilot/query",
        headers=h,
        json={
            "app_version": "0.6.0",
            "device_id": d,
            "question": "help",
            "context": context or {"scan_bytes": 5},
        },
    )


def test_ai_auth_free_and_device_enforcement(client, db):
    assert query(client, {}, "missing").status_code == 401
    h, d = setup(client, db)
    assert query(client, h, d).status_code == 403
    assert query(client, h, "wrong-device-identifier-value").status_code == 403


def test_ai_provider_failure_is_accounted_without_private_data(client, db, monkeypatch):
    class Failed:
        def generate(self, *args):
            raise ProviderUnavailable()

    h, d = setup(client, db, True)
    monkeypatch.setattr(ai_gateway, "provider", Failed())
    assert query(client, h, d).status_code == 503
    usage = db.scalar(select(AiUsage))
    assert usage.request_status == "PROVIDER_FAILURE"
    assert not {"prompt", "context", "path", "filename", "hash"} & set(
        AiUsage.__table__.columns.keys()
    )


def test_ai_quota_and_privacy_limits(client, db, monkeypatch):
    class Ok:
        def generate(self, *args):
            from app.ai_provider import ProviderResult

            return ProviderResult(
                summary="ok",
                facts=[],
                recommendations=[],
                warnings=[],
                actions=[],
                model="fake",
            )

    h, d = setup(client, db, True)
    monkeypatch.setattr(ai_gateway, "provider", Ok())
    assert query(client, h, d).status_code == 200
    assert query(client, h, d).status_code == 402
    assert query(client, h, d, {"filename": "private.txt"}).status_code == 422
    assert query(client, h, d, {"aggregate": "x" * 17000}).status_code == 422
