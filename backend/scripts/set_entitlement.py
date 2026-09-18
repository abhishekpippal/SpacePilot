"""Development-only entitlement administration. Never exposed over HTTP."""

import argparse
import sys
from pathlib import Path

from sqlalchemy import select

sys.path.insert(0, str(Path(__file__).parents[1]))

from app.capabilities import for_plan  # noqa: E402
from app.config import get_settings  # noqa: E402
from app.database import SessionLocal  # noqa: E402
from app.models import Entitlement, User  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["grant-pro", "grant-free", "show"])
    parser.add_argument("email")
    args = parser.parse_args()
    if get_settings().app_env != "development":
        print("This command is available only in development.", file=sys.stderr)
        return 2
    with SessionLocal() as db:
        user = db.scalar(
            select(User).where(User.normalized_email == args.email.lower())
        )
        if not user:
            print("Account not found.", file=sys.stderr)
            return 1
        entitlement = db.get(Entitlement, user.id)
        if args.action != "show":
            plan = "PRO" if args.action == "grant-pro" else "FREE"
            entitlement.plan = plan
            entitlement.features = for_plan(plan)
            entitlement.device_limit = 5 if plan == "PRO" else 1
            entitlement.ai_monthly_limit = (
                get_settings().ai_pro_monthly_limit
                if plan == "PRO"
                else get_settings().ai_free_monthly_limit
            )
            db.commit()
        print(f"{user.email}: {entitlement.plan} ({entitlement.status})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
