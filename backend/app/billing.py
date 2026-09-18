from datetime import datetime, timezone
import uuid
from fastapi import APIRouter, Depends, Request
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session
from .auth import current, error
from .config import get_settings
from .database import session
from .entitlement_service import reconcile_entitlement
from .models import PaymentEvent, SubscriptionRecord
from .payments import PaymentError, StripeProvider
from .plans import paid_plans
from .rate_limits import client_key, enforce

router = APIRouter(prefix="/v1/billing", tags=["billing"])
provider = StripeProvider()


def dt(value):
    return datetime.fromtimestamp(value, tz=timezone.utc) if value else None


@router.get("/status")
def status(db: Session = Depends(session), u=Depends(current)):
    s = db.scalar(
        select(SubscriptionRecord)
        .where(SubscriptionRecord.user_id == u.id)
        .order_by(SubscriptionRecord.updated_at.desc())
    )
    return {
        "plan": s.internal_plan if s else "FREE",
        "status": s.status if s else "free",
        "current_period_end": s.current_period_end if s else None,
        "cancel_at_period_end": s.cancel_at_period_end if s else False,
    }


@router.post("/checkout")
def checkout(body: dict, request: Request, u=Depends(current)):
    enforce(client_key(request, "checkout", str(u.id)), 5, 300)
    plan = paid_plans().get(body.get("plan"))
    if not plan:
        error("INVALID_PLAN", 400)
    if not plan.price_id:
        error("BILLING_NOT_CONFIGURED", 503)
    try:
        return {
            "url": provider.create_checkout_session(
                price_id=plan.price_id,
                user_id=str(u.id),
                email=u.email,
                plan=plan.code,
                success_url=get_settings().billing_success_url,
                cancel_url=get_settings().billing_cancel_url,
            )["url"]
        }
    except PaymentError:
        error("PAYMENT_PROVIDER_UNAVAILABLE", 503)


@router.post("/portal")
def portal(db: Session = Depends(session), u=Depends(current)):
    s = db.scalar(
        select(SubscriptionRecord).where(
            SubscriptionRecord.user_id == u.id,
            SubscriptionRecord.provider_customer_id.is_not(None),
        )
    )
    if not s:
        error("NO_BILLING_ACCOUNT", 404)
    try:
        return {
            "url": provider.create_billing_portal_session(
                s.provider_customer_id, get_settings().billing_success_url
            )["url"]
        }
    except PaymentError:
        error("PAYMENT_PROVIDER_UNAVAILABLE", 503)


def process_event(db, event):
    obj = event.get("data", {}).get("object", {})
    kind = event["type"]
    metadata = obj.get("metadata", {})
    if kind == "checkout.session.completed":
        return
    subscription_id = (
        obj.get("subscription") if kind.startswith("invoice.") else obj.get("id")
    )
    if not subscription_id:
        return
    row = db.scalar(
        select(SubscriptionRecord)
        .where(SubscriptionRecord.external_id == subscription_id)
        .with_for_update()
    )
    user_id = metadata.get("spacepilot_user_id")
    if not row and user_id:
        row = SubscriptionRecord(
            user_id=uuid.UUID(user_id),
            provider="stripe",
            external_id=subscription_id,
            status="incomplete",
        )
        db.add(row)
    if not row:
        return
    if kind == "invoice.payment_failed":
        row.status = "past_due"
    elif kind in {
        "invoice.paid",
        "customer.subscription.created",
        "customer.subscription.updated",
    }:
        row.status = (
            obj.get("status", "active") if kind.startswith("customer.") else "active"
        )
    elif kind == "customer.subscription.deleted":
        row.status = "canceled"
        row.canceled_at = datetime.now(timezone.utc)
    row.provider_customer_id = (
        str(obj.get("customer") or row.provider_customer_id or "") or None
    )
    row.internal_plan = metadata.get("spacepilot_plan", row.internal_plan)
    row.provider_price_id = (
        ((obj.get("items") or {}).get("data") or [{}])[0]
        .get("price", {})
        .get("id", row.provider_price_id)
    )
    row.current_period_start = dt(obj.get("current_period_start"))
    row.current_period_end = dt(obj.get("current_period_end"))
    row.cancel_at_period_end = bool(obj.get("cancel_at_period_end", False))
    reconcile_entitlement(db, row.user_id)


@router.post("/webhook")
async def webhook(request: Request, db: Session = Depends(session)):
    raw = await request.body()
    try:
        event = provider.verify_webhook(
            raw, request.headers.get("stripe-signature", "")
        )
    except (PaymentError, ValueError):
        error("INVALID_WEBHOOK_SIGNATURE", 400)
    record = PaymentEvent(
        provider="stripe", provider_event_id=event["id"], event_type=event["type"]
    )
    db.add(record)
    try:
        db.flush()
    except IntegrityError:
        db.rollback()
        return {"status": "duplicate"}
    try:
        process_event(db, event)
        record.processing_status = "PROCESSED"
        record.processed_at = datetime.now(timezone.utc)
        db.commit()
    except Exception:
        db.rollback()
        error("WEBHOOK_PROCESSING_FAILED", 500)
    return {"status": "processed"}
