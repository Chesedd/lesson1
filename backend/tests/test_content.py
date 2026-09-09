import json
from pathlib import Path
import pytest
from pydantic import ValidationError
from app.content import ContentRepository
ROOT=Path(__file__).parents[2]
def test_manifest_validates():
    assert ContentRepository.load(ROOT/"content/manifest.json").lessons[0].id=="pandas-intro"
def test_invalid_manifest_rejected(tmp_path):
    data=json.loads((ROOT/"content/manifest.json").read_text())
    data["lessons"][0]["blocks"][0]["exercises"].pop()
    path=tmp_path/"bad.json";path.write_text(json.dumps(data))
    with pytest.raises(ValidationError): ContentRepository.load(path)
def test_public_dto_excludes_private_fields():
    payload=ContentRepository(ROOT/"content/manifest.json").public_lesson("pandas-intro")
    serialized=json.dumps(payload)
    assert "grader" not in serialized and "hidden" not in serialized and "asset_ids" not in serialized
    assert "content/assets" not in serialized
