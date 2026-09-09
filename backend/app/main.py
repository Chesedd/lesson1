import secrets
from contextlib import asynccontextmanager

from fastapi import Cookie, Depends, FastAPI, HTTPException, Response
from fastapi.middleware.cors import CORSMiddleware
from sqlalchemy import select
from sqlalchemy.orm import Session

from .config import settings
from .content import ContentRepository
from .database import get_db
from .models import ExerciseCompletion, Learner

COOKIE_NAME = "learner_session"


def get_content() -> ContentRepository:
    return app.state.content


def current_learner(
    response: Response,
    learner_session: str | None = Cookie(default=None),
    db: Session = Depends(get_db),
) -> Learner:
    learner = db.get(Learner, learner_session) if learner_session else None
    if learner is None:
        learner = Learner(id=secrets.token_urlsafe(32))
        db.add(learner)
        db.commit()
        response.set_cookie(
            COOKIE_NAME,
            learner.id,
            httponly=True,
            secure=settings.cookie_secure,
            samesite="lax",
            max_age=60 * 60 * 24 * 365,
        )
    return learner


@asynccontextmanager
async def lifespan(instance: FastAPI):
    instance.state.content = ContentRepository(settings.content_manifest_path)
    yield


app = FastAPI(title="Lesson 1 API", lifespan=lifespan)
app.add_middleware(
    CORSMiddleware,
    allow_origins=[settings.cors_origin],
    allow_credentials=True,
    allow_methods=["GET"],
    allow_headers=["Content-Type"],
)


@app.get("/api/lessons/{lesson_id}")
def lesson_detail(
    lesson_id: str,
    _learner: Learner = Depends(current_learner),
    content: ContentRepository = Depends(get_content),
) -> dict:
    lesson = content.public_lesson(lesson_id)
    if lesson is None:
        raise HTTPException(status_code=404, detail="Lesson not found")
    return lesson


@app.get("/api/lessons/{lesson_id}/progress")
def lesson_progress(
    lesson_id: str,
    learner: Learner = Depends(current_learner),
    content: ContentRepository = Depends(get_content),
    db: Session = Depends(get_db),
) -> dict:
    lesson = content.lesson(lesson_id)
    if lesson is None:
        raise HTTPException(status_code=404, detail="Lesson not found")
    exercise_ids = [exercise.id for block in lesson.blocks for exercise in block.exercises]
    completed_ids = list(
        db.scalars(
            select(ExerciseCompletion.exercise_id).where(
                ExerciseCompletion.learner_id == learner.id,
                ExerciseCompletion.content_version == lesson.content_version,
                ExerciseCompletion.exercise_id.in_(exercise_ids),
            )
        )
    )
    total = len(exercise_ids)
    completed = len(completed_ids)
    return {
        "lesson_id": lesson.id,
        "completed": completed,
        "total": total,
        "percent": round(completed / total * 100) if total else 0,
        "completed_exercise_ids": completed_ids,
    }
