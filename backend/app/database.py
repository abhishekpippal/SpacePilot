from sqlalchemy import create_engine
from sqlalchemy.orm import DeclarativeBase, sessionmaker
from .config import get_settings


class Base(DeclarativeBase):
    pass


database_url = get_settings().database_url
pool_options = (
    {} if database_url.startswith("sqlite") else {"pool_size": 5, "max_overflow": 10}
)
engine = create_engine(
    database_url,
    pool_pre_ping=True,
    pool_recycle=300,
    hide_parameters=True,
    **pool_options,
)
SessionLocal = sessionmaker(bind=engine, expire_on_commit=False)


def session():
    db = SessionLocal()
    try:
        yield db
    except Exception:
        db.rollback()
        raise
    finally:
        db.close()
