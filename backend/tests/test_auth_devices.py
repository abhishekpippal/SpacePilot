from datetime import datetime, timedelta, timezone
from sqlalchemy import select
from app.models import AuthSession, User

PASSWORD = "correct horse battery staple"


def signup(client, email="user@example.com"):
    return client.post("/v1/auth/signup", json={"email": email, "password": PASSWORD})


def headers(response):
    return {"Authorization": f"Bearer {response.json()['access_token']}"}


def device(identifier="stable-device-identifier-0001"):
    return {
        "device_public_id": identifier,
        "device_name": "PC",
        "platform": "windows",
        "architecture": "x86_64",
        "app_version": "0.6.0",
    }


def test_signup_duplicate_login_and_access(client):
    first = signup(client)
    assert first.status_code == 201
    assert signup(client).status_code == 409
    assert (
        client.post(
            "/v1/auth/login",
            json={"email": "user@example.com", "password": "wrong password value"},
        ).status_code
        == 401
    )
    login = client.post(
        "/v1/auth/login", json={"email": "user@example.com", "password": PASSWORD}
    )
    assert login.status_code == 200
    assert (
        client.get("/v1/auth/me", headers=headers(login)).json()["email"]
        == "user@example.com"
    )
    assert client.get("/v1/auth/me").status_code == 401


def test_refresh_rotation_logout_and_reuse(client):
    issued = signup(client).json()
    rotated = client.post(
        "/v1/auth/refresh", json={"refresh_token": issued["refresh_token"]}
    )
    assert rotated.status_code == 200
    assert (
        client.post(
            "/v1/auth/refresh", json={"refresh_token": issued["refresh_token"]}
        ).status_code
        == 401
    )
    new = rotated.json()["refresh_token"]
    assert (
        client.post("/v1/auth/logout", json={"refresh_token": new}).status_code == 204
    )
    assert (
        client.post("/v1/auth/refresh", json={"refresh_token": new}).status_code == 401
    )


def test_suspended_account_is_denied(client, db):
    issued = signup(client)
    u = db.scalar(select(User))
    u.account_status = "SUSPENDED"
    db.commit()
    assert client.get("/v1/auth/me", headers=headers(issued)).status_code == 403


def test_device_idempotency_limit_revoke_and_wrong_user(client):
    one = signup(client, "one@example.com")
    h1 = headers(one)
    first = client.post("/v1/devices/register", headers=h1, json=device())
    second = client.post(
        "/v1/devices/register", headers=h1, json={**device(), "app_version": "0.6.1"}
    )
    assert first.json()["id"] == second.json()["id"]
    assert (
        client.post(
            "/v1/devices/register",
            headers=h1,
            json=device("stable-device-identifier-0002"),
        ).status_code
        == 403
    )
    two = signup(client, "two@example.com")
    assert (
        client.delete(
            f"/v1/devices/{first.json()['id']}", headers=headers(two)
        ).status_code
        == 404
    )
    assert (
        client.delete(f"/v1/devices/{first.json()['id']}", headers=h1).status_code
        == 204
    )
    assert (
        client.get(
            "/v1/entitlement",
            headers=h1,
            params={"device_public_id": device()["device_public_id"]},
        ).status_code
        == 403
    )


def test_expired_reset_token(client, db, monkeypatch):
    from app import password_reset

    monkeypatch.setattr(
        password_reset,
        "token_urlsafe",
        lambda n: "expired-reset-token-that-is-long-enough",
    )
    signup(client)
    client.post("/v1/auth/password-reset/request", json={"email": "user@example.com"})
    row = db.scalar(select(password_reset.PasswordReset))
    row.expires_at = datetime.now(timezone.utc) - timedelta(seconds=1)
    db.commit()
    assert (
        client.post(
            "/v1/auth/password-reset/confirm",
            json={
                "token": "expired-reset-token-that-is-long-enough",
                "password": "another secure password",
            },
        ).status_code
        == 400
    )


def test_password_reset_revokes_all_sessions(client, db, monkeypatch):
    from app import password_reset

    monkeypatch.setattr(
        password_reset,
        "token_urlsafe",
        lambda n: "reset-token-that-is-definitely-long-enough",
    )
    original = signup(client).json()
    client.post(
        "/v1/auth/login", json={"email": "user@example.com", "password": PASSWORD}
    )
    client.post("/v1/auth/password-reset/request", json={"email": "user@example.com"})
    client.post(
        "/v1/auth/password-reset/confirm",
        json={
            "token": "reset-token-that-is-definitely-long-enough",
            "password": "another secure password",
        },
    )
    assert all(s.revoked_at for s in db.scalars(select(AuthSession)).all())
    assert (
        client.post(
            "/v1/auth/refresh", json={"refresh_token": original["refresh_token"]}
        ).status_code
        == 401
    )
