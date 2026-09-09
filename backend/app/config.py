from pathlib import Path

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    database_url: str = "postgresql+psycopg://lesson1:lesson1@localhost:5432/lesson1"
    content_manifest_path: Path = Path(__file__).parents[2] / "content" / "manifest.json"
    cookie_secure: bool = False
    cors_origin: str = "http://localhost:5173"

    model_config = SettingsConfigDict(env_file=".env", extra="ignore")


settings = Settings()
