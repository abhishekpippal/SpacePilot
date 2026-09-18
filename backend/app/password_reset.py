from datetime import datetime, timedelta, timezone
from secrets import token_urlsafe
from fastapi import APIRouter, Depends, Request
from pydantic import BaseModel, EmailStr, Field
from sqlalchemy import select, update
from sqlalchemy.orm import Session
from .auth import error
from .config import get_settings
from .database import session
from .email_delivery import email_provider
from .models import AuthSession, PasswordReset, User
from .rate_limits import client_key, enforce
from .security import password_hash, token_hash

router = APIRouter(prefix="/v1/auth/password-reset", tags=["auth"])


class RequestReset(BaseModel):
    email: EmailStr


class ConfirmReset(BaseModel):
    token: str = Field(min_length=32, max_length=256)
    password: str = Field(min_length=12, max_length=128)


@router.post("/request")
def request_reset(body: RequestReset, request: Request, db: Session = Depends(session)):
    enforce(client_key(request, "password-reset", body.email.lower()), 3, 3600)
    u = db.scalar(select(User).where(User.normalized_email == body.email.lower()))
    if u:
        raw = token_urlsafe(32)
        db.add(
            PasswordReset(
                user_id=u.id,
                token_hash=token_hash(raw),
                expires_at=datetime.now(timezone.utc) + timedelta(hours=1),
            )
        )
        db.commit()
        try:
            email_provider().send(
                u.email,
                "Reset your SpacePilot password",
                f'<p>Use this secure link to reset your password:</p><p><a href="{get_settings().password_reset_url}?token={raw}">Reset password</a></p><p>This link expires in one hour.</p>',
            )
        except Exception:
            pass
    return {"status": "accepted"}


@router.post("/confirm")
def confirm(body: ConfirmReset, db: Session = Depends(session)):
    row = db.scalar(
        select(PasswordReset).where(PasswordReset.token_hash == token_hash(body.token))
    )
    if not row or row.used_at:
        error("INVALID_RESET_TOKEN", 400)
    expires = row.expires_at
    if expires and expires.tzinfo is None:
        expires = expires.replace(tzinfo=timezone.utc)
    if expires < datetime.now(timezone.utc):
        error("INVALID_RESET_TOKEN", 400)
    u = db.get(User, row.user_id)
    u.password_hash = password_hash(body.password)
    row.used_at = datetime.now(timezone.utc)
    db.execute(
        update(AuthSession)
        .where(AuthSession.user_id == u.id, AuthSession.revoked_at.is_(None))
        .values(revoked_at=datetime.now(timezone.utc))
    )
    db.commit()
    return {"status": "password_reset"}
