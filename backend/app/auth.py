from datetime import datetime, timedelta, timezone
import uuid
import jwt
from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session
from .capabilities import for_plan
from .config import get_settings
from .database import session
from .models import AuthSession, Entitlement, SecurityEvent, User
from .schemas import Login, Refresh, Signup
from .security import (
    access,
    decode_access,
    new_refresh,
    password_hash,
    password_valid,
    token_hash,
)
from .rate_limits import client_key, enforce

router = APIRouter(prefix="/v1/auth", tags=["auth"])
bearer = HTTPBearer(auto_error=False)


def error(code, status=400):
    raise HTTPException(
        status, {"code": code, "message": "Request could not be completed."}
    )


def current(
    db: Session = Depends(session),
    credentials: HTTPAuthorizationCredentials | None = Depends(bearer),
):
    if not credentials:
        error("AUTH_INVALID", 401)
    try:
        value = db.get(User, uuid.UUID(decode_access(credentials.credentials)))
    except jwt.ExpiredSignatureError:
        error("AUTH_EXPIRED", 401)
    except Exception:
        error("AUTH_INVALID", 401)
    if not value:
        error("AUTH_INVALID", 401)
    if value.account_status != "ACTIVE":
        error("ACCOUNT_SUSPENDED", 403)
    return value


def issue(db, u):
    raw = new_refresh()
    db.add(
        AuthSession(
            user_id=u.id,
            refresh_hash=token_hash(raw),
            expires_at=datetime.now(timezone.utc)
            + timedelta(days=get_settings().refresh_token_days),
        )
    )
    db.commit()
    return {
        "access_token": access(u.id),
        "refresh_token": raw,
        "token_type": "bearer",
        "expires_in": get_settings().access_token_minutes * 60,
    }


@router.post("/signup", status_code=201)
def signup(body: Signup, request: Request, db: Session = Depends(session)):
    enforce(client_key(request, "signup", body.email.strip().lower()), 5, 900)
    normalized = body.email.strip().lower()
    u = User(
        email=body.email,
        normalized_email=normalized,
        password_hash=password_hash(body.password),
    )
    db.add(u)
    try:
        db.flush()
    except IntegrityError:
        db.rollback()
        error("ACCOUNT_EXISTS", 409)
    db.add(
        Entitlement(
            user_id=u.id,
            plan="FREE",
            status="ACTIVE",
            features=for_plan("FREE"),
            device_limit=1,
            ai_monthly_limit=get_settings().ai_free_monthly_limit,
        )
    )
    db.commit()
    return issue(db, u)


@router.post("/login")
def login(body: Login, request: Request, db: Session = Depends(session)):
    enforce(client_key(request, "login", body.email.strip().lower()), 10, 300)
    u = db.scalar(
        select(User).where(User.normalized_email == body.email.strip().lower())
    )
    if not u or not password_valid(u.password_hash, body.password):
        db.add(SecurityEvent(event_type="login_failure", metadata_json={}))
        db.commit()
        error("AUTH_INVALID", 401)
    if u.account_status != "ACTIVE":
        error("ACCOUNT_SUSPENDED", 403)
    u.last_login_at = datetime.now(timezone.utc)
    db.add(SecurityEvent(user_id=u.id, event_type="login_success", metadata_json={}))
    db.commit()
    return issue(db, u)


@router.post("/refresh")
def refresh(body: Refresh, request: Request, db: Session = Depends(session)):
    enforce(client_key(request, "refresh", token_hash(body.refresh_token)[:16]), 20)
    row = db.scalar(
        select(AuthSession)
        .where(AuthSession.refresh_hash == token_hash(body.refresh_token))
        .with_for_update()
    )
    if not row or row.revoked_at:
        error("AUTH_REVOKED", 401)
    expires = row.expires_at
    if expires.tzinfo is None:
        expires = expires.replace(tzinfo=timezone.utc)
    if expires < datetime.now(timezone.utc):
        error("AUTH_REVOKED", 401)
    row.revoked_at = datetime.now(timezone.utc)
    u = db.get(User, row.user_id)
    db.commit()
    return issue(db, u)


@router.post("/logout", status_code=204)
def logout(body: Refresh, db: Session = Depends(session)):
    row = db.scalar(
        select(AuthSession).where(
            AuthSession.refresh_hash == token_hash(body.refresh_token)
        )
    )
    if row:
        row.revoked_at = datetime.now(timezone.utc)
        db.commit()


@router.get("/me")
def me(u=Depends(current)):
    return {
        "id": u.id,
        "email": u.email,
        "email_verified": u.email_verified,
        "status": u.account_status,
    }
