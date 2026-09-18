from datetime import datetime, timezone, timedelta
import uuid
from fastapi import APIRouter, Depends, Request
from sqlalchemy import func, select
from sqlalchemy.orm import Session
from .auth import current, error
from .config import get_settings
from .database import session
from .models import AiUsage, Device, Entitlement, SecurityEvent
from .schemas import DeviceInput
from .security import sign_entitlement
from .rate_limits import client_key, enforce

router = APIRouter(prefix="/v1", tags=["devices"])


@router.post("/devices/register")
def register(
    body: DeviceInput,
    request: Request,
    db: Session = Depends(session),
    u=Depends(current),
):
    enforce(client_key(request, "device-register", str(u.id)), 12)
    ent = db.scalar(
        select(Entitlement).where(Entitlement.user_id == u.id).with_for_update()
    )
    row = db.scalar(
        select(Device).where(
            Device.user_id == u.id, Device.device_public_id == body.device_public_id
        )
    )
    if row:
        if row.revoked_at:
            error("DEVICE_REVOKED", 403)
        for key in ("device_name", "platform", "architecture", "app_version"):
            setattr(row, key, getattr(body, key))
        row.last_seen_at = datetime.now(timezone.utc)
        db.commit()
        return row
    count = db.scalar(
        select(func.count())
        .select_from(Device)
        .where(Device.user_id == u.id, Device.revoked_at.is_(None))
    )
    if count >= ent.device_limit:
        db.add(
            SecurityEvent(
                user_id=u.id, event_type="device_limit_denial", metadata_json={}
            )
        )
        db.commit()
        error("DEVICE_LIMIT_REACHED", 403)
    row = Device(user_id=u.id, **body.model_dump())
    db.add(row)
    db.add(
        SecurityEvent(user_id=u.id, event_type="device_registration", metadata_json={})
    )
    db.commit()
    db.refresh(row)
    return row


@router.get("/devices")
def list_devices(db: Session = Depends(session), u=Depends(current)):
    return db.scalars(select(Device).where(Device.user_id == u.id)).all()


@router.delete("/devices/{device_id}", status_code=204)
def revoke(device_id: str, db: Session = Depends(session), u=Depends(current)):
    try:
        parsed_id = uuid.UUID(device_id)
    except ValueError:
        error("INVALID_REQUEST", 404)
    row = db.get(Device, parsed_id)
    if not row or row.user_id != u.id:
        error("INVALID_REQUEST", 404)
    row.revoked_at = datetime.now(timezone.utc)
    db.add(
        SecurityEvent(user_id=u.id, event_type="device_revocation", metadata_json={})
    )
    db.commit()


@router.get("/entitlement")
def entitlement(
    device_public_id: str,
    request: Request,
    db: Session = Depends(session),
    u=Depends(current),
):
    enforce(client_key(request, "entitlement", str(u.id)), 30)
    d = db.scalar(
        select(Device).where(
            Device.user_id == u.id,
            Device.device_public_id == device_public_id,
            Device.revoked_at.is_(None),
        )
    )
    if not d:
        error("DEVICE_REVOKED", 403)
    e = db.get(Entitlement, u.id)
    now = datetime.now(timezone.utc)
    grace = e.grace_until or now + timedelta(days=get_settings().offline_grace_days)
    payload = {
        "version": 1,
        "user_id": str(u.id),
        "device_public_id": device_public_id,
        "plan": e.plan,
        "capabilities": e.features,
        "device_limit": e.device_limit,
        "issued_at": int(now.timestamp()),
        "expires_at": int((now + timedelta(hours=24)).timestamp()),
        "grace_until": int(grace.timestamp()),
    }
    month = now.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
    used = db.scalar(
        select(func.count())
        .select_from(AiUsage)
        .where(AiUsage.user_id == u.id, AiUsage.created_at >= month)
    )
    return {
        "plan": e.plan,
        "status": e.status,
        "capabilities": e.features,
        "device_limit": e.device_limit,
        "ai_allowance": e.ai_monthly_limit,
        "ai_usage": used,
        "valid_until": e.valid_until,
        "grace_until": grace,
        "assertion": sign_entitlement(payload),
    }
