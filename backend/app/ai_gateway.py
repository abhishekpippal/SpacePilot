from datetime import datetime, timezone
from time import monotonic

from fastapi import APIRouter, Depends, Request
from sqlalchemy import func, select
from sqlalchemy.orm import Session

from .ai_provider import AiProvider, OpenAiProvider, ProviderUnavailable
from .auth import current, error
from .database import session
from .models import AiUsage, Device, Entitlement, SecurityEvent
from .rate_limits import client_key, enforce
from .schemas import CopilotRequest

router = APIRouter(prefix="/v1/copilot", tags=["copilot"])
provider: AiProvider = OpenAiProvider()


@router.post("/query")
def query(
    body: CopilotRequest,
    request: Request,
    db: Session = Depends(session),
    u=Depends(current),
):
    device = db.scalar(
        select(Device).where(
            Device.user_id == u.id,
            Device.device_public_id == body.device_id,
            Device.revoked_at.is_(None),
        )
    )
    if not device:
        error("DEVICE_REVOKED", 403)
    entitlement = db.scalar(
        select(Entitlement).where(Entitlement.user_id == u.id).with_for_update()
    )
    if (
        not entitlement
        or entitlement.status != "ACTIVE"
        or not entitlement.features.get("can_use_ai_copilot")
    ):
        error("ENTITLEMENT_REQUIRED", 403)
    month = datetime.now(timezone.utc).replace(
        day=1, hour=0, minute=0, second=0, microsecond=0
    )
    used = db.scalar(
        select(func.count())
        .select_from(AiUsage)
        .where(AiUsage.user_id == u.id, AiUsage.created_at >= month)
    )
    if used >= entitlement.ai_monthly_limit:
        db.add(
            SecurityEvent(user_id=u.id, event_type="ai_quota_denial", metadata_json={})
        )
        db.commit()
        error("AI_QUOTA_EXCEEDED", 402)
    enforce(client_key(request, "ai", f"{u.id}:{device.id}"), 10)
    started = monotonic()
    try:
        result = provider.generate(body.question, body.context)
    except ProviderUnavailable:
        db.add(
            AiUsage(
                user_id=u.id,
                device_id=device.id,
                model="provider",
                request_status="PROVIDER_FAILURE",
                latency_ms=int((monotonic() - started) * 1000),
            )
        )
        db.commit()
        error("PROVIDER_UNAVAILABLE", 503)
    db.add(
        AiUsage(
            user_id=u.id,
            device_id=device.id,
            model=result.model,
            request_status="SUCCESS",
            input_tokens=result.input_tokens,
            output_tokens=result.output_tokens,
            latency_ms=int((monotonic() - started) * 1000),
        )
    )
    db.commit()
    return result.model_dump()
