from abc import ABC, abstractmethod
import httpx
from .config import get_settings


class EmailUnavailable(Exception):
    pass


class EmailDelivery(ABC):
    @abstractmethod
    def send(self, to, subject, html): ...


class DevelopmentEmail(EmailDelivery):
    def send(self, to, subject, html):
        return True


class ResendEmail(EmailDelivery):
    def send(self, to, subject, html):
        s = get_settings()
        if not s.resend_api_key:
            raise EmailUnavailable("email delivery is not configured")
        r = httpx.post(
            "https://api.resend.com/emails",
            headers={"Authorization": f"Bearer {s.resend_api_key}"},
            json={"from": s.email_from, "to": [to], "subject": subject, "html": html},
            timeout=15,
        )
        r.raise_for_status()
        return True


def email_provider():
    return (
        DevelopmentEmail() if get_settings().app_env == "development" else ResendEmail()
    )
