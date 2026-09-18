from collections import defaultdict, deque
from time import monotonic
from fastapi import HTTPException

_events = defaultdict(deque)


def enforce(key: str, limit: int, window: int = 60):
    now = monotonic()
    bucket = _events[key]
    while bucket and bucket[0] <= now - window:
        bucket.popleft()
    if len(bucket) >= limit:
        raise HTTPException(
            429, {"code": "RATE_LIMITED", "message": "Try again later."}
        )
    bucket.append(now)


def reset():
    _events.clear()


def client_key(request, operation: str, subject: str = "anonymous") -> str:
    host = request.client.host if request.client else "native"
    return f"{operation}:{subject}:{host}"
