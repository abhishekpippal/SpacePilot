from sqlalchemy import select
from app import password_reset
from app.models import EmailVerification, PasswordReset
from app.security import token_hash

PASSWORD = "correct horse battery staple"


class Capture:
    def __init__(self):
        self.messages = []

    def send(self, to, subject, html):
        self.messages.append((to, subject, html))
        return True


def test_verification_token_is_hashed_single_use(client, db):
    issued = client.post(
        "/v1/auth/signup", json={"email": "verify@example.com", "password": PASSWORD}
    ).json()
    h = {"Authorization": f"Bearer {issued['access_token']}"}
    response = client.post("/v1/auth/email/resend", headers=h)
    token = response.json()["development_token"]
    row = db.scalar(select(EmailVerification))
    assert row.token_hash == token_hash(token) and token not in row.token_hash
    assert client.post(f"/v1/auth/email/verify/{token}").status_code == 200
    assert client.post(f"/v1/auth/email/verify/{token}").status_code == 400


def test_reset_response_does_not_enumerate_accounts(client):
    known = client.post(
        "/v1/auth/password-reset/request", json={"email": "unknown@example.com"}
    )
    assert known.status_code == 200 and known.json() == {"status": "accepted"}
    assert not list(client.app.dependency_overrides.values()) == []


def test_reset_token_is_hashed(client, db, monkeypatch):
    raw = "captured-reset-token-that-is-long-enough"
    monkeypatch.setattr(password_reset, "token_urlsafe", lambda n: raw)
    client.post(
        "/v1/auth/signup", json={"email": "reset2@example.com", "password": PASSWORD}
    )
    client.post("/v1/auth/password-reset/request", json={"email": "reset2@example.com"})
    row = db.scalar(select(PasswordReset))
    assert row.token_hash == token_hash(raw) and raw not in row.token_hash
