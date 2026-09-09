import json
from enum import StrEnum
from pathlib import Path
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator


StrictText = Annotated[str, Field(min_length=1)]
StableId = Annotated[str, Field(pattern=r"^[a-z0-9][a-z0-9-]*$")]


class Difficulty(StrEnum):
    BASIC = "basic"
    INTERMEDIATE = "intermediate"
    ADVANCED = "advanced"


class TextPart(BaseModel):
    model_config = ConfigDict(extra="forbid")
    type: Literal["paragraph"]
    text: StrictText


class CodePart(BaseModel):
    model_config = ConfigDict(extra="forbid")
    type: Literal["code"]
    language: Literal["python"]
    code: StrictText


class HeadingPart(BaseModel):
    model_config = ConfigDict(extra="forbid")
    type: Literal["heading"]
    text: StrictText


TheoryPart = Annotated[TextPart | CodePart | HeadingPart, Field(discriminator="type")]


class Theory(BaseModel):
    model_config = ConfigDict(extra="forbid")
    parts: Annotated[list[TheoryPart], Field(min_length=1)]


class PublicExample(BaseModel):
    model_config = ConfigDict(extra="forbid")
    input: str
    output: str


class Asset(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: StableId
    filename: Annotated[str, Field(pattern=r"^[A-Za-z0-9_.-]+$")]
    media_type: Literal["text/csv"]
    description: StrictText


class Exercise(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: StableId
    title: StrictText
    difficulty: Difficulty
    statement: StrictText
    starter_code: str
    hints: list[StrictText]
    public_examples: list[PublicExample] = []
    asset_ids: list[StableId] = []
    order: Annotated[int, Field(ge=1)]


class LearningBlock(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: StableId
    title: StrictText
    order: Annotated[int, Field(ge=1)]
    theory: Theory
    exercises: Annotated[list[Exercise], Field(min_length=3, max_length=3)]

    @model_validator(mode="after")
    def validate_exercises(self) -> "LearningBlock":
        if len({item.id for item in self.exercises}) != len(self.exercises):
            raise ValueError("exercise ids must be unique")
        if [item.order for item in self.exercises] != sorted(item.order for item in self.exercises):
            raise ValueError("exercises must be ordered")
        return self


class Lesson(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: StableId
    title: StrictText
    age_group: StrictText
    content_version: StrictText
    blocks: Annotated[list[LearningBlock], Field(min_length=1)]


class LearningTrack(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: StableId
    title: StrictText
    age_group: StrictText
    assets: list[Asset]
    lessons: Annotated[list[Lesson], Field(min_length=1)]

    @model_validator(mode="after")
    def validate_references_and_order(self) -> "LearningTrack":
        asset_ids = {asset.id for asset in self.assets}
        lesson_ids: set[str] = set()
        exercise_ids: set[str] = set()
        for lesson in self.lessons:
            if lesson.id in lesson_ids:
                raise ValueError("lesson ids must be unique")
            lesson_ids.add(lesson.id)
            if [block.order for block in lesson.blocks] != sorted(block.order for block in lesson.blocks):
                raise ValueError("blocks must be ordered")
            for block in lesson.blocks:
                for exercise in block.exercises:
                    if exercise.id in exercise_ids:
                        raise ValueError("exercise ids must be globally unique")
                    exercise_ids.add(exercise.id)
                    if not set(exercise.asset_ids) <= asset_ids:
                        raise ValueError("exercise references an unknown asset")
        return self


class ContentRepository:
    def __init__(self, path: Path):
        self.path = path
        self.track = self.load(path)

    @staticmethod
    def load(path: Path) -> LearningTrack:
        return LearningTrack.model_validate_json(path.read_text(encoding="utf-8"))

    def lesson(self, lesson_id: str) -> Lesson | None:
        return next((lesson for lesson in self.track.lessons if lesson.id == lesson_id), None)

    def public_lesson(self, lesson_id: str) -> dict | None:
        lesson = self.lesson(lesson_id)
        if not lesson:
            return None
        asset_by_id = {asset.id: asset for asset in self.track.assets}
        payload = lesson.model_dump(mode="json")
        for block in payload["blocks"]:
            for exercise in block["exercises"]:
                exercise["assets"] = [asset_by_id[item].model_dump(mode="json") for item in exercise.pop("asset_ids")]
        return payload
