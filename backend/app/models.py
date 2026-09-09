from datetime import datetime

from sqlalchemy import DateTime, ForeignKey, String, UniqueConstraint, func
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, relationship


class Base(DeclarativeBase):
    pass


class Learner(Base):
    __tablename__ = "learner"

    id: Mapped[str] = mapped_column(String(64), primary_key=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
    completions: Mapped[list["ExerciseCompletion"]] = relationship(cascade="all, delete-orphan")


class ExerciseCompletion(Base):
    __tablename__ = "exercise_completion"
    __table_args__ = (
        UniqueConstraint("learner_id", "exercise_id", "content_version", name="uq_completion_identity"),
    )

    id: Mapped[int] = mapped_column(primary_key=True)
    learner_id: Mapped[str] = mapped_column(ForeignKey("learner.id", ondelete="CASCADE"), index=True)
    exercise_id: Mapped[str] = mapped_column(String(120))
    content_version: Mapped[str] = mapped_column(String(40))
    completed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
