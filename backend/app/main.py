from contextlib import asynccontextmanager
import json
import logging
from time import monotonic
from fastapi import Depends, FastAPI, HTTPException, Request
from fastapi.middleware.cors import CORSMiddleware
from starlette.middleware.trustedhost import TrustedHostMiddleware
from sqlalchemy import text
from sqlalchemy.orm import Session
from .database import session
from .auth import router as auth_router
from .email_verification import router as email_router
from .devices import router as device_router
from .ai_gateway import router as ai_router
from .billing import router as billing_router
from .password_reset import router as password_reset_router
from .config import get_settings

logger = logging.getLogger("spacepilot.api")
logging.basicConfig(level=logging.INFO, format="%(message)s")


@asynccontextmanager
async def lifespan(app):
    logger.info(
        json.dumps(
            {"event": "application_start", "environment": get_settings().app_env}
        )
    )
    yield
    logger.info(json.dumps({"event": "application_stop"}))


app = FastAPI(title="SpacePilot API", version="0.1.0", lifespan=lifespan)
s = get_settings()
hosts = [value.strip() for value in s.trusted_hosts.split(",") if value.strip()]
if hosts and s.app_env != "development":
    app.add_middleware(TrustedHostMiddleware, allowed_hosts=hosts)
origins = [
    value.strip() for value in s.cors_allowed_origins.split(",") if value.strip()
]
if origins:
    app.add_middleware(
        CORSMiddleware,
        allow_origins=origins,
        allow_credentials=True,
        allow_methods=["GET", "POST", "DELETE"],
        allow_headers=["Authorization", "Content-Type", "Stripe-Signature"],
    )


@app.middleware("http")
async def request_metrics(request: Request, call_next):
    started = monotonic()
    response = await call_next(request)
    route = request.scope.get("route")
    template = getattr(route, "path", "unmatched")
    logger.info(
        json.dumps(
            {
                "event": "request_complete",
                "method": request.method,
                "route": template,
                "status": response.status_code,
                "latency_ms": round((monotonic() - started) * 1000),
            }
        )
    )
    return response


app.include_router(auth_router)
app.include_router(email_router)
app.include_router(device_router)
app.include_router(ai_router)
app.include_router(billing_router)
app.include_router(password_reset_router)


@app.get("/health")
def health():
    return {"status": "ok", "version": "0.1.0"}


@app.get("/ready")
def ready(db: Session = Depends(session)):
    try:
        db.execute(text("select 1"))
        return {"status": "ready", "database": "available"}
    except Exception as error:
        logger.error(json.dumps({"event": "database_readiness_failed"}))
        raise HTTPException(
            503, {"code": "DATABASE_UNAVAILABLE", "message": "Database is unavailable."}
        ) from error
