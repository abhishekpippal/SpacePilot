from datetime import datetime, timedelta, timezone
import secrets
from fastapi import APIRouter, Depends, Request
from sqlalchemy import select
from sqlalchemy.orm import Session
from .auth import current, error
from .database import session
from .models import EmailVerification, User
from .rate_limits import enforce
from .security import token_hash
from .email_delivery import email_provider
from .config import get_settings

router = APIRouter(prefix="/v1/auth/email", tags=["email"])


class DevelopmentDelivery:
    def deliver(self, email: str, token: str):
        return token


delivery = DevelopmentDelivery()


def create(db: Session, u: User):
    raw = secrets.token_urlsafe(32)
    db.add(
        EmailVerification(
            user_id=u.id,
            token_hash=token_hash(raw),
            expires_at=datetime.now(timezone.utc) + timedelta(hours=24),
        )
    )
    db.commit()
    return raw


@router.post("/resend")
def resend(request: Request, db: Session = Depends(session), u=Depends(current)):
    enforce(
        f"email:{u.id}:{request.client.host if request.client else 'native'}", 3, 3600
    )
    if u.email_verified:
        return {"status": "already_verified"}
    token = create(db, u)
    if get_settings().app_env != "development":
        try:
            email_provider().send(
                u.email,
                "Verify your SpacePilot email",
                f"<p>Verify your account with this one-time token:</p><p>{token}</p>",
            )
        except Exception:
            error("EMAIL_DELIVERY_FAILED", 503)
        return {"status": "verification_created"}
    return {
        "status": "verification_created",
        "development_token": delivery.deliver(u.email, token),
    }


@router.post("/verify/{token}")
def verify(token: str, db: Session = Depends(session)):
    row = db.scalar(
        select(EmailVerification).where(
            EmailVerification.token_hash == token_hash(token)
        )
    )
    if not row or row.used_at:
        error("INVALID_REQUEST", 400)
    expires = row.expires_at
    if expires.tzinfo is None:
        expires = expires.replace(tzinfo=timezone.utc)
    if expires < datetime.now(timezone.utc):
        error("INVALID_REQUEST", 400)
    row.used_at = datetime.now(timezone.utc)
    u = db.get(User, row.user_id)
    u.email_verified = True
    db.commit()
    return {"status": "verified"}
