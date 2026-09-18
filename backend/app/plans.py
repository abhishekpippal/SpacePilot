from dataclasses import dataclass
from .capabilities import for_plan
from .config import get_settings


@dataclass(frozen=True)
class Plan:
    code: str
    price_id: str
    interval: str
    device_limit: int
    ai_allowance: int

    @property
    def features(self):
        return for_plan("PRO")


def paid_plans():
    s = get_settings()
    return {
        "PRO_MONTHLY": Plan(
            "PRO_MONTHLY",
            s.stripe_price_pro_monthly,
            "month",
            5,
            s.ai_pro_monthly_limit,
        ),
        "PRO_ANNUAL": Plan(
            "PRO_ANNUAL", s.stripe_price_pro_annual, "year", 5, s.ai_pro_monthly_limit
        ),
    }
