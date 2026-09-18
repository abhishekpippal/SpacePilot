from datetime import datetime, timedelta, timezone
from sqlalchemy import select
from .capabilities import for_plan
from .config import get_settings
from .models import Entitlement, SubscriptionRecord


def reconcile_entitlement(db, user_id):
    now = datetime.now(timezone.utc)
    subs = db.scalars(
        select(SubscriptionRecord)
        .where(SubscriptionRecord.user_id == user_id)
        .order_by(SubscriptionRecord.current_period_end.desc())
    ).all()
    active = next(
        (
            s
            for s in subs
            if s.status in {"active", "trialing"}
            and (not s.current_period_end or s.current_period_end > now)
        ),
        None,
    )
    past = next(
        (
            s
            for s in subs
            if s.status == "past_due"
            and s.current_period_end
            and s.current_period_end + timedelta(days=get_settings().billing_grace_days)
            > now
        ),
        None,
    )
    chosen = active or past
    e = db.scalar(
        select(Entitlement).where(Entitlement.user_id == user_id).with_for_update()
    )
    if chosen:
        e.plan = "PRO"
        e.status = "ACTIVE" if active else "BILLING_GRACE"
        e.features = for_plan("PRO")
        e.device_limit = 5
        e.ai_monthly_limit = get_settings().ai_pro_monthly_limit
        e.valid_until = chosen.current_period_end
    else:
        e.plan = "FREE"
        e.status = "ACTIVE"
        e.features = for_plan("FREE")
        e.device_limit = 1
        e.ai_monthly_limit = get_settings().ai_free_monthly_limit
        e.valid_until = None
    db.flush()
    return e
