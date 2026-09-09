from pathlib import Path
import pytest
from fastapi.testclient import TestClient
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker
from sqlalchemy.pool import StaticPool
from app.content import ContentRepository
from app.database import get_db
from app.main import app
from app.models import Base

@pytest.fixture
def db_factory():
    engine=create_engine("sqlite://",connect_args={"check_same_thread":False},poolclass=StaticPool)
    Base.metadata.create_all(engine)
    return sessionmaker(bind=engine,expire_on_commit=False)

@pytest.fixture
def client(db_factory):
    def override():
        with db_factory() as session: yield session
    app.dependency_overrides[get_db]=override
    app.state.content=ContentRepository(Path(__file__).parents[2]/"content"/"manifest.json")
    with TestClient(app) as value: yield value
    app.dependency_overrides.clear()
