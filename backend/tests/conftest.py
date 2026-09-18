import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

os.environ.setdefault("DATABASE_URL", "sqlite+pysqlite://")
os.environ.setdefault("JWT_SIGNING_SECRET", "test-signing-secret-that-is-long-enough")
os.environ.setdefault(
    "ENTITLEMENT_SIGNING_PRIVATE_KEY", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
)

import pytest
from fastapi.testclient import TestClient
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker
from sqlalchemy.pool import StaticPool

from app.database import Base, session
from app.main import app
from app.rate_limits import reset


@pytest.fixture
def db():
    engine = create_engine(
        "sqlite+pysqlite://",
        connect_args={"check_same_thread": False},
        poolclass=StaticPool,
    )
    Base.metadata.create_all(engine)
    factory = sessionmaker(bind=engine, expire_on_commit=False)
    with factory() as value:
        yield value
    Base.metadata.drop_all(engine)


@pytest.fixture
def client(db):
    app.dependency_overrides[session] = lambda: db
    reset()
    with TestClient(app) as value:
        yield value
    app.dependency_overrides.clear()
    reset()
